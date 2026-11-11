# MCP integration

Bellhound `0.6.0` includes a standard-input/output MCP server so the reasoning
agent that decides to call can own the telephone turn loop. Bellhound continues
to own telephony, speech transport, ordering, and cleanup; it does not insert a
second conversational model.

Building Bellhound does not install an MCP app, create a tunnel, configure
credentials, start a public listener, or place a call.

## Transport choices

The MCP tool contract is model-provider neutral. The transport chosen by the
MCP host determines how it reaches the same `bellhound mcp` process.

| Host shape | Connection | Current support |
|---|---|---|
| Local MCP host, including Claude Code | Launch Bellhound over stdio | Supported |
| OpenAI-hosted agent | OpenAI Secure MCP Tunnel to local stdio | Proven in a private live canary |
| Claude Desktop | Local binary MCP server; a `.dxt` package can wrap it | Server supported; `.dxt` packaging remains future work |
| Web or mobile host | Public Streamable HTTP MCP service with end-user authentication | Future deployment work |

The OpenAI Secure MCP Tunnel is an OpenAI access path, not a vendor-neutral
public MCP URL. Claude and other local MCP hosts can launch Bellhound directly.
A shared cross-vendor hosted service needs a stable Streamable HTTP endpoint and
an authorization design.

Official references:

