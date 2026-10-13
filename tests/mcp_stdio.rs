use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn mcp_stdio_exposes_cursor_safe_idempotent_tools() {
    let relay_port = available_port();
    let control_port = available_port();
    let temp = tempfile::tempdir().unwrap();
    let config_path = write_config(&temp, relay_port, control_port);
    let token = "test-token-with-at-least-24-characters";

    let mut relay = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args(["--config", config_path.to_str().unwrap(), "relay"])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut relay_stdout = BufReader::new(relay.stdout.take().unwrap());
    let ready = read_line_json(&mut relay_stdout);
    assert_eq!(ready["event"], "ready");
    let ready_cursor = ready["event_id"].as_u64().unwrap();

    let mut mcp = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args(["--config", config_path.to_str().unwrap(), "mcp"])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .env("RUST_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut mcp_stdin = mcp.stdin.take().unwrap();
    let mut mcp_stdout = BufReader::new(mcp.stdout.take().unwrap());

    let initialized = request(
        &mut mcp_stdin,
        &mut mcp_stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "bellhound-test", "version": "1.0.0"}
            }
        }),
    );
    assert_eq!(initialized["result"]["serverInfo"]["name"], "bellhound");
    notify(
        &mut mcp_stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );

    let listed = request(
        &mut mcp_stdin,
        &mut mcp_stdout,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    let status_tool = tools
        .iter()
        .find(|tool| tool["name"] == "bellhound_status")
        .unwrap();
    assert_eq!(status_tool["annotations"]["readOnlyHint"], true);
    let dial_tool = tools
        .iter()
        .find(|tool| tool["name"] == "bellhound_dial")
        .unwrap();
    assert_eq!(dial_tool["annotations"]["idempotentHint"], true);
    let hangup_tool = tools
        .iter()
        .find(|tool| tool["name"] == "bellhound_hangup")
        .unwrap();
    assert_eq!(hangup_tool["annotations"]["destructiveHint"], true);

    let status = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        3,
        "bellhound_status",
        json!({}),
    );
    assert_eq!(status["control_reachable"], true);
    assert_eq!(status["active_call"], false);
    assert_eq!(status["latest_event_id"], ready_cursor);

    let first_wait = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        4,
        "bellhound_wait",
        json!({"after_event_id":0,"timeout_seconds":1,"max_events":10}),
    );
    assert_eq!(first_wait["next_cursor"], ready_cursor);
    assert_eq!(first_wait["events"][0]["event"], "ready");
    let rendered = serde_json::to_string(&first_wait).unwrap();
    assert!(!rendered.contains("call_control_id"));
    assert!(!rendered.contains("session_id"));

    let request_id = Uuid::new_v4().to_string();
    let first_speak = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        5,
        "bellhound_speak",
        json!({
            "request_id": request_id,
            "responding_to_sequence": 1,
            "text": "This has no active telephone session."
        }),
    );
    assert_eq!(first_speak["accepted"], true);
    assert_eq!(first_speak["duplicate"], false);

    let error_wait = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        6,
        "bellhound_wait",
        json!({
            "after_event_id": ready_cursor,
            "timeout_seconds": 1,
            "max_events": 10
        }),
    );
    assert_eq!(error_wait["events"][0]["event"], "error");
    assert_eq!(error_wait["events"][0]["code"], "no_active_session");
    let error_cursor = error_wait["next_cursor"].as_u64().unwrap();

    let replay = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        7,
        "bellhound_speak",
        json!({
            "request_id": request_id,
            "responding_to_sequence": 1,
            "text": "This has no active telephone session."
        }),
    );
    assert_eq!(replay["accepted"], true);
    assert_eq!(replay["duplicate"], true);

    let no_duplicate_event = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        8,
        "bellhound_wait",
        json!({
            "after_event_id": error_cursor,
            "timeout_seconds": 1,
            "max_events": 10
        }),
    );
    assert_eq!(no_duplicate_event["timed_out"], true);
    assert!(no_duplicate_event["events"].as_array().unwrap().is_empty());

    shutdown(&mut mcp, mcp_stdin);
    let mut mcp = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args(["--config", config_path.to_str().unwrap(), "mcp"])
        .env("BELLHOUND_RELAY_TOKEN", token)
        .env("RUST_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut mcp_stdin = mcp.stdin.take().unwrap();
    let mut mcp_stdout = BufReader::new(mcp.stdout.take().unwrap());
    request(
        &mut mcp_stdin,
        &mut mcp_stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "bellhound-restart-test", "version": "1.0.0"}
            }
        }),
    );
    notify(
        &mut mcp_stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );

    let relay_replay = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        21,
        "bellhound_speak",
        json!({
            "request_id": request_id,
            "responding_to_sequence": 1,
            "text": "This has no active telephone session."
        }),
    );
    assert_eq!(relay_replay["accepted"], true);
    assert_eq!(relay_replay["duplicate"], true);

    let no_restart_duplicate_event = call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        22,
        "bellhound_wait",
        json!({
            "after_event_id": error_cursor,
            "timeout_seconds": 1,
            "max_events": 10
        }),
    );
    assert_eq!(no_restart_duplicate_event["timed_out"], true);

    let conflict = raw_call_tool(
        &mut mcp_stdin,
        &mut mcp_stdout,
        23,
        "bellhound_speak",
        json!({
            "request_id": request_id,
            "responding_to_sequence": 1,
            "text": "Different arguments must be rejected."
        }),
    );
    assert_eq!(conflict["result"]["isError"], true);
    assert!(
        conflict["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("different arguments")
    );

    shutdown(&mut mcp, mcp_stdin);
    relay.kill().unwrap();
    relay.wait().unwrap();
}

