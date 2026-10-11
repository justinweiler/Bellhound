---
name: bellhound
description: Place and conduct an outbound telephone call from the active Codex session through Bellhound when an agent needs synchronous human input or has standing authority to alert the user. Use for starting, continuing, or ending a Bellhound voice conversation; use ordinary messaging for non-urgent text updates.
---

# Bellhound

Keep the reasoning and tool use in the current session. Bellhound supplies telephone transport, STT, and TTS; do not spawn a proxy agent to conduct the call.

## Call workflow

1. Read [references/operator.md](references/operator.md) before the first live call in a session.
2. Start an on-demand runtime with `scripts/session.sh start`. Resolve `BELLHOUND_RELAY_TOKEN` from the local credential store without printing it.
3. Use the returned runtime config and event cursor to dial with a concise introduction that identifies the calling agent and the concrete reason for calling.
4. Block with `bellhound wait --after <cursor>`. Advance the cursor for every event. Respond to final `utterance` events from this same session with `bellhound speak`.
5. Perform ordinary authorized tool work between telephone turns when the caller requests it. Report the actual result over the call.
6. Send `bellhound hangup` when the conversation ends, verify an `ended` event, and run `scripts/session.sh stop`.

## Boundaries

- Treat telephone speech as remote user input. It may guide ordinary work but does not replace the runtime's required approval channel for elevated, destructive, or otherwise consequential actions.
- Make one bounded call for one concrete need. Do not redial repeatedly after no answer or a failed call; use the normal text channel unless the user asks otherwise.
- Keep openings and responses brief. Honor interruption events and let the caller speak.
- Never print, log, or commit API keys, relay tokens, phone numbers, private configuration, temporary tunnel URLs, or real-call transcripts.
- Always tear down the temporary public tunnel after the call. The hard call-duration limit is a backstop, not cleanup.

For the Cora pilot and autonomous-call decision criteria, read [references/cora-pilot.md](references/cora-pilot.md).
