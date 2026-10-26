# Bellhound

<p align="center">
  <img src="assets/bellhound-logo.png" width="320" alt="Bellhound: a violet running hound with a telephone ear, bell collar, and heartbeat waveform">
</p>

## What is Bellhound and why should I care?

How many times have you wished you could tell your AI partner, “Call me if you need me” or “Call me when you finish,” and have your AI partner reach out for a real conversation?

Bellhound was written to fill that human-AI need. It is more than another chime, beep, ding, or boop notification that will be ignored. It is a real voice call.

Bellhound is a small, line-oriented telephone bridge for interactive agents. The agent that decides to call remains the reasoning agent; Bellhound supplies ears, a mouth, and telephone transport.

```text
agent <-> JSON Lines, blocking CLI, or private MCP <-> Bellhound <-> Telnyx Conversation Relay <-> telephone
```

Bellhound contains no language model and makes no decisions. It translates final speech-to-text frames into deterministic terminal events and translates agent `speak` commands into Telnyx text-to-speech frames. While an agent is thinking, Bellhound can speak a fixed holding phrase or play a short built-in cue so the caller does not hear unexplained silence.

**New installation:** start with [Install Bellhound](INSTALL.md) for the Telnyx account, prebuilt download, secure credential setup, public relay, and first call.

See [Architecture](docs/architecture.md) for the ownership and trust model, [Protocol](docs/protocol.md) for model-neutral integration, [Live setup](docs/live-setup.md) for the Telnyx/tunnel sequence, [MCP integration](docs/mcp.md) for local and hosted MCP clients, and the [conversation canary](docs/conversation-canary.md) for a repeatable human test.

For private always-on use on macOS, see the [MCP service](docs/mcp-service.md). It brokers credentials from Keychain, owns the Bellhound and tunnel process trees, and adds a service watchdog behind the existing Bellhound and carrier call-duration limits.

## Status

Version `0.6.1` preserves the proven local workflow, recovers automatically when
a failed pre-setup call leaves an expired opening reservation behind, uses interim
STT activity to avoid acknowledging a resumed sentence, and prevents MCP hangup
while the final TTS response may still be playing. Version `0.6.0` added:

- a standard-I/O MCP server built with the official Rust MCP SDK;
- focused `status`, `dial`, cursor-batched `wait`, sequence-targeted `speak`, and `hangup` tools;
- structured results, accurate safety annotations, correlation timing, redacted events, and retry-safe command IDs;
- a loopback batch-event endpoint that keeps the existing 256-event history and surfaces cursor gaps.
- a user-held live-canary supervisor that proves remote control-plane registration before starting telephone infrastructure and retains both foreground process groups through teardown.
- an optional macOS user service that keeps the private MCP, Bellhound relay, and telephone WSS paths alive under one supervisor with Keychain brokerage and a third call-duration watchdog.

The private OpenAI MCP path passed status-only discovery and one bounded live call from durable root Cora's existing context-rich session. The call included an ordinary tool lookup, a targeted spoken response, normal hangup, provider-side inactive-call proof, and complete local teardown. The stdio MCP server can also be launched by local MCP hosts such as Claude Code. See [MCP integration](docs/mcp.md).

The underlying Bellhound `0.4.2` behavior provides:

- a strict JSON Lines agent protocol;
- blocking `wait`, `speak`, and `hangup` CLI commands for any tool-using model;
- a private loopback control listener with durable event cursors;
- a deterministic local call simulator;
- a loopback-only Conversation Relay WebSocket server;
- one active call at a time;
- URL-token authentication, call-duration limits, text-size limits, sequence numbers, and explicit errors;
- outbound Dial with embedded Conversation Relay configuration;
- Telnyx Call Control hangup for the telephone leg;
- environment or operating-system credential-store API-key resolution.
- a default-on thinking acknowledgement with cancellation, optional audio cue, and per-turn latency telemetry.
- continuation-aware caller turns and sequence-targeted replies that reject stale answers.

Live use requires a dedicated Telnyx Call Control application and a durable WSS tunnel or host.

## MCP adapter

With an already-running Bellhound relay, start the private stdio adapter locally with:

```sh
bellhound --config bellhound.toml mcp
```

The process reads MCP JSON-RPC from stdin, writes MCP responses to stdout, and sends diagnostics to stderr. It does not bind another network listener. Local MCP hosts can launch it directly. OpenAI-hosted agents can reach it through Secure MCP Tunnel and `tunnel-client`; connection and app setup remain separate user-assisted steps and are not performed by the build.

