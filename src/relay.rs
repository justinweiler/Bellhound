use std::{
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Bytes,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::{
    config::Config,
    output::{self, EventSink},
    protocol::{AgentCommand, AgentEvent, TelnyxFrame, telnyx_end_frame, telnyx_text_frame},
    telnyx,
};

#[derive(Clone)]
struct AppState {
    token: Arc<str>,
    config: Config,
    sink: EventSink,
    commands: broadcast::Sender<AgentCommand>,
    active_session: Arc<Mutex<Option<String>>>,
    active_call_control_id: Arc<StdMutex<Option<String>>>,
}

pub async fn run(config: Config) -> Result<()> {
    let token = std::env::var(&config.relay.token_env).with_context(|| {
        format!(
            "required relay token environment variable {} is not set",
            config.relay.token_env
        )
    })?;
    if token.len() < 24 {
        anyhow::bail!("relay token must contain at least 24 characters");
    }

    let (sink, writer) = output::channel();
    let writer_task = tokio::spawn(writer);
    let (commands, _) = broadcast::channel(32);
    let command_task = tokio::spawn(output::read_commands(
        config.protocol.max_speak_chars,
        sink.clone(),
        commands.clone(),
    ));

    sink.send(AgentEvent::Ready {
        version: env!("CARGO_PKG_VERSION"),
        bind: config.relay.bind.to_string(),
        websocket_path: format!("{}/<token>", config.relay.path),
    });

    let state = AppState {
        token: token.into(),
        config: config.clone(),
        sink,
        commands,
        active_session: Arc::new(Mutex::new(None)),
        active_call_control_id: Arc::new(StdMutex::new(None)),
    };
    let app = Router::new()
        .route(&config.route(), get(upgrade))
        .route(&config.webhook_route(), post(webhook))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(config.relay.bind).await?;
    tracing::info!(address = %config.relay.bind, "Bellhound relay listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    command_task.abort();
    writer_task.abort();
    Ok(())
}

async fn webhook(
    Path(token): Path<String>,
    State(state): State<AppState>,
    body: Bytes,
) -> StatusCode {
    if token.as_bytes() != state.token.as_bytes() {
        return StatusCode::UNAUTHORIZED;
    }
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            state.sink.send(AgentEvent::Error {
                code: "invalid_telnyx_webhook".to_owned(),
                message: error.to_string(),
                retryable: true,
            });
            return StatusCode::BAD_REQUEST;
        }
    };
    let data = value.get("data").unwrap_or(&serde_json::Value::Null);
    let payload = data.get("payload").unwrap_or(&serde_json::Value::Null);
    state.sink.send(AgentEvent::CallStatus {
        event_type: data
            .get("event_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        call_control_id: string_field(payload, "call_control_id"),
        call_session_id: string_field(payload, "call_session_id"),
        hangup_cause: string_field(payload, "hangup_cause"),
    });
    StatusCode::OK
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

async fn upgrade(
    Path(token): Path<String>,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    if token.as_bytes() != state.token.as_bytes() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if state.active_session.lock().await.is_some() {
        return StatusCode::CONFLICT.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let session_id = Uuid::new_v4().to_string();
    {
        let mut active = state.active_session.lock().await;
        if active.is_some() {
            return;
        }
        *active = Some(session_id.clone());
    }

    state.sink.send(AgentEvent::Connected {
        session_id: session_id.clone(),
    });

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut commands = state.commands.subscribe();
    let mut sequence = 0_u64;
    let timeout = tokio::time::sleep(Duration::from_secs(state.config.relay.max_call_seconds));
    tokio::pin!(timeout);
    let mut end_reason = "remote_closed".to_owned();

    loop {
        tokio::select! {
            _ = &mut timeout => {
                end_reason = "max_duration".to_owned();
                let frame = telnyx_end_frame(end_reason.clone());
                let _ = ws_sender.send(Message::Text(frame.to_string().into())).await;
                request_call_hangup(&state).await;
                let _ = ws_sender.send(Message::Close(None)).await;
                break;
            }
            command = commands.recv() => {
                match command {
                    Ok(AgentCommand::Speak { text, last }) => {
                        let frame = telnyx_text_frame(text.clone(), last);
                        if ws_sender.send(Message::Text(frame.to_string().into())).await.is_err() {
                            end_reason = "send_failed".to_owned();
                            break;
                        }
                        state.sink.send(AgentEvent::Spoken {
                            session_id: session_id.clone(),
                            text,
                        });
                    }
                    Ok(AgentCommand::Hangup { reason }) => {
                        end_reason = reason.unwrap_or_else(|| "agent_hangup".to_owned());
                        let frame = telnyx_end_frame(end_reason.clone());
                        let _ = ws_sender.send(Message::Text(frame.to_string().into())).await;
                        request_call_hangup(&state).await;
                        let _ = ws_sender.send(Message::Close(None)).await;
                        break;
                    }
                    Ok(AgentCommand::Ping) => state.sink.send(AgentEvent::Pong),
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state.sink.send(AgentEvent::Error {
                            code: "command_lag".to_owned(),
                            message: format!("skipped {skipped} commands"),
                            retryable: false,
                        });
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        end_reason = "command_channel_closed".to_owned();
                        break;
                    }
                }
            }
            message = ws_receiver.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        handle_telnyx_text(&state, &session_id, &mut sequence, text.as_str());
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if ws_sender.send(Message::Pong(payload)).await.is_err() {
                            end_reason = "pong_failed".to_owned();
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        end_reason = "websocket_error".to_owned();
                        state.sink.send(AgentEvent::Error {
                            code: "websocket_error".to_owned(),
                            message: error.to_string(),
                            retryable: false,
                        });
                        break;
                    }
                }
            }
        }
    }

    {
        let mut active = state.active_session.lock().await;
        if active.as_deref() == Some(&session_id) {
            *active = None;
        }
    }
    if let Ok(mut call_control_id) = state.active_call_control_id.lock() {
        *call_control_id = None;
    }
    state.sink.send(AgentEvent::Ended {
        session_id,
        reason: end_reason,
    });
}

