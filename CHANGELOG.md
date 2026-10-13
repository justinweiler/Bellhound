# Changelog

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
