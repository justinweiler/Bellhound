use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
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

[acknowledgement]
enabled = true
delay_ms = 1000
mode = "speech"
text = "Got it. Give me a moment."
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

    prepare_opening(control_port, token, "Bellhound opening.");

    let (mut websocket, _) =
        connect(format!("ws://127.0.0.1:{port}/conversation-relay/{token}")).unwrap();
    let connected = read_event(&mut stdout);
    assert_eq!(connected["event"], "connected");

    websocket
        .send(Message::Text(
            r#"{"type":"setup","sessionId":"relay-session","callControlId":"call-control","callSessionId":"call-session","callStatus":"active"}"#
                .into(),
        ))
        .unwrap();
    assert_eq!(read_event(&mut stdout)["event"], "setup");
    let outbound = websocket.read().unwrap();
    let Message::Text(outbound) = outbound else {
        panic!("expected the prepared opening frame");
    };
    let outbound: Value = serde_json::from_str(outbound.as_str()).unwrap();
    assert_eq!(outbound["type"], "text");
    assert_eq!(outbound["token"], "Bellhound opening.");
    let opening_spoken = read_event(&mut stdout);
    assert_eq!(opening_spoken["event"], "spoken");
    assert_eq!(opening_spoken["responding_to_sequence"], Value::Null);

    let waiter = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "wait",
            "--after",
            "4",
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
    assert_eq!(waited["event_id"], 5);
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
    let spoken = read_event(&mut stdout);
    assert_eq!(spoken["event"], "spoken");
    assert_eq!(spoken["responding_to_sequence"], 1);
    assert_eq!(spoken["acknowledgement_preceded"], Value::Null);

    let spoken_event_id = spoken["event_id"].as_u64().unwrap().to_string();
    let no_acknowledgement = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "wait",
            "--after",
            &spoken_event_id,
            "--timeout-seconds",
            "2",
        ])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .output()
        .unwrap();
    assert!(no_acknowledgement.status.success());
    let no_acknowledgement: Value = serde_json::from_slice(&no_acknowledgement.stdout).unwrap();
    assert_eq!(no_acknowledgement["event"], "wait_timeout");

    websocket
        .send(Message::Text(
            r#"{"type":"prompt","voicePrompt":"Please check one more thing.","lang":"en","last":true}"#
                .into(),
        ))
        .unwrap();
    let second_utterance = read_event(&mut stdout);
    assert_eq!(second_utterance["event"], "utterance");
    assert_eq!(second_utterance["sequence"], 2);

    let outbound = websocket.read().unwrap();
    let Message::Text(outbound) = outbound else {
        panic!("expected an acknowledgement frame");
    };
    let outbound: Value = serde_json::from_str(outbound.as_str()).unwrap();
    assert_eq!(outbound["type"], "text");
    assert_eq!(outbound["token"], "Got it. Give me a moment.");
    let acknowledged = read_event(&mut stdout);
    assert_eq!(acknowledged["event"], "acknowledged");
    assert_eq!(acknowledged["responding_to_sequence"], 2);
    assert_eq!(acknowledged["mode"], "speech");
    assert_eq!(acknowledged["text"], "Got it. Give me a moment.");
    assert!(acknowledged["response_latency_ms"].as_u64().unwrap() >= 900);

    let spoke = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "speak",
            "The second check passed too.",
        ])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .output()
        .unwrap();
    assert!(spoke.status.success());
    let outbound = websocket.read().unwrap();
    let Message::Text(outbound) = outbound else {
        panic!("expected the agent reply frame");
    };
    let outbound: Value = serde_json::from_str(outbound.as_str()).unwrap();
    assert_eq!(outbound["token"], "The second check passed too.");
    let spoken = read_event(&mut stdout);
    assert_eq!(spoken["event"], "spoken");
    assert_eq!(spoken["responding_to_sequence"], 2);
    assert_eq!(spoken["acknowledgement_preceded"], true);
    assert!(spoken["response_latency_ms"].as_u64().unwrap() >= 900);

    websocket.close(None).unwrap();
    let ended = read_event(&mut stdout);
    assert_eq!(ended["event"], "ended");
    assert_eq!(ended["reason"], "remote_closed");

    assert_thinking_sound_is_served(port, token);
    let fetched = read_event(&mut stdout);
    assert_eq!(fetched["event"], "acknowledgement_audio_fetched");
    assert!(fetched["bytes"].as_u64().unwrap() > 10_000);

    child.kill().unwrap();
    child.wait().unwrap();
}

fn prepare_opening(port: u16, token: &str, text: &str) {
    let body = serde_json::json!({"text": text}).to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "POST /v1/opening HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nX-Bellhound-Token: {token}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 202"), "{response}");
}

fn assert_thinking_sound_is_served(port: u16, token: &str) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "GET /conversation-relay/{token}/thinking.mp3 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    stream.flush().unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8_lossy(&response[..header_end]);
    assert!(headers.starts_with("HTTP/1.1 200"), "{headers}");
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-type: audio/mpeg")
    );
    assert!(response[header_end + 4..].starts_with(b"ID3"));
}

fn read_event(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(!line.is_empty(), "Bellhound closed stdout unexpectedly");
    serde_json::from_str(&line).unwrap()
}