fn handle_telnyx_text(state: &AppState, session_id: &str, sequence: &mut u64, source: &str) {
    let value: serde_json::Value = match serde_json::from_str(source) {
        Ok(value) => value,
        Err(error) => {
            state.sink.send(AgentEvent::Error {
                code: "invalid_telnyx_frame".to_owned(),
                message: error.to_string(),
                retryable: true,
            });
            return;
        }
    };
    let frame: TelnyxFrame = match serde_json::from_value(value.clone()) {
        Ok(frame) => frame,
        Err(error) => {
            state.sink.send(AgentEvent::Error {
                code: "invalid_telnyx_frame".to_owned(),
                message: error.to_string(),
                retryable: true,
            });
            return;
        }
    };

    match frame.kind.as_str() {
        "prompt" => {
            let final_utterance = frame.last.unwrap_or(false);
            if final_utterance || state.config.relay.emit_partial_transcripts {
                *sequence += 1;
                state.sink.send(AgentEvent::Utterance {
                    session_id: session_id.to_owned(),
                    sequence: *sequence,
                    text: frame.voice_prompt.unwrap_or_default(),
                    language: frame.lang,
                    final_utterance,
                });
            }
        }
        "interrupt" => state.sink.send(AgentEvent::Interrupted {
            session_id: session_id.to_owned(),
        }),
        "dtmf" => state.sink.send(AgentEvent::Dtmf {
            session_id: session_id.to_owned(),
            digit: frame.digit.unwrap_or_default(),
        }),
        "error" => state.sink.send(AgentEvent::Error {
            code: "telnyx_relay_error".to_owned(),
            message: frame
                .description
                .unwrap_or_else(|| "Telnyx reported an unspecified relay error".to_owned()),
            retryable: false,
        }),
        "setup" => {
            if let Some(call_control_id) = frame.call_control_id.as_ref()
                && let Ok(mut active_call_control_id) = state.active_call_control_id.lock()
            {
                *active_call_control_id = Some(call_control_id.clone());
            }
            state.sink.send(AgentEvent::Setup {
                session_id: session_id.to_owned(),
                telnyx_session_id: frame.session_id,
                call_control_id: frame.call_control_id,
                call_session_id: frame.call_session_id,
                direction: frame.direction,
                call_status: frame.call_status,
            });
        }
        _ => state.sink.send(AgentEvent::Transport {
            session_id: session_id.to_owned(),
            kind: frame.kind,
        }),
    }
}

