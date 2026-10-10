use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentCommand {
    Speak {
        text: String,
        #[serde(default = "default_true")]
        last: bool,
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

fn default_true() -> bool {
    true
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
}