#[test]
fn mcp_status_only_profile_discovers_tools_and_fails_closed() {
    let config_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/mcp-status-only.toml");
    let mut mcp = Command::new(env!("CARGO_BIN_EXE_bellhound"))
        .args(["--config", config_path.to_str().unwrap(), "mcp"])
        .env_remove("BELLHOUND_MCP_STATUS_ONLY_RELAY_TOKEN")
        .env_remove("BELLHOUND_MCP_STATUS_ONLY_TELNYX_KEY")
        .env("RUST_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = mcp.stdin.take().unwrap();
    let mut stdout = BufReader::new(mcp.stdout.take().unwrap());

    request(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "status-only-test", "version": "1.0.0"}
            }
        }),
    );
    notify(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let listed = request(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 5);

    let status = call_tool(&mut stdin, &mut stdout, 3, "bellhound_status", json!({}));
    assert_eq!(status["control_reachable"], false);
    assert_eq!(status["active_call"], Value::Null);

    let dial = raw_call_tool(
        &mut stdin,
        &mut stdout,
        4,
        "bellhound_dial",
        json!({
            "request_id": Uuid::new_v4().to_string(),
            "opening": "This discovery profile must never dial."
        }),
    );
    assert_eq!(dial["result"]["isError"], true);
    assert!(
        dial["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("control listener is unavailable")
    );

    shutdown(&mut mcp, stdin);
}

fn available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_config(temp: &TempDir, relay_port: u16, control_port: u16) -> std::path::PathBuf {
    let path = temp.path().join("bellhound.toml");
    std::fs::write(
        &path,
        format!(
            r#"
[credentials]
api_key_env = "TELNYX_API_KEY"
keyring_service = "Telnyx API key"
keyring_account = "test"

[relay]
bind = "127.0.0.1:{relay_port}"
control_bind = "127.0.0.1:{control_port}"
path = "/conversation-relay"
token_env = "BELLHOUND_RELAY_TOKEN"
max_call_seconds = 30
emit_partial_transcripts = false
continuation_window_ms = 1000

[protocol]
max_speak_chars = 2000

[acknowledgement]
enabled = false
delay_ms = 300
mode = "speech"
text = "Got it. Give me a moment."
"#
        ),
    )
    .unwrap();
    path
}

fn notify(stdin: &mut ChildStdin, value: Value) {
    writeln!(stdin, "{value}").unwrap();
    stdin.flush().unwrap();
}

fn request(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>, value: Value) -> Value {
    let expected_id = value["id"].clone();
    notify(stdin, value);
    for _ in 0..10 {
        let response = read_line_json(stdout);
        if response["id"] == expected_id {
            return response;
        }
    }
    panic!("MCP server did not return response {expected_id}");
}

fn raw_call_tool(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    request(
        stdin,
        stdout,
        json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
    )
}

fn call_tool(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    let response = raw_call_tool(stdin, stdout, id, name, arguments);
    assert_ne!(response["result"]["isError"], true, "{response}");
    response["result"]["structuredContent"].clone()
}

fn read_line_json(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(!line.is_empty(), "process closed stdout unexpectedly");
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("invalid JSON {error}: {line}"))
}

fn shutdown(child: &mut Child, stdin: ChildStdin) {
    drop(stdin);
    if child.wait().is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
