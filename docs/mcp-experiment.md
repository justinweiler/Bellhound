# Private MCP experiment

Bellhound `0.5.0-alpha.1` adds an experimental standard-I/O MCP adapter so a context-rich hosted agent can own the telephone turn loop while Bellhound continues to own telephony, STT/TTS transport, ordering, and cleanup.

This branch is a local experiment. It does not install a plugin, create an OpenAI Secure MCP Tunnel, expose a listener, configure credentials, or place a call.

## Architecture

```text
hosted originating agent
  | Bellhound MCP tools
  v
OpenAI-hosted private tunnel endpoint
  | outbound-only tunnel-client connection
  v
bellhound mcp (stdio; no listener)
  | authenticated HTTP on relay.control_bind
  v
Bellhound relay (loopback only)
  | existing Conversation Relay WSS path
  v
Telnyx <-> telephone
```

Secure MCP Tunnel is the intended private test transport because it can forward requests to a local standard-I/O MCP server without opening an inbound firewall port. The Bellhound control listener remains loopback-only and is never used as the MCP endpoint.

Official references: [OpenAI Secure MCP Tunnel](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels), [connect and test a custom MCP server](https://developers.openai.com/plugins/deploy/connect-chatgpt), and the [official MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk).

## Tools

The adapter exposes five focused tools:

| Tool | Effect |
|---|---|
| `bellhound_status` | Read relay reachability, active-call state, and retained-event bounds. |
| `bellhound_dial` | Call the single destination already fixed in private Bellhound configuration and return the starting event cursor. |
| `bellhound_wait` | Wait up to 30 seconds and return an ordered batch of at most 32 redacted events after a cursor. |
| `bellhound_speak` | Speak to one exact unanswered utterance sequence. |
| `bellhound_hangup` | End the active call with a short machine-readable reason. |

`dial`, `speak`, and `hangup` require a fresh UUID `request_id`. Dial forwards that UUID to Telnyx as its command ID. The relay keeps the latest 256 MCP command IDs so a restarted MCP adapter cannot repeat an accepted `speak` or `hangup` against the same relay process. Reusing an ID with different arguments is rejected.

Tool results include structured data and elapsed time. Event results deliberately omit Bellhound/Telnyx session and call-control identifiers. Caller and agent text remains visible because it is the conversation payload the originating model needs.

## Event loop

1. Call `bellhound_status`; require `control_reachable=true` and `active_call=false`.
2. Call `bellhound_dial` with a fresh UUID and concise opening. Save `event_cursor`.
3. Call `bellhound_wait` with the saved cursor. Advance to every returned `next_cursor`, including setup, acknowledgement, speech, error, and status events.
4. On `utterance`, reason and use ordinary tools in the originating session. Call `bellhound_speak` with that event's exact `sequence` and a fresh UUID.
5. If Bellhound reports `stale_response`, wait again and reassess the newest caller turn.
6. Call `bellhound_hangup` with a fresh UUID, wait for `ended`, then verify the foreground Bellhound supervisor exits and all runtime processes stop.

A wait timeout is not a failure. It returns the same cursor and lets the client issue another bounded wait. An `event_history_gap` is explicit and must never be treated as an empty conversation.

## Local verification

Run the branch with a separate Cargo target directory:

```sh
export CARGO_TARGET_DIR=/path/outside/the/primary/Bellhound/target
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --locked
```

The process integration test starts a loopback relay and the real MCP stdio server, negotiates MCP, inspects all five tools and annotations, calls status and wait, and proves duplicate `speak` suppression survives an MCP server restart. It uses no Telnyx credential and places no call.

## User-assisted private connection plan

These are gated future steps. Do not perform them merely because the branch exists.

### Gate 1: status-only discovery

This gate proves MCP transport and tool discovery without starting telephone infrastructure.

1. Build the experimental branch in its isolated worktree. Leave the Bellhound relay and foreground telephone supervisor stopped.
2. In OpenAI Platform tunnel settings, create or select a tunnel associated with the target Platform organization and ChatGPT workspace. This requires the applicable Tunnels permissions.
3. Obtain the official `tunnel-client` through the Platform download or current official release. Configure a local stdio profile whose MCP command runs the experimental `bellhound --config <worktree>/examples/mcp-status-only.toml mcp` process. This tracked profile has no Telnyx configuration, uses dedicated unused loopback ports, and names unset credentials. Supply only the tunnel runtime key through the local secret store or ephemeral environment; never put it in Git, TOML, chat, or command output.
4. Run `tunnel-client doctor` and require healthy/ready status before connecting ChatGPT.
5. In ChatGPT Plugins, add a custom MCP server, choose **Tunnel**, select the approved tunnel, configure the requested authentication, review the warning, and create it as a private test plugin.
6. Refresh discovered metadata and inspect the five tools. In the actual durable agent conversation, call only `bellhound_status`. The expected result is `control_reachable=false`, which proves the hosted session reached the local MCP process while confirming that no telephone runtime is active.

Gate 1 uses the outbound-only OpenAI Secure MCP Tunnel. It does **not** read the private Bellhound configuration, start Bellhound's relay, create a public Telnyx WSS tunnel, require the Bellhound relay token, or place a call. With the status-only profile, `dial`, `wait`, `speak`, and `hangup` fail closed before reaching Telnyx or a Bellhound control listener.

### Gate 2: explicitly approved live canary

Only after Gate 1 succeeds, request separate approval to:

1. Start the experimental foreground Bellhound supervisor with the existing private configuration. This also starts Bellhound's ordinary temporary public WSS tunnel so Telnyx Conversation Relay can reach the local relay.
2. Supply the Bellhound relay token to the local MCP process through the secret store or ephemeral environment and restart the MCP/tunnel-client profile if required.
3. Require `bellhound_status` to report a reachable control listener before dialing.
4. Place one bounded comparison call, then measure MCP control round-trip and end-of-utterance-to-first-audio latency separately from reasoning and other tool time.
5. Verify `ended`, supervisor exit, stopped Bellhound relay and public WSS tunnel, stopped or intentionally retained Secure MCP Tunnel client, and inactive Telnyx call state.

The OpenAI Secure MCP Tunnel and Bellhound's temporary public Telnyx WSS tunnel serve different paths and require separate approval scopes.

## Known limitations

- The experiment does not prove that a hosted agent turn can keep issuing bounded MCP waits for an entire telephone conversation. Product timeout, tool-loop, and cancellation behavior require status-only discovery and then a live canary.
- `bellhound_wait` clamps one request to 30 seconds because no reliable hosted-tool timeout is documented. The caller must retain and reuse the cursor across timeouts.
- Dial idempotency is backed by Telnyx command IDs and the MCP process cache. Speech and hangup idempotency survive MCP adapter restarts only while the same relay process remains alive.
- The MCP process does not start or own the foreground relay/public-WSS-tunnel supervisor. Status-only discovery deliberately leaves that runtime stopped; an explicitly approved live canary starts it separately. Autonomous durable hosting is later work.
- Secure MCP Tunnel is suitable for private testing, not public plugin submission. A public Bellhound plugin would require a stable public HTTPS MCP endpoint and an end-user authorization design.
- The private stdio experiment trusts the approved tunnel/workspace association and local process boundary. It does not add per-user OAuth inside Bellhound.
- No latency improvement is claimed until the durable-session comparison is measured.
