use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde_json::Value;
use tungstenite::{Message, connect};

#[test]
fn relay_bridges_telnyx_text_and_agent_commands() {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let control_probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let control_port = control_probe.local_addr().unwrap().port();
    drop(control_probe);

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
control_bind = "127.0.0.1:{control_port}"
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
        .stdin(Stdio::null())
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

    let waiter = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "wait",
            "--after",
            "2",
            "--timeout-seconds",
            "5",
        ])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(50));

    websocket
        .send(Message::Text(
            r#"{"type":"prompt","voicePrompt":"Did the canary pass?","lang":"en","last":true}"#
                .into(),
        ))
        .unwrap();
    let waited = waiter.wait_with_output().unwrap();
    assert!(waited.status.success());
    let waited: Value = serde_json::from_slice(&waited.stdout).unwrap();
    assert_eq!(waited["event"], "utterance");
    assert_eq!(waited["event_id"], 3);
    assert_eq!(waited["text"], "Did the canary pass?");

    let utterance = read_event(&mut stdout);
    assert_eq!(utterance["event"], "utterance");
    assert_eq!(utterance["text"], "Did the canary pass?");

    let spoke = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "speak",
            "Yes. The relay is healthy.",
        ])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .output()
        .unwrap();
    assert!(spoke.status.success());

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