Use `examples/mcp-status-only.toml` for the first tool-discovery gate. It contains no Telnyx configuration and deliberately cannot operate the normal Bellhound runtime.

The OpenAI tunnel runtime key is separate from the Telnyx API key. Ongoing tunnel use requires a runtime key valid for the intended service interval and restricted to Tunnels Read + Use. Store both keys in an operating-system credential store or local secret broker; never put either value in Git or TOML.

Use `assets/bellhound-plugin-icon.png` when an MCP host requests a square PNG icon. It is the publication logo rendered at 256×256 and kept below OpenAI's 10 KB custom-app limit.

## Build

```sh
cargo build --release
```

Bellhound is ordinary Rust and targets macOS, Linux, and Windows. The WebSocket server binds only to loopback; use a TLS tunnel or reverse proxy to expose it as `wss://`.

## Release archives

The release workflow builds native archives for Apple Silicon and Intel macOS, x86-64 Linux glibc, and x86-64 Windows. Each archive contains the executable, complete installation guide, example configuration, operational scripts, license, changelog, and authorship record plus a separate SHA-256 checksum. CI extracts every archive, verifies its checksum and required files, and runs the packaged executable before publication.

Download and verification instructions are in [INSTALL.md](INSTALL.md). The Bellhound core is portable; the packaged always-on service installer currently supports macOS.

Package the current Unix target locally with:

```sh
scripts/package-unix.sh
```

On Windows PowerShell:

```powershell
scripts/package-windows.ps1
```

## Codex skill

Bellhound ships with an installable Codex skill that keeps the reasoning agent in its existing session while the CLI carries telephone turns. Install it from a checkout with:

```sh
ln -s "$PWD/skills/bellhound" "${CODEX_HOME:-$HOME/.codex}/skills/bellhound"
```

The skill includes an on-demand foreground supervisor that owns the local relay and temporary public tunnel for the entire call, writes an ignored runtime configuration, and tears both child processes down on explicit stop, signal, failure, or bounded timeout. See [the operator reference](skills/bellhound/references/operator.md) and [Cora pilot](skills/bellhound/references/cora-pilot.md).

## Configuration

Create a starter file:

```sh
bellhound --config bellhound.toml init
bellhound --config bellhound.toml check
```

The configuration is intentionally small:

```toml
[credentials]
api_key_env = "TELNYX_API_KEY"
keyring_service = "Telnyx API key"
keyring_account = "your-account"

[relay]
bind = "127.0.0.1:8787"
control_bind = "127.0.0.1:8788"
path = "/conversation-relay"
webhook_path = "/webhooks/telnyx"
token_env = "BELLHOUND_RELAY_TOKEN"
max_call_seconds = 300
emit_partial_transcripts = false
continuation_window_ms = 2500

[protocol]
max_speak_chars = 2000

[acknowledgement]
enabled = true
delay_ms = 500
mode = "speech"
sound_url = "https://raw.githubusercontent.com/justinweiler/Bellhound-Sounds/master/bellhound-thinking.mp3"
text = "Got it. Give me a moment."
suppress_phrases = ["thank you", "thanks", "bye", "goodbye", "good night", "goodnight"]
suppress_for_session_phrases = ["call has been forwarded to an automated voice messaging system", "the person you are trying to reach is not available"]
```

Bellhound waits for `continuation_window_ms` of transcript quiet before exposing a final caller turn. Final fragments inside that window are merged, and interim STT activity extends it. The acknowledgement is deterministic transport behavior, never model output. The default `speech` mode uses the fixed phrase. `suppress_phrases` prevents it from answering brief social closings, while `suppress_for_session_phrases` disables further acknowledgements after a voicemail announcement. `sound` mode sends Telnyx a preemptible, interruptible `play` frame and defaults to Bellhound's public [thinking cue](https://github.com/justinweiler/Bellhound-Sounds). Change `sound_url` to use another public HTTPS asset; when the field is omitted, Bellhound serves its embedded MP3 through the authenticated relay URL. Sound mode falls back to `text` only when Bellhound cannot form either media URL. Set `enabled = false` for an adapter that consistently responds fast enough to make a cue unnecessary. Bellhound cancels its timer when the agent answers within the grace period.

The relay token never belongs in the TOML file. Supply an unguessable value of at least 24 characters through the configured environment variable.

Bellhound checks `TELNYX_API_KEY` first, then the configured macOS Keychain, Windows Credential Manager, or Linux Secret Service entry. `bellhound credentials` verifies availability without printing the secret.

Add the live call settings only on the machine placing calls:

