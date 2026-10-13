use std::{
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast, mpsc};
use uuid::Uuid;

use crate::{
    config::{AcknowledgementMode, Config},
    output::{self, EventSink, NextEvent},
    protocol::{
        AgentCommand, AgentEvent, TelnyxFrame, telnyx_end_frame, telnyx_play_frame,
        telnyx_text_frame,
    },
    telnyx,
};

const THINKING_SOUND: &[u8] = include_bytes!("../assets/bellhound-thinking.mp3");

#[derive(Clone)]
struct AppState {
    token: Arc<str>,
    config: Config,
    sink: EventSink,
    commands: broadcast::Sender<AgentCommand>,
    active_session: Arc<Mutex<Option<String>>>,
    active_call_control_id: Arc<StdMutex<Option<String>>>,
    pending_opening: Arc<StdMutex<Option<PendingOpening>>>,
}

struct PendingOpening {
    text: String,
    expires_at: Instant,
}

struct PendingTurn {
    sequence: u64,
    received_at: Instant,
    acknowledgement_sent: bool,
}

struct PendingIncomingTurn {
    generation: u64,
    text: String,
    language: Option<String>,
    fragment_count: usize,
}

struct FinalUtteranceFragment {
    text: String,
    language: Option<String>,
}

#[derive(Default)]
struct FrameOutcome {
    opening: Option<String>,
    final_fragment: Option<FinalUtteranceFragment>,
    prompt_activity: bool,
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

    let listener = tokio::net::TcpListener::bind(config.relay.bind).await?;
    let control_listener = tokio::net::TcpListener::bind(config.relay.control_bind).await?;

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
        pending_opening: Arc::new(StdMutex::new(None)),
    };
    let app = Router::new()
        .route(&config.route(), get(upgrade))
        .route(
            &config.acknowledgement_audio_route(),
            get(acknowledgement_audio),
        )
        .route(&config.webhook_route(), post(webhook))
        .with_state(state.clone());
    let control_app = Router::new()
        .route("/v1/events/next", get(next_event))
        .route("/v1/commands", post(control_command))
        .route("/v1/opening", post(prepare_opening).delete(clear_opening))
        .with_state(state);
    tracing::info!(address = %config.relay.bind, "Bellhound relay listening");
    tracing::info!(address = %config.relay.control_bind, "Bellhound control listener ready");

    let mut relay_task = tokio::spawn(async move { axum::serve(listener, app).await });
    let mut control_task =
        tokio::spawn(async move { axum::serve(control_listener, control_app).await });
    tokio::select! {
        result = &mut relay_task => result??,
        result = &mut control_task => result??,
        _ = tokio::signal::ctrl_c() => {}
    }
    relay_task.abort();
    control_task.abort();

    command_task.abort();
    writer_task.abort();
    Ok(())
}

#[derive(Deserialize)]
struct WaitQuery {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_wait_seconds")]
    timeout_seconds: u64,
}

fn default_wait_seconds() -> u64 {
    120
}

fn control_authorized(headers: &HeaderMap, state: &AppState) -> bool {
    headers
        .get("x-bellhound-token")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.as_bytes() == state.token.as_bytes())
}

#[derive(Serialize)]
struct HistoryGapResponse {
    error: &'static str,
    after: u64,
    oldest_event_id: u64,
}

