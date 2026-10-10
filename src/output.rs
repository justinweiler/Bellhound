use std::io;

use anyhow::Result;
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::mpsc,
};

use crate::protocol::AgentEvent;

#[derive(Clone)]
pub struct EventSink {
    pub(crate) sender: mpsc::UnboundedSender<AgentEvent>,
}

impl EventSink {
    pub fn send(&self, event: AgentEvent) {
        let _ = self.sender.send(event);
    }
}

pub fn channel() -> (EventSink, impl std::future::Future<Output = Result<()>>) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<AgentEvent>();
    let sink = EventSink { sender };
    let writer = async move {
        let mut stdout = BufWriter::new(tokio::io::stdout());
        while let Some(event) = receiver.recv().await {
            let mut line = serde_json::to_vec(&event)?;
            line.push(b'\n');
            stdout.write_all(&line).await?;
            stdout.flush().await?;
        }
        Ok(())
    };
    (sink, writer)
}

pub async fn read_commands(
    max_speak_chars: usize,
    sink: EventSink,
    sender: tokio::sync::broadcast::Sender<crate::protocol::AgentCommand>,
) -> Result<()> {
    use tokio::io::AsyncBufReadExt;

    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<crate::protocol::AgentCommand>(&line) {
            Ok(crate::protocol::AgentCommand::Speak { text, .. })
                if text.chars().count() > max_speak_chars =>
            {
                sink.send(AgentEvent::Error {
                    code: "speak_too_long".to_owned(),
                    message: format!("speak text exceeds {max_speak_chars} characters"),
                    retryable: true,
                });
            }
            Ok(command) => {
                if sender.send(command).is_err() {
                    sink.send(AgentEvent::Error {
                        code: "no_active_session".to_owned(),
                        message: "no telephone session is listening".to_owned(),
                        retryable: true,
                    });
                }
            }
            Err(error) => sink.send(AgentEvent::Error {
                code: "invalid_command".to_owned(),
                message: error.to_string(),
                retryable: true,
            }),
        }
    }
    Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed").into())
}