- [OpenAI Secure MCP Tunnel](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels)
- [Connect and test an OpenAI custom MCP server](https://developers.openai.com/plugins/deploy/connect-chatgpt)
- [Claude local MCP servers](https://support.anthropic.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop)
- [Claude remote MCP servers](https://support.anthropic.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers)
- [Official MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)

## Architecture

```text
originating MCP agent
  | local stdio or an approved private transport
  v
bellhound mcp (stdio; no network listener)
  | authenticated HTTP on relay.control_bind
  v
Bellhound relay (loopback only)
  | Conversation Relay WSS path
  v
Telnyx <-> telephone
```

For the tested OpenAI-hosted path, `tunnel-client` makes an outbound HTTPS
connection to the OpenAI-hosted tunnel endpoint and forwards MCP requests to
Bellhound's stdio process. The Bellhound control listener remains loopback-only
and is never the MCP endpoint.

## Tools

The adapter exposes five focused tools:

| Tool | Effect |
|---|---|
| `bellhound_status` | Read relay reachability, active-call state, retained-event bounds, and estimated pending TTS playback. |
| `bellhound_dial` | Call the single destination fixed in private Bellhound configuration and return the starting event cursor. |
| `bellhound_wait` | Wait up to 30 seconds and return an ordered batch of at most 32 redacted events after a cursor. |
| `bellhound_speak` | Speak to one exact unanswered utterance sequence. |
| `bellhound_hangup` | End the active call after estimated TTS playback reaches zero. |

`dial`, `speak`, and `hangup` require a fresh UUID `request_id`. Dial forwards
that UUID to Telnyx as its command ID. The relay keeps the latest 256 MCP
command IDs so a restarted MCP adapter cannot repeat an accepted `speak` or
`hangup` against the same relay process. Reusing an ID with different arguments
is rejected.

Tool results include structured data and elapsed time. Event results omit
Bellhound and Telnyx session and call-control identifiers. Caller and agent text
remain visible because they are the conversation payload the originating model
needs.

## Event loop

1. Immediately before dial in the same agent turn, call `bellhound_status`. Require a new `correlation_id`, current `server_observed_at_unix_ms`, `control_reachable=true`, `active_call=false`, `opening_pending=false`, and `speech_playback_pending_ms=0`. The successful hosted call is the tunnel freshness proof; never reuse an earlier status result.
2. Call `bellhound_dial` with a fresh UUID and concise opening. Save `event_cursor`.
3. Call `bellhound_wait` with the saved cursor. Advance to every returned `next_cursor`.
4. On `utterance`, reason and use ordinary tools in the originating session. Call `bellhound_speak` with that event's exact `sequence` and a fresh UUID.
5. If Bellhound reports `stale_response`, wait again and reassess the newest caller turn.
6. After a final `spoken`, wait for at least `playback_remaining_ms` while watching for caller events. Require `bellhound_status.speech_playback_pending_ms=0`, then call `bellhound_hangup` with a fresh UUID, wait for `ended`, and verify that the telephone leg is inactive. Leave an installed always-on service running.

A wait timeout is not a failure. It returns the same cursor and lets the client
issue another bounded wait. An `event_history_gap` is explicit and must never be
treated as an empty conversation.

## Local MCP hosts

Build Bellhound and validate the private configuration before registering it:

```sh
cargo build --release --locked
bellhound --config /absolute/path/to/bellhound.toml check
```

An MCP host should launch this exact command over stdio:

```sh
/absolute/path/to/bellhound --config /absolute/path/to/bellhound.toml mcp
```

For Claude Code, the equivalent registration shape is:

```sh
claude mcp add bellhound -- \
  /absolute/path/to/bellhound \
  --config /absolute/path/to/bellhound.toml \
  mcp
```

The private configuration fixes the destination number and contains no API key.
Store the Telnyx key in the operating-system credential store as described in
[Live setup](live-setup.md). Keep the Bellhound relay and its public Telnyx WSS
path under a foreground owner or a separately designed service supervisor.

## OpenAI Secure MCP Tunnel

The tunnel requires a `tunnel_id`, a local MCP command, and a runtime API key.
The runtime key needs only **Tunnels Read + Use**. Creating or editing the tunnel
is a separate administrative operation requiring **Tunnels Read + Manage**;
do not grant Manage to the runtime key.

The tunnel runtime key and the Telnyx API key are different credentials:

| Credential | Used by | Minimum purpose |
|---|---|---|
| Telnyx API key | Bellhound | Dial and control the configured telephone leg |
| OpenAI tunnel runtime key | `tunnel-client` | Read and use the selected private MCP tunnel |
| Relay token | Bellhound processes | Authenticate the public relay path and private loopback control path |

The one-day tunnel key used for the October 13 canary was intentionally
temporary. Ongoing use needs a runtime key whose expiration covers the intended
service interval. OpenAI's tunnel documentation requires a runtime API key but
does not prescribe its expiration. Apply a practical rotation policy, keep the
scope at Read + Use, and store it in macOS Keychain, Windows Credential Manager,
Linux Secret Service, or another local secret broker. Never put any of these
values in Git, TOML, chat, command arguments, screenshots, or logs.

Before enabling calls:

1. Associate the tunnel with the intended Platform organization and ChatGPT workspace.
2. Configure the official `tunnel-client` profile to launch `bellhound ... mcp` over stdio.
3. Run `tunnel-client doctor` as a static check.
4. Start the client and require the authenticated `tunnel metadata fetched` control-plane event. Local `/healthz` and `/readyz` alone do not prove authentication.
5. Treat any HTTP 401 or `invalid_api_key` as fatal.
6. Discover the five tools and call `bellhound_status` before starting telephony.

`examples/mcp-status-only.toml` is a fail-closed discovery profile. It contains
no Telnyx section, uses dedicated unused loopback ports, and names credentials
that should remain unset. Its write tools cannot reach a live Bellhound relay.

## Bounded canary supervisor

`scripts/mcp-canary-supervisor.py` is a test harness for one controlled live
canary. It is not a persistent daemon. On macOS it uses
`scripts/mcp-key-dialog.swift` to collect the tunnel key in a masked local
dialog, validates remote authentication before starting telephone
infrastructure, waits on a nonce-bound operator release, owns both child process
groups, records terminal evidence, and cleans descendants on success or failure.

The October 13, 2026 validation proved:

- status-only tool discovery through the private OpenAI app;
- one dial owned entirely by durable root Cora from her existing context-rich session;
- an ordinary time lookup during the live telephone conversation;
- a 502 ms deterministic acknowledgement and an 8.939 second utterance-to-response dispatch measurement;
- a normal `ended` event with no redial;
- Telnyx `RetrieveCallStatus` returning HTTP 200 and `data.is_alive=false`;
- zero Bellhound, `tunnel-client`, Cloudflare, or supervisor processes after teardown, with all local control ports unreachable.

The canary exposed one cosmetic issue: a farewell produced an unnecessary
automatic acknowledgement. It did not affect call ownership, tool use, or
cleanup.

## Verification

```sh
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --locked
PYTHONDONTWRITEBYTECODE=1 PYTHONWARNINGS='error::ResourceWarning' \
  python3 -m unittest tests/test_mcp_canary_supervisor.py -v
xcrun swiftc -typecheck scripts/mcp-key-dialog.swift
```

The process integration test starts a loopback relay and the real MCP stdio
server, negotiates MCP, inspects all five tools and annotations, calls status and
wait, and proves duplicate `speak` suppression survives an MCP server restart.
It uses no Telnyx credential and places no call.

## Always-on operation

The MCP tool contract and private hosted path are proven. Bellhound also ships a
macOS user-service supervisor that keeps `tunnel-client`, the Bellhound relay,
and the temporary telephone WSS path under one lifecycle. See
[Always-on MCP service](mcp-service.md). The bounded canary supervisor remains a
separate proof harness and deliberately stops after one short session.

A long-lived runtime key is necessary for the service, but the supervisor and
LaunchAgent provide the actual process ownership and restart behavior.

Other limits:

- `bellhound_wait` clamps one request to 30 seconds; callers retain and reuse the cursor across timeouts.
- Speech and hangup deduplication survive MCP adapter restarts only while the same relay process remains alive.
- Secure MCP Tunnel is for private OpenAI connectivity, not public plugin distribution.
- A public or shared cross-vendor service needs Streamable HTTP, stable hosting, per-user authorization, rate limits, and credential isolation.
- The stdio adapter trusts the local host boundary and does not add end-user OAuth inside Bellhound.