async fn request_call_hangup(state: &AppState) {
    let call_control_id = state
        .active_call_control_id
        .lock()
        .ok()
        .and_then(|value| value.clone());
    let Some(call_control_id) = call_control_id else {
        state.sink.send(AgentEvent::Error {
            code: "missing_call_control_id".to_owned(),
            message: "cannot hang up the telephone leg before Telnyx setup completes".to_owned(),
            retryable: false,
        });
        return;
    };
    if let Err(error) = telnyx::hangup(&state.config, &call_control_id).await {
        state.sink.send(AgentEvent::Error {
            code: "telnyx_hangup_failed".to_owned(),
            message: error.to_string(),
            retryable: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn final_prompt_becomes_utterance() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sink = crate::output::EventSink { sender };
        let (commands, _) = broadcast::channel(4);
        let state = AppState {
            token: "abcdefghijklmnopqrstuvwxyz".into(),
            config: Config::default(),
            sink,
            commands,
            active_session: Arc::new(Mutex::new(None)),
            active_call_control_id: Arc::new(StdMutex::new(None)),
        };
        let mut sequence = 0;
        handle_telnyx_text(
            &state,
            "session-1",
            &mut sequence,
            r#"{"type":"prompt","voicePrompt":"hello","lang":"en","last":true}"#,
        );
        let event = receiver.recv().await.unwrap();
        assert_eq!(
            event,
            AgentEvent::Utterance {
                session_id: "session-1".to_owned(),
                sequence: 1,
                text: "hello".to_owned(),
                language: Some("en".to_owned()),
                final_utterance: true,
            }
        );
    }

    #[tokio::test]
    async fn setup_event_omits_phone_numbers() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sink = crate::output::EventSink { sender };
        let (commands, _) = broadcast::channel(4);
        let state = AppState {
            token: "abcdefghijklmnopqrstuvwxyz".into(),
            config: Config::default(),
            sink,
            commands,
            active_session: Arc::new(Mutex::new(None)),
            active_call_control_id: Arc::new(StdMutex::new(None)),
        };
        let mut sequence = 0;
        handle_telnyx_text(
            &state,
            "bellhound-session",
            &mut sequence,
            r#"{"type":"setup","sessionId":"relay-session","callControlId":"call-control","callSessionId":"call-session","from":"+14245550100","to":"+18185550100","direction":"outbound","callStatus":"active"}"#,
        );
        let event = receiver.recv().await.unwrap();
        let serialized = serde_json::to_string(&event).unwrap();
        assert_eq!(
            event,
            AgentEvent::Setup {
                session_id: "bellhound-session".to_owned(),
                telnyx_session_id: Some("relay-session".to_owned()),
                call_control_id: Some("call-control".to_owned()),
                call_session_id: Some("call-session".to_owned()),
                direction: Some("outbound".to_owned()),
                call_status: Some("active".to_owned()),
            }
        );
        assert!(!serialized.contains("+14245550100"));
        assert!(!serialized.contains("+18185550100"));
        assert_eq!(
            state.active_call_control_id.lock().unwrap().as_deref(),
            Some("call-control")
        );
    }
}
