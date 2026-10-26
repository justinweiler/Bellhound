use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentCommand {
    Speak {
        text: String,
        #[serde(default = "default_true")]
        last: bool,
        #[serde(default)]
        responding_to_sequence: Option<u64>,
    },
    Hangup {
        reason: Option<String>,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    Ready {
        version: &'static str,
        bind: String,
        websocket_path: String,
    },
    Connected {
        session_id: String,
    },
    Utterance {
        session_id: String,
        sequence: u64,
        text: String,
        language: Option<String>,
        final_utterance: bool,
        fragment_count: usize,
    },
    Interrupted {
        session_id: String,
    },
    Dtmf {
        session_id: String,
        digit: String,
    },
    Spoken {
        session_id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        responding_to_sequence: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_latency_ms: Option<u64>,
        playback_remaining_ms: u64,
        #[serde(default, skip_serializing_if = "is_false")]
        acknowledgement_preceded: bool,
    },
    Acknowledged {
        session_id: String,
        responding_to_sequence: u64,
        mode: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        response_latency_ms: u64,
    },
    AcknowledgementAudioFetched {
        session_id: Option<String>,
        bytes: usize,
    },
    Pong,
    Ended {
        session_id: String,
        reason: String,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    Transport {
        session_id: String,
        kind: String,
    },
    CallStatus {
        event_type: String,
        call_control_id: Option<String>,
        call_session_id: Option<String>,
        hangup_cause: Option<String>,
    },
    Setup {
        session_id: String,
        telnyx_session_id: Option<String>,
        call_control_id: Option<String>,
        call_session_id: Option<String>,
        direction: Option<String>,
        call_status: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct TelnyxFrame {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "voicePrompt")]
    pub voice_prompt: Option<String>,
    pub lang: Option<String>,
    pub last: Option<bool>,
    pub digit: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(rename = "callControlId")]
    pub call_control_id: Option<String>,
    #[serde(rename = "callSessionId")]
    pub call_session_id: Option<String>,
    pub direction: Option<String>,
    #[serde(rename = "callStatus")]
    pub call_status: Option<String>,
}

pub fn telnyx_text_frame(text: String, last: bool) -> Value {
    serde_json::json!({
        "type": "text",
        "token": text,
        "last": last,
    })
}

pub fn telnyx_end_frame(reason: String) -> Value {
    serde_json::json!({
        "type": "end",
        "handoffData": serde_json::json!({ "reason": reason }).to_string(),
    })
}

pub fn telnyx_play_frame(source: String) -> Value {
    serde_json::json!({
        "type": "play",
        "source": source,
        "loop": 1,
        "interruptible": true,
        "preemptible": true,
    })
}

fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_speak_command_with_default_last() {
        let command: AgentCommand =
            serde_json::from_str(r#"{"command":"speak","text":"Hello"}"#).unwrap();
        assert_eq!(
            command,
            AgentCommand::Speak {
                text: "Hello".to_owned(),
                last: true,
                responding_to_sequence: None,
            }
        );
    }

    #[test]
    fn parses_sequence_targeted_speak_command() {
        let command: AgentCommand =
            serde_json::from_str(r#"{"command":"speak","text":"Done","responding_to_sequence":7}"#)
                .unwrap();
        assert_eq!(
            command,
            AgentCommand::Speak {
                text: "Done".to_owned(),
                last: true,
                responding_to_sequence: Some(7),
            }
        );
    }

    #[test]
    fn parses_telnyx_final_prompt() {
        let frame: TelnyxFrame = serde_json::from_str(
            r#"{"type":"prompt","voicePrompt":"I defeated the crocodile","lang":"en","last":true}"#,
        )
        .unwrap();
        assert_eq!(frame.kind, "prompt");
        assert_eq!(
            frame.voice_prompt.as_deref(),
            Some("I defeated the crocodile")
        );
        assert_eq!(frame.last, Some(true));
    }

    #[test]
    fn renders_telnyx_tts_frame() {
        assert_eq!(
            telnyx_text_frame("Well done.".to_owned(), true),
            serde_json::json!({"type":"text","token":"Well done.","last":true})
        );
    }

    #[test]
    fn renders_telnyx_end_frame() {
        assert_eq!(
            telnyx_end_frame("conversation_complete".to_owned()),
            serde_json::json!({
                "type":"end",
                "handoffData":"{\"reason\":\"conversation_complete\"}"
            })
        );
    }

    #[test]
    fn renders_interruptible_preemptible_audio_frame() {
        let frame = telnyx_play_frame("https://example.test/thinking.mp3".to_owned());
        assert_eq!(frame["type"], "play");
        assert_eq!(frame["loop"], 1);
        assert_eq!(frame["interruptible"], true);
        assert_eq!(frame["preemptible"], true);
    }
}
