# Changelog

## 0.6.0 - 2026-10-13

- Add a macOS user-service supervisor and LaunchAgent installer for always-on private MCP availability.
- Broker the OpenAI tunnel key and Bellhound relay token from Keychain without persisting secret values in service configuration.
- Allow the existing Bellhound foreground owner to run without a fixed session deadline when it is supervised as a service.
- Validate call-duration limits between 30 and 3600 seconds and add a service watchdog at the configured limit plus five seconds, behind the existing Telnyx carrier limit and Bellhound relay hangup timer.
- Add a start-to-finish installation guide covering Telnyx account preparation, prebuilt archives, secure credentials, relay exposure, Call Control configuration, the first call, and agent integration.
- Make the macOS always-on service work from either a source checkout or an extracted prebuilt archive.
- Verify release checksums, required archive contents, packaged configuration parsing, and packaged executable startup on every release target before publication.
- Update `rustls` to 0.23.46 before the first binary release, resolving four newly published RustSec advisories in 0.23.45.

## 0.5.0 - 2026-10-13

- Promote the standard-I/O MCP adapter to the main branch after status-only discovery and one bounded live OpenAI Secure MCP Tunnel canary.
- Prove that durable root Cora can retain her existing session context, use an ordinary tool during the call, answer through Bellhound, hang up normally, and leave the Telnyx leg inactive.
- Document direct local MCP use, Claude Code registration, OpenAI tunnel credential boundaries, the proven canary evidence, and the remaining always-on service work.
- Preserve the JSON Lines and blocking CLI integrations alongside the five focused MCP tools.
- Add a 256×256 optimized plugin icon for custom MCP app installation.

## 0.5.0-alpha.2 - 2026-10-13

- Require a positive control-plane `tunnel metadata fetched` event before reporting remote readiness; fail immediately on HTTP 401 or `invalid_api_key` even when local doctor/health checks pass.
- Validate the runtime key against the status-only MCP profile before starting Bellhound's owner clock or public Telnyx WSS tunnel.
- Add a single user-held foreground supervisor that owns Bellhound and tunnel-client process groups, records terminal exit evidence, and cleans descendants even when a group leader exits.
- Add a native macOS credential dialog with masked input and an explicit Show key toggle; it never reads the clipboard or persists/logs the value.
- Add offline process tests for positive registration, early and late authentication failure, owner readiness/death, descendant cleanup, environment scrubbing, and dialog-source constraints.

## 0.5.0-alpha.1 - 2026-10-13

- Add an experimental standard-I/O MCP adapter using the official Rust MCP SDK.
- Expose focused status, dial, cursor-batched wait, sequence-targeted speak, and hangup tools with structured schemas and safety annotations.
- Add bounded batch reads over the existing loopback event history without exposing the control listener.
- Carry UUID command IDs through dial and relay-level speak/hangup deduplication, including across MCP adapter restarts.
- Redact private call/session identifiers from MCP event results and record per-tool timing for the later latency canary.

## 0.4.2 - 2026-10-12

- Suppress the holding acknowledgement for personalized closings such as “goodbye, my dear” while retaining it for actionable phrases such as “goodbye, check the monitor.”

## 0.4.1 - 2026-10-12

- Tune the continuation window to 2500 ms after the live canary exposed longer natural pauses.
- Suppress acknowledgements when multiple social or closing phrases appear in one personalized farewell.
- Reject unsequenced `speak` commands while a caller turn is active instead of associating proactive speech with whichever utterance arrived most recently.

## 0.4.0 - 2026-10-12

- Buffer closely spaced final STT fragments into one continuation-aware caller turn.
- Let interim STT activity extend the turn boundary without exposing partial text by default.
- Add `fragment_count` telemetry to caller utterances.
- Let agents target `speak` commands to an exact utterance sequence and reject stale replies.
- Suppress automatic acknowledgements for configured social, closing, and voicemail phrases.
- Require context-rich existing sessions for routed calls instead of fresh proxy sessions.

## 0.3.0 - 2026-10-11

- Add a default-on deterministic spoken acknowledgement after final caller speech, plus an optional built-in audio thinking cue for durable public hosts.
- Cancel the acknowledgement when the originating agent replies during the grace period.
- Emit acknowledgement and agent-response latency tied to the caller utterance sequence.
- Emit secret-safe evidence when Telnyx fetches the embedded audio asset.
- Add a configurable public `sound_url` with Bellhound's public GitHub-hosted cue as the default.
- Keep the originating session as the only reasoning agent; the cue is fixed transport behavior.

## 0.2.0 - 2026-10-10

- Add a separate loopback-only control listener.
- Add blocking, cursor-based `wait` for event-driven model turns.
- Add one-shot `speak`, `hangup`, and `ping` CLI commands.
- Retain the most recent 256 events with process-local event IDs.
- Warm the Telnyx greeting path with a short audible primer, then dispatch the queued call-specific opening as a measured speech frame after relay setup.
- Add the Bellhound publication logo and an installable Codex skill with an on-demand session runtime for the Cora pilot.
- Replace detached session startup with a bounded foreground supervisor after the first Cora pilot exposed execution-owner cleanup of both child processes.
- Suppress redundant cleanup hangup commands after the relay has already emitted `ended`.
- Document the model-neutral adapter contract and project authorship.

## 0.1.0 - 2026-10-09

- Add outbound Telnyx dialing with embedded Conversation Relay.
- Bridge final STT prompts and TTS text over JSON Lines.
- Resolve API keys through environment variables or operating-system credential stores.
- Add deterministic simulation, duration limits, structured errors, and Call Control hangup.