async fn next_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WaitQuery>,
) -> Response {
    if !control_authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let wait = Duration::from_secs(query.timeout_seconds.clamp(1, 300));
    match state.sink.next_after(query.after, wait).await {
        Some(NextEvent::Event(event)) => Json(event.as_ref().clone()).into_response(),
        Some(NextEvent::HistoryGap { oldest_event_id }) => (
            StatusCode::CONFLICT,
            Json(HistoryGapResponse {
                error: "event_history_gap",
                after: query.after,
                oldest_event_id,
            }),
        )
            .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn control_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(command): Json<AgentCommand>,
) -> StatusCode {
    if !control_authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED;
    }
    output::dispatch_command(
        command,
        state.config.protocol.max_speak_chars,
        &state.sink,
        &state.commands,
    );
    StatusCode::ACCEPTED
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningRequest {
    text: String,
}

async fn prepare_opening(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<OpeningRequest>,
) -> StatusCode {
    if !control_authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED;
    }
    let text = request.text.trim();
    if text.is_empty() || text.chars().count() > state.config.protocol.max_speak_chars {
        return StatusCode::BAD_REQUEST;
    }
    if state.active_session.lock().await.is_some() {
        return StatusCode::CONFLICT;
    }
    let expires_at =
        Instant::now() + Duration::from_secs(state.config.relay.max_call_seconds.max(30) + 60);
    match state.pending_opening.lock() {
        Ok(mut opening) => {
            *opening = Some(PendingOpening {
                text: text.to_owned(),
                expires_at,
            });
            StatusCode::ACCEPTED
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn clear_opening(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
    if !control_authorized(&headers, &state) {
        return StatusCode::UNAUTHORIZED;
    }
    match state.pending_opening.lock() {
        Ok(mut opening) => {
            *opening = None;
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
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

async fn acknowledgement_audio(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Response {
    if token.as_bytes() != state.token.as_bytes() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let session_id = state.active_session.lock().await.clone();
    state.sink.send(AgentEvent::AcknowledgementAudioFetched {
        session_id: session_id.clone(),
        bytes: THINKING_SOUND.len(),
    });
    tracing::info!(
        session_id = session_id.as_deref().unwrap_or("none"),
        bytes = THINKING_SOUND.len(),
        "thinking audio fetched"
    );
    (
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        Body::from(Bytes::from_static(THINKING_SOUND)),
    )
        .into_response()
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
    let (acknowledgement_due_tx, mut acknowledgement_due_rx) = mpsc::unbounded_channel();
    let (turn_due_tx, mut turn_due_rx) = mpsc::unbounded_channel();
    let mut sequence = 0_u64;
    let mut pending_turn: Option<PendingTurn> = None;
    let mut pending_incoming_turn: Option<PendingIncomingTurn> = None;
    let mut incoming_generation = 0_u64;
    let mut acknowledgements_suppressed_for_session = false;
    let timeout = tokio::time::sleep(Duration::from_secs(state.config.relay.max_call_seconds));
    tokio::pin!(timeout);
    let mut end_reason = "remote_closed".to_owned();

    loop {
        tokio::select! {
            biased;
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
                    Ok(AgentCommand::Speak {
                        text,
                        last,
                        responding_to_sequence,
                    }) => {
                        if responding_to_sequence.is_none()
                            && (pending_turn.is_some() || pending_incoming_turn.is_some())
                        {
                            state.sink.send(AgentEvent::Error {
                                code: "missing_response_sequence".to_owned(),
                                message: "an unanswered caller turn is active; supply responding_to_sequence"
                                    .to_owned(),
                                retryable: true,
                            });
                            continue;
                        }
                        if let Some(target) = responding_to_sequence {
                            let active = pending_turn.as_ref().map(|turn| turn.sequence);
                            if pending_incoming_turn.is_some() || active != Some(target) {
                                state.sink.send(AgentEvent::Error {
                                    code: "stale_response".to_owned(),
                                    message: if pending_incoming_turn.is_some() {
                                        format!(
                                            "response targets utterance {target} while a newer caller turn is still being buffered"
                                        )
                                    } else { match active {
                                        Some(active) => format!(
                                            "response targets utterance {target} but active utterance is {active}"
                                        ),
                                        None => format!(
                                            "response targets utterance {target} but no unanswered utterance is active"
                                        ),
                                    }},
                                    retryable: true,
                                });
                                continue;
                            }
                        }
                        let response = pending_turn.take();
                        let frame = telnyx_text_frame(text.clone(), last);
                        if ws_sender.send(Message::Text(frame.to_string().into())).await.is_err() {
                            end_reason = "send_failed".to_owned();
                            break;
                        }
                        state.sink.send(AgentEvent::Spoken {
                            session_id: session_id.clone(),
                            text,
                            responding_to_sequence: response.as_ref().map(|turn| turn.sequence),
                            response_latency_ms: response.as_ref().map(|turn| {
                                elapsed_millis(turn.received_at.elapsed())
                            }),
                            acknowledgement_preceded: response
                                .is_some_and(|turn| turn.acknowledgement_sent),
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
                        let outcome = handle_telnyx_text(
                            &state,
                            &session_id,
                            &mut sequence,
                            text.as_str(),
                        );
                        if let Some(opening) = outcome.opening {
                            let frame = telnyx_text_frame(opening.clone(), true);
                            if ws_sender
                                .send(Message::Text(frame.to_string().into()))
                                .await
                                .is_err()
                            {
                                end_reason = "opening_send_failed".to_owned();
                                break;
                            }
                            state.sink.send(AgentEvent::Spoken {
                                session_id: session_id.clone(),
                                text: opening,
                                responding_to_sequence: None,
                                response_latency_ms: None,
                                acknowledgement_preceded: false,
                            });
                        }
                        if let Some(fragment) = outcome.final_fragment {
                            incoming_generation = incoming_generation.saturating_add(1);
                            match pending_incoming_turn.as_mut() {
                                Some(turn) => {
                                    turn.generation = incoming_generation;
                                    turn.text = merge_transcript_fragments(&turn.text, &fragment.text);
                                    turn.language = fragment.language.or_else(|| turn.language.clone());
                                    turn.fragment_count = turn.fragment_count.saturating_add(1);
                                }
                                None => {
                                    pending_incoming_turn = Some(PendingIncomingTurn {
                                        generation: incoming_generation,
                                        text: fragment.text,
                                        language: fragment.language,
                                        fragment_count: 1,
                                    });
                                }
                            }
                            schedule_turn_flush(
                                turn_due_tx.clone(),
                                incoming_generation,
                                state.config.relay.continuation_window_ms,
                            );
                        } else if outcome.prompt_activity
                            && let Some(turn) = pending_incoming_turn.as_mut()
                        {
                            incoming_generation = incoming_generation.saturating_add(1);
                            turn.generation = incoming_generation;
                            schedule_turn_flush(
                                turn_due_tx.clone(),
                                incoming_generation,
                                state.config.relay.continuation_window_ms,
                            );
                        }
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
            due = turn_due_rx.recv() => {
                let Some(due_generation) = due else {
                    continue;
                };
                let Some(buffered) = pending_incoming_turn.as_ref() else {
                    continue;
                };
                if buffered.generation != due_generation {
                    continue;
                }
                let buffered = pending_incoming_turn.take().expect("buffer checked above");
                sequence = sequence.saturating_add(1);
                let final_sequence = sequence;
                state.sink.send(AgentEvent::Utterance {
                    session_id: session_id.clone(),
                    sequence: final_sequence,
                    text: buffered.text.clone(),
                    language: buffered.language,
                    final_utterance: true,
                    fragment_count: buffered.fragment_count,
                });

                let acknowledgement_sent = pending_turn
                    .as_ref()
                    .is_some_and(|turn| turn.acknowledgement_sent);
                if matches_suppression_phrases(
                    &buffered.text,
                    &state.config.acknowledgement.suppress_for_session_phrases,
                ) {
                    acknowledgements_suppressed_for_session = true;
                }
                pending_turn = Some(PendingTurn {
                    sequence: final_sequence,
                    received_at: Instant::now(),
                    acknowledgement_sent,
                });
                if state.config.acknowledgement.enabled
                    && !acknowledgement_sent
                    && !acknowledgements_suppressed_for_session
                    && should_acknowledge(&state.config, &buffered.text)
                {
                    let sender = acknowledgement_due_tx.clone();
                    let delay = Duration::from_millis(state.config.acknowledgement.delay_ms);
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        let _ = sender.send(final_sequence);
                    });
                }
            }
            due = acknowledgement_due_rx.recv() => {
                let Some(due_sequence) = due else {
                    continue;
                };
                let Some(turn) = pending_turn.as_mut() else {
                    continue;
                };
                if turn.sequence != due_sequence || turn.acknowledgement_sent {
                    continue;
                }
                let text = state.config.acknowledgement.text.clone();
                let (frame, mode, spoken_text) = match state.config.acknowledgement.mode {
                    AcknowledgementMode::Sound => match acknowledgement_sound_url(&state) {
                        Some(source) => (telnyx_play_frame(source), "sound", None),
                        None => (
                            telnyx_text_frame(text.clone(), true),
                            "speech_fallback",
                            Some(text.clone()),
                        ),
                    },
                    AcknowledgementMode::Speech => {
                        (telnyx_text_frame(text.clone(), true), "speech", Some(text.clone()))
                    }
                };
                if ws_sender.send(Message::Text(frame.to_string().into())).await.is_err() {
                    end_reason = "acknowledgement_send_failed".to_owned();
                    break;
                }
                turn.acknowledgement_sent = true;
                state.sink.send(AgentEvent::Acknowledged {
                    session_id: session_id.clone(),
                    responding_to_sequence: turn.sequence,
                    mode: mode.to_owned(),
                    text: spoken_text,
                    response_latency_ms: elapsed_millis(turn.received_at.elapsed()),
                });
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
    if let Ok(mut opening) = state.pending_opening.lock() {
        *opening = None;
    }
    state.sink.send(AgentEvent::Ended {
        session_id,
        reason: end_reason,
    });
}

fn handle_telnyx_text(
    state: &AppState,
    session_id: &str,
    sequence: &mut u64,
    source: &str,
) -> FrameOutcome {
    let value: serde_json::Value = match serde_json::from_str(source) {
        Ok(value) => value,
        Err(error) => {
            state.sink.send(AgentEvent::Error {
                code: "invalid_telnyx_frame".to_owned(),
                message: error.to_string(),
                retryable: true,
            });
            return FrameOutcome::default();
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
            return FrameOutcome::default();
        }
    };

    let mut outcome = FrameOutcome::default();
    match frame.kind.as_str() {
        "prompt" => {
            outcome.prompt_activity = true;
            let final_utterance = frame.last.unwrap_or(false);
            let text = frame.voice_prompt.unwrap_or_default();
            if final_utterance {
                if !text.trim().is_empty() {
                    outcome.final_fragment = Some(FinalUtteranceFragment {
                        text,
                        language: frame.lang,
                    });
                }
            } else if state.config.relay.emit_partial_transcripts {
                *sequence += 1;
                state.sink.send(AgentEvent::Utterance {
                    session_id: session_id.to_owned(),
                    sequence: *sequence,
                    text,
                    language: frame.lang,
                    final_utterance: false,
                    fragment_count: 1,
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
            if let Ok(mut pending) = state.pending_opening.lock()
                && let Some(candidate) = pending.take()
                && candidate.expires_at > Instant::now()
            {
                outcome.opening = Some(candidate.text);
            }
        }
        _ => state.sink.send(AgentEvent::Transport {
            session_id: session_id.to_owned(),
            kind: frame.kind,
        }),
    }
    outcome
}

fn schedule_turn_flush(sender: mpsc::UnboundedSender<u64>, generation: u64, delay_ms: u64) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        let _ = sender.send(generation);
    });
}

fn merge_transcript_fragments(current: &str, next: &str) -> String {
    let current = current.trim();
    let next = next.trim();
    if current.is_empty() {
        return next.to_owned();
    }
    if next.is_empty() {
        return current.to_owned();
    }

    let current_lower = current.to_lowercase();
    let next_lower = next.to_lowercase();
    if next_lower.starts_with(&current_lower) {
        return next.to_owned();
    }
    if current_lower.ends_with(&next_lower) {
        return current.to_owned();
    }

    format!("{current} {next}")
}

fn should_acknowledge(config: &Config, text: &str) -> bool {
    !matches_suppression_phrases(text, &config.acknowledgement.suppress_phrases)
}

fn matches_suppression_phrases(text: &str, phrases: &[String]) -> bool {
    let normalized = normalize_phrase(text);
    if normalized.is_empty() {
        return false;
    }
    let word_count = normalized.split_whitespace().count();
    let mut boundary_matches = 0_usize;
    for phrase in phrases {
        let phrase = normalize_phrase(phrase);
        if phrase.is_empty() {
            continue;
        }
        let phrase_words = phrase.split_whitespace().count();
        if normalized == phrase || (phrase_words >= 4 && normalized.contains(&phrase)) {
            return true;
        }
        let trailing_words = normalized.strip_prefix(&format!("{phrase} "));
        let leading_words = normalized.strip_suffix(&format!(" {phrase}"));
        let boundary_match = trailing_words.is_some() || leading_words.is_some();
        if boundary_match {
            boundary_matches = boundary_matches.saturating_add(1);
            if word_count <= phrase_words.saturating_add(1) {
                return true;
            }
            if trailing_words.is_some_and(is_short_possessive_vocative)
                || leading_words.is_some_and(is_short_possessive_vocative)
            {
                return true;
            }
        }
    }
    boundary_matches >= 2
}

fn is_short_possessive_vocative(value: &str) -> bool {
    let mut words = value.split_whitespace();
    matches!(
        (words.next(), words.next(), words.next()),
        (Some("my"), Some(_), None)
    )
}

fn normalize_phrase(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn acknowledgement_sound_url(state: &AppState) -> Option<String> {
    if let Some(sound_url) = &state.config.acknowledgement.sound_url {
        return Some(sound_url.clone());
    }
    let base = state
        .config
        .telnyx
        .as_ref()?
        .public_wss_base
        .trim_end_matches('/');
    let base = base
        .strip_prefix("wss://")
        .map(|rest| format!("https://{rest}"))
        .or_else(|| {
            base.strip_prefix("ws://")
                .map(|rest| format!("http://{rest}"))
        })?;
    Some(format!("{}/{}/thinking.mp3", base, state.token))
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

    #[test]
    fn final_prompt_becomes_bufferable_fragment() {
        let (sink, _writer) = crate::output::channel();
        let (commands, _) = broadcast::channel(4);
        let state = AppState {
            token: "abcdefghijklmnopqrstuvwxyz".into(),
            config: Config::default(),
            sink: sink.clone(),
            commands,
            active_session: Arc::new(Mutex::new(None)),
            active_call_control_id: Arc::new(StdMutex::new(None)),
            pending_opening: Arc::new(StdMutex::new(None)),
        };
        let mut sequence = 0;
        let outcome = handle_telnyx_text(
            &state,
            "session-1",
            &mut sequence,
            r#"{"type":"prompt","voicePrompt":"hello","lang":"en","last":true}"#,
        );
        let fragment = outcome.final_fragment.expect("expected final fragment");
        assert_eq!(fragment.text, "hello");
        assert_eq!(fragment.language.as_deref(), Some("en"));
        assert!(outcome.prompt_activity);
        assert_eq!(sequence, 0);
    }

    #[test]
    fn transcript_fragments_merge_without_repeating_cumulative_text() {
        assert_eq!(
            merge_transcript_fragments("please check", "the monitor"),
            "please check the monitor"
        );
        assert_eq!(
            merge_transcript_fragments("please check", "please check the monitor"),
            "please check the monitor"
        );
        assert_eq!(
            merge_transcript_fragments("please check the monitor", "the monitor"),
            "please check the monitor"
        );
    }

    #[test]
    fn acknowledgement_suppresses_social_and_session_wide_voicemail_phrases() {
        let config = Config::default();
        assert!(!should_acknowledge(&config, "Thank you, love."));
        assert!(!should_acknowledge(&config, "Goodbye."));
        assert!(!should_acknowledge(&config, "Goodbye, my dear."));
        assert!(!should_acknowledge(&config, "My dear, goodbye."));
        assert!(!should_acknowledge(&config, "Thank you, hotness, goodbye."));
        assert!(should_acknowledge(
            &config,
            "Thank you, now check the monitor."
        ));
        assert!(should_acknowledge(&config, "Goodbye, check the monitor."));
        assert!(matches_suppression_phrases(
            "Your call has been forwarded to an automated voice messaging system.",
            &config.acknowledgement.suppress_for_session_phrases,
        ));
        assert!(should_acknowledge(
            &config,
            "Please check the live monitor."
        ));
    }

    #[tokio::test]
    async fn setup_event_omits_phone_numbers() {
        let (sink, _writer) = crate::output::channel();
        let (commands, _) = broadcast::channel(4);
        let state = AppState {
            token: "abcdefghijklmnopqrstuvwxyz".into(),
            config: Config::default(),
            sink: sink.clone(),
            commands,
            active_session: Arc::new(Mutex::new(None)),
            active_call_control_id: Arc::new(StdMutex::new(None)),
            pending_opening: Arc::new(StdMutex::new(None)),
        };
        let mut sequence = 0;
        handle_telnyx_text(
            &state,
            "bellhound-session",
            &mut sequence,
            r#"{"type":"setup","sessionId":"relay-session","callControlId":"call-control","callSessionId":"call-session","from":"+14245550100","to":"+18185550100","direction":"outbound","callStatus":"active"}"#,
        );
        let Some(NextEvent::Event(event)) = sink.next_after(0, Duration::from_millis(10)).await
        else {
            panic!("expected setup event");
        };
        let serialized = serde_json::to_string(event.as_ref()).unwrap();
        assert_eq!(
            event.event,
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
