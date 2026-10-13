use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use serde::Serialize;
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::{Notify, mpsc},
};

use crate::protocol::{AgentCommand, AgentEvent};

const EVENT_HISTORY_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelope {
    pub event_id: u64,
    pub emitted_at_unix_ms: u128,
    #[serde(flatten)]
    pub event: AgentEvent,
}

pub enum NextEvent {
    Event(Arc<EventEnvelope>),
    HistoryGap { oldest_event_id: u64 },
}

pub enum NextEvents {
    Events(Vec<Arc<EventEnvelope>>),
    HistoryGap { oldest_event_id: u64 },
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct EventHistoryStatus {
    pub oldest_event_id: Option<u64>,
    pub latest_event_id: Option<u64>,
}

struct EventHistory {
    next_id: AtomicU64,
    events: StdMutex<VecDeque<Arc<EventEnvelope>>>,
    notify: Notify,
}

#[derive(Clone)]
pub struct EventSink {
    sender: mpsc::UnboundedSender<Arc<EventEnvelope>>,
    history: Arc<EventHistory>,
}

impl EventSink {
    pub fn send(&self, event: AgentEvent) {
        let envelope = Arc::new(EventEnvelope {
            event_id: self.history.next_id.fetch_add(1, Ordering::Relaxed),
            emitted_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            event,
        });
        if let Ok(mut events) = self.history.events.lock() {
            events.push_back(envelope.clone());
            while events.len() > EVENT_HISTORY_CAPACITY {
                events.pop_front();
            }
        }
        let _ = self.sender.send(envelope);
        self.history.notify.notify_waiters();
    }

    pub async fn next_after(&self, after_event_id: u64, wait: Duration) -> Option<NextEvent> {
        tokio::time::timeout(wait, async {
            loop {
                let notified = self.history.notify.notified();
                if let Ok(events) = self.history.events.lock() {
                    if let Some(oldest) = events.front()
                        && after_event_id.saturating_add(1) < oldest.event_id
                    {
                        return NextEvent::HistoryGap {
                            oldest_event_id: oldest.event_id,
                        };
                    }
                    if let Some(event) = events.iter().find(|event| event.event_id > after_event_id)
                    {
                        return NextEvent::Event(event.clone());
                    }
                }
                notified.await;
            }
        })
        .await
        .ok()
    }

    pub async fn events_after(
        &self,
        after_event_id: u64,
        limit: usize,
        wait: Duration,
    ) -> Option<NextEvents> {
        let limit = limit.clamp(1, EVENT_HISTORY_CAPACITY);
        tokio::time::timeout(wait, async {
            loop {
                let notified = self.history.notify.notified();
                if let Ok(events) = self.history.events.lock() {
                    if let Some(oldest) = events.front()
                        && after_event_id.saturating_add(1) < oldest.event_id
                    {
                        return NextEvents::HistoryGap {
                            oldest_event_id: oldest.event_id,
                        };
                    }
                    let retained = events
                        .iter()
                        .filter(|event| event.event_id > after_event_id)
                        .take(limit)
                        .cloned()
                        .collect::<Vec<_>>();
                    if !retained.is_empty() {
                        return NextEvents::Events(retained);
                    }
                }
                notified.await;
            }
        })
        .await
        .ok()
    }

    pub fn history_status(&self) -> EventHistoryStatus {
        let Ok(events) = self.history.events.lock() else {
            return EventHistoryStatus {
                oldest_event_id: None,
                latest_event_id: None,
            };
        };
        EventHistoryStatus {
            oldest_event_id: events.front().map(|event| event.event_id),
            latest_event_id: events.back().map(|event| event.event_id),
        }
    }
}

pub fn channel() -> (EventSink, impl std::future::Future<Output = Result<()>>) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Arc<EventEnvelope>>();
    let sink = EventSink {
        sender,
        history: Arc::new(EventHistory {
            next_id: AtomicU64::new(1),
            events: StdMutex::new(VecDeque::with_capacity(EVENT_HISTORY_CAPACITY)),
            notify: Notify::new(),
        }),
    };
    let writer = async move {
        let mut stdout = BufWriter::new(tokio::io::stdout());
        while let Some(event) = receiver.recv().await {
            let mut line = serde_json::to_vec(event.as_ref())?;
            line.push(b'\n');
            stdout.write_all(&line).await?;
            stdout.flush().await?;
        }
        Ok(())
    };
    (sink, writer)
}

