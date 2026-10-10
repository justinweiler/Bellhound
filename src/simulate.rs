use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::io::AsyncBufReadExt;
use uuid::Uuid;

use crate::{
    config::Config,
    output,
    protocol::{AgentCommand, AgentEvent, TelnyxFrame},
};

pub async fn run(config: Config, script: PathBuf) -> Result<()> {
    let source = tokio::fs::read_to_string(&script)
        .await
        .with_context(|| format!("failed to read simulation script {}", script.display()))?;
    let prompts = parse_script(&source)?;
    if prompts.is_empty() {
        anyhow::bail!("simulation script contains no final prompt frames");
    }

    let (sink, writer) = output::channel();
    let writer_task = tokio::spawn(writer);
    let session_id = format!("sim-{}", Uuid::new_v4());

    sink.send(AgentEvent::Ready {
        version: env!("CARGO_PKG_VERSION"),
        bind: "simulation".to_owned(),
        websocket_path: "simulation".to_owned(),
    });
    sink.send(AgentEvent::Connected {
        session_id: session_id.clone(),
    });

    let mut prompts = prompts.into_iter();
    let mut sequence = 1_u64;
    emit_prompt(&sink, &session_id, sequence, prompts.next().unwrap());

    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<AgentCommand>(&line) {
            Ok(AgentCommand::Speak { text, .. }) => {
                if text.chars().count() > config.protocol.max_speak_chars {
                    sink.send(AgentEvent::Error {
                        code: "speak_too_long".to_owned(),
                        message: format!(
                            "speak text exceeds {} characters",
                            config.protocol.max_speak_chars
                        ),
                        retryable: true,
                    });
                    continue;
                }
                sink.send(AgentEvent::Spoken {
                    session_id: session_id.clone(),
                    text,
                });
                match prompts.next() {
                    Some(prompt) => {
                        sequence += 1;
                        emit_prompt(&sink, &session_id, sequence, prompt);
                    }
                    None => {
                        sink.send(AgentEvent::Ended {
                            session_id: session_id.clone(),
                            reason: "simulation_complete".to_owned(),
                        });
                        break;
                    }
                }
            }
            Ok(AgentCommand::Hangup { reason }) => {
                sink.send(AgentEvent::Ended {
                    session_id: session_id.clone(),
                    reason: reason.unwrap_or_else(|| "agent_hangup".to_owned()),
                });
                break;
            }
            Ok(AgentCommand::Ping) => sink.send(AgentEvent::Pong),
            Err(error) => sink.send(AgentEvent::Error {
                code: "invalid_command".to_owned(),
                message: error.to_string(),
                retryable: true,
            }),
        }
    }

    drop(sink);
    writer_task.await??;
    Ok(())
}

fn parse_script(source: &str) -> Result<Vec<TelnyxFrame>> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|(index, line)| {
            let frame: TelnyxFrame = serde_json::from_str(line)
                .with_context(|| format!("invalid JSON on simulation line {}", index + 1))?;
            if frame.kind != "prompt" || frame.last != Some(true) {
                anyhow::bail!(
                    "simulation line {} must be a final Telnyx prompt frame",
                    index + 1
                );
            }
            Ok(frame)
        })
        .collect()
}

fn emit_prompt(sink: &output::EventSink, session_id: &str, sequence: u64, frame: TelnyxFrame) {
    sink.send(AgentEvent::Utterance {
        session_id: session_id.to_owned(),
        sequence,
        text: frame.voice_prompt.unwrap_or_default(),
        language: frame.lang,
        final_utterance: true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_requires_final_prompts() {
        let error =
            parse_script(r#"{"type":"prompt","voicePrompt":"hmm","last":false}"#).unwrap_err();
        assert!(error.to_string().contains("final Telnyx prompt"));
    }

    #[test]
    fn script_allows_comments_and_blanks() {
        let script = r#"
            # Justin enters the crocodile pit.
            {"type":"prompt","voicePrompt":"I won","lang":"en","last":true}

        "#;
        assert_eq!(parse_script(script).unwrap().len(), 1);
    }
}
