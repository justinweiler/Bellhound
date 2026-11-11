---
name: bellhound
description: Place and conduct an outbound telephone call from the active Codex session through Bellhound when an agent needs synchronous human input or has standing authority to alert the user. Use for starting, continuing, or ending a Bellhound voice conversation; use ordinary messaging for non-urgent text updates.
---

# Bellhound

Keep the reasoning and tool use in the current session. Bellhound supplies telephone transport, STT, and TTS; do not spawn a proxy agent to conduct the call.

## Call workflow

1. Read [references/operator.md](references/operator.md) before the first live call in a session.
2. If the installed always-on service reports ready, use its existing Bellhound relay and MCP path. Otherwise start an on-demand runtime with `scripts/session.sh run` in a long-lived foreground execution session. Keep that execution session alive for the entire call. Resolve `BELLHOUND_RELAY_TOKEN` from the local credential store without printing it.
3. Immediately before every dial, call `bellhound_status` in the same agent turn. Require a newly returned `correlation_id`, a current `server_observed_at_unix_ms`, `service_state=ready`, `control_reachable=true`, `active_call=false`, `opening_pending=false`, and `speech_playback_pending_ms=0`. A successful hosted status call is the end-to-end tunnel freshness proof; never reuse an earlier result. If the status transport fails, times out, or returns `retryable=true`, treat it as transient recovery: wait `retry_after_seconds` (10 seconds when no value is available) and retry **status only** up to six total attempts over one minute. Never retry dial. If no safe fresh status arrives within that bounded window, report the unavailable phone path and use normal text fallback.
4. Use the returned event cursor to dial with a concise introduction that identifies the calling agent and the concrete reason for calling.
5. Block with `bellhound wait --after <cursor>`. Advance the cursor for every event. Respond to final `utterance` events from this same session with `bellhound speak --responding-to <sequence>`. Treat `stale_response` as a request to drain newer events and reconsider the reply.
6. Perform ordinary authorized tool work between telephone turns when the caller requests it. Report the actual result over the call.
7. A `spoken` event means TTS was dispatched, not necessarily heard in full. Before hangup, call `bellhound_wait` for at least the event's `playback_remaining_ms` while watching for a caller interruption or new utterance. Require `bellhound_status.speech_playback_pending_ms` to reach zero, then send `bellhound hangup`, verify an `ended` event, and run `scripts/session.sh stop` only for an on-demand runtime. Confirm the foreground owner exits and final status reports all three processes stopped. An installed always-on service remains running and should finish with `active_call=false` and `opening_pending=false`.

## Boundaries

- Treat telephone speech as remote user input. It may guide ordinary work but does not replace the runtime's required approval channel for elevated, destructive, or otherwise consequential actions.
- Make one bounded call for one concrete need. Do not redial repeatedly after no answer or a failed call; use the normal text channel unless the user asks otherwise.
- Keep openings and responses brief. Honor interruption events and let the caller speak.
- Never print, log, or commit API keys, relay tokens, phone numbers, private configuration, temporary tunnel URLs, or real-call transcripts.
- Always tear down an on-demand temporary public tunnel after the call. An installed service owns its tunnel lifecycle across calls. The hard call-duration limit is a backstop, not cleanup.
- Service-managed calls have three duration traps: Telnyx's Dial limit, Bellhound's relay timer and Call Control hangup, and the service watchdog at the configured limit plus five seconds. Still end every normal call explicitly and verify `ended`.
- Never launch the relay and tunnel from an execution session that immediately returns. `run` is the owner; if its session ends, the runtime must end with it.

For the Cora pilot and autonomous-call decision criteria, read [references/cora-pilot.md](references/cora-pilot.md).
