use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Command, Stdio},
};

use serde_json::Value;
use tungstenite::{Message, connect};

#[test]
fn relay_bridges_telnyx_text_and_agent_commands() {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("bellhound.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
[credentials]
api_key_env = "TELNYX_API_KEY"
keyring_service = "Telnyx API key"
keyring_account = "test"

[relay]
bind = "127.0.0.1:{port}"
path = "/conversation-relay"
token_env = "BELLHOUND_RELAY_TOKEN"
max_call_seconds = 30
emit_partial_transcripts = false

[protocol]
max_speak_chars = 2000
"#
        ),
    )
    .unwrap();

    let token = "test-token-with-at-least-24-characters";
    let mut child = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args(["--config", config_path.to_str().unwrap(), "relay"])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    assert_eq!(read_event(&mut stdout)["event"], "ready");

    let (mut websocket, _) =
        connect(format!("ws://127.0.0.1:{port}/conversation-relay/{token}")).unwrap();
    let connected = read_event(&mut stdout);
    assert_eq!(connected["event"], "connected");

    websocket
        .send(Message::Text(
            r#"{"type":"prompt","voicePrompt":"Did the canary pass?","lang":"en","last":true}"#
                .into(),
        ))
        .unwrap();
    let utterance = read_event(&mut stdout);
    assert_eq!(utterance["event"], "utterance");
    assert_eq!(utterance["text"], "Did the canary pass?");

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(
        stdin,
        r#"{{"command":"speak","text":"Yes. The relay is healthy."}}"#
    )
    .unwrap();
    stdin.flush().unwrap();

    let outbound = websocket.read().unwrap();
    let Message::Text(outbound) = outbound else {
        panic!("expected a text frame");
    };
    let outbound: Value = serde_json::from_str(outbound.as_str()).unwrap();
    assert_eq!(outbound["type"], "text");
    assert_eq!(outbound["token"], "Yes. The relay is healthy.");
    assert_eq!(read_event(&mut stdout)["event"], "spoken");

    websocket.close(None).unwrap();
    let ended = read_event(&mut stdout);
    assert_eq!(ended["event"], "ended");
    assert_eq!(ended["reason"], "remote_closed");

    child.kill().unwrap();
    child.wait().unwrap();
}

fn read_event(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(!line.is_empty(), "Bellhound closed stdout unexpectedly");
    serde_json::from_str(&line).unwrap()
}