pub fn dispatch_command(
    command: AgentCommand,
    max_speak_chars: usize,
    sink: &EventSink,
    sender: &tokio::sync::broadcast::Sender<AgentCommand>,
) {
    if command == AgentCommand::Ping {
        sink.send(AgentEvent::Pong);
        return;
    }
    if let AgentCommand::Speak { text, .. } = &command
        && text.chars().count() > max_speak_chars
    {
        sink.send(AgentEvent::Error {
            code: "speak_too_long".to_owned(),
            message: format!("speak text exceeds {max_speak_chars} characters"),
            retryable: true,
        });
        return;
    }
    if sender.send(command).is_err() {
        sink.send(AgentEvent::Error {
            code: "no_active_session".to_owned(),
            message: "no telephone session is listening".to_owned(),
            retryable: true,
        });
    }
}

pub async fn read_commands(
    max_speak_chars: usize,
    sink: EventSink,
    sender: tokio::sync::broadcast::Sender<AgentCommand>,
) -> Result<()> {
    use tokio::io::AsyncBufReadExt;

    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<AgentCommand>(&line) {
            Ok(command) => dispatch_command(command, max_speak_chars, &sink, &sender),
            Err(error) => sink.send(AgentEvent::Error {
                code: "invalid_command".to_owned(),
                message: error.to_string(),
                retryable: true,
            }),
        }
    }
    Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn history_returns_buffered_and_future_events() {
        let (sink, _writer) = channel();
        sink.send(AgentEvent::Pong);
        let Some(NextEvent::Event(first)) = sink.next_after(0, Duration::from_millis(10)).await
        else {
            panic!("expected a retained event");
        };
        assert_eq!(first.event_id, 1);
        assert_eq!(first.event, AgentEvent::Pong);

        let waiting_sink = sink.clone();
        let waiter = tokio::spawn(async move {
            let Some(NextEvent::Event(event)) =
                waiting_sink.next_after(1, Duration::from_secs(1)).await
            else {
                panic!("expected a future event");
            };
            event
        });
        tokio::task::yield_now().await;
        sink.send(AgentEvent::Pong);
        assert_eq!(waiter.await.unwrap().event_id, 2);
    }

    #[tokio::test]
    async fn history_wait_times_out() {
        let (sink, _writer) = channel();
        assert!(sink.next_after(0, Duration::from_millis(1)).await.is_none());
    }

    #[tokio::test]
    async fn history_reports_a_cursor_gap() {
        let (sink, _writer) = channel();
        for _ in 0..=EVENT_HISTORY_CAPACITY {
            sink.send(AgentEvent::Pong);
        }
        let Some(NextEvent::HistoryGap { oldest_event_id }) =
            sink.next_after(0, Duration::from_millis(10)).await
        else {
            panic!("expected an event history gap");
        };
        assert_eq!(oldest_event_id, 2);
        let Some(NextEvents::HistoryGap { oldest_event_id }) =
            sink.events_after(0, 16, Duration::from_millis(10)).await
        else {
            panic!("expected a batched event history gap");
        };
        assert_eq!(oldest_event_id, 2);
    }

    #[tokio::test]
    async fn history_batches_retained_events_and_reports_bounds() {
        let (sink, _writer) = channel();
        sink.send(AgentEvent::Pong);
        sink.send(AgentEvent::Pong);
        sink.send(AgentEvent::Pong);

        let Some(NextEvents::Events(events)) =
            sink.events_after(0, 2, Duration::from_millis(10)).await
        else {
            panic!("expected retained events");
        };
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, 1);
        assert_eq!(events[1].event_id, 2);
        let status = sink.history_status();
        assert_eq!(status.oldest_event_id, Some(1));
        assert_eq!(status.latest_event_id, Some(3));
    }
}