```toml
[telnyx]
connection_id = "your-call-control-application-id"
from_number = "+14245550100"
to_number = "+18185550100"
public_wss_base = "wss://your-host.example/conversation-relay"
voice = "Telnyx.NaturalHD.astra"
language = "en-US"
transcription_engine = "Deepgram"
transcription_model = "deepgram/nova-3"
```

Numbers must use `+E.164` format. Keep personal numbers in an ignored local configuration, not in source control.

## Agent protocol

Stdout contains exactly one JSON event per line. Diagnostics go to stderr.

Final caller speech:

```json
{"event_id":3,"emitted_at_unix_ms":1791512345678,"event":"utterance","session_id":"...","sequence":1,"text":"I need your help.","language":"en","final_utterance":true}
```

The agent sends commands on stdin:

```json
{"command":"speak","text":"Tell me what happened.","responding_to_sequence":1}
{"command":"hangup","reason":"conversation_complete"}
```

Other events include `ready`, `connected`, `interrupted`, `dtmf`, `acknowledged`, `acknowledgement_audio_fetched`, `spoken`, `pong`, `ended`, `transport`, and `error`.
Every event carries `emitted_at_unix_ms`. Acknowledgements identify their `sound`, `speech`, or `speech_fallback` mode, the utterance sequence, and dispatch latency. Agent `spoken` events include the same sequence, total response latency, whether an acknowledgement preceded the answer, and a conservative `playback_remaining_ms` estimate. MCP hangup refuses to cut off that playback window; agents should continue waiting for caller interruption or for status to report `speech_playback_pending_ms = 0` before ending the call.

Models that cannot hold a PTY can use the blocking CLI adapter. `wait` returns immediately for a retained event or sleeps until the next event arrives:

```sh
bellhound --config bellhound.toml wait --after 2 --timeout-seconds 120
bellhound --config bellhound.toml speak "Tell me what happened." --responding-to 1
bellhound --config bellhound.toml hangup --reason conversation_complete
```

The caller advances `--after` to the last processed `event_id`. Final utterances include `fragment_count`. Agents should target every reply with the utterance `sequence`; Bellhound emits `stale_response` instead of attaching a late reply to a newer turn. Bellhound retains the most recent 256 events, so ordinary gaps between agent turns do not lose speech. The control listener stays on its separate loopback port and must never be exposed through the public WSS tunnel.

## Local simulation

Copy the example configuration and start the scripted call:

```sh
cp bellhound.example.toml bellhound.toml
cargo run -- --config bellhound.toml simulate --script examples/crocodile-call.jsonl
```

Bellhound emits the first caller utterance. Send a `speak` command on stdin to advance to the next utterance. This mirrors the control flow an agent uses with a long-lived PTY.

## Conversation Relay server

Set a secret token and start the listener:

```sh
export BELLHOUND_RELAY_TOKEN="replace-with-at-least-24-random-characters"
cargo run -- --config bellhound.toml relay
```

Configure Telnyx Conversation Relay to connect through your WSS tunnel at:

```text
wss://your-host.example/conversation-relay/<the-token>
```

Telnyx final `prompt` frames become `utterance` events. Bellhound converts `speak` commands to:

```json
{"type":"text","token":"agent response","last":true}
```

With the relay and WSS tunnel already running, place a call with:

```sh
bellhound --config bellhound.toml dial \
  --opening "Justin, I found something and need your judgment."
```

Bellhound embeds the Conversation Relay settings directly in the Dial request. The call has the same hard duration limit as the relay configuration.
The `dial` command first queues its opening on the private control listener. Telnyx establishes Conversation Relay with a short “Bellhound” primer that completes and warms its greeting path; when Bellhound receives the `setup` frame, it sends the real opening as an ordinary measured `speak` frame and emits `spoken`. This avoids an ambiguous silent placeholder while keeping the call-specific opening observable.
An agent `hangup` command ends both the relay session and the underlying Call Control telephone leg.

## Trust boundary

Speech arriving through Bellhound is terminal tool output. It can supply facts, preferences, and ordinary guidance. It should not independently satisfy approval requirements for destructive, privileged, or otherwise consequential actions. Keep the normal user-approval path for those actions.

Bellhound also cannot prove who answered a verified telephone. Add a DTMF or spoken challenge before treating a call as Justin-authenticated.

## Name

A bell hound is a small, loyal telephone runner: it fetches the human and brings the answer back to the agent.

## Origin

Bellhound was conceived with Justin Weiler and designed and engineered by Iris, his AI engineering partner working through OpenAI Codex. Read the factual authorship record in [AUTHORS.md](AUTHORS.md).

Bellhound is MIT licensed. Contributions are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).
