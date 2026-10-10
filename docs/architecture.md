# Bellhound architecture

Bellhound is a transport process, not an agent.

```text
originating agent
  | JSON Lines on a long-lived PTY
  v
Bellhound
  | bidirectional WebSocket text frames
  v
Telnyx Conversation Relay
  | STT, TTS, DTMF, interruption, and PSTN transport
  v
telephone
```

The originating agent launches Bellhound from its existing task and keeps the process session open. Final caller utterances appear as `utterance` events. The agent can inspect its existing task context, use its ordinary tools, and send a concise `speak` command. The call therefore remains inside one long agent turn without introducing a proxy model.

## Process boundaries

- **Bellhound owns:** Telnyx API requests, local WebSocket serving, frame conversion, call limits, ordering, and secret-safe credential access.
- **Telnyx owns:** the phone number, carrier transport, speech-to-text, text-to-speech, DTMF, and interruption detection.
- **The originating agent owns:** reasoning, task context, tools, decisions, and what text should be spoken.

## State machine

```text
ready -> dialing -> connected -> utterance/speak loop -> ended
                      |                    |
                      +---- interrupt -----+
```

Bellhound permits one active WebSocket call. Every final utterance receives a monotonically increasing sequence number. Calls stop at the configured maximum duration even when the model or caller forgets to hang up.

## Trust

Telephone speech reaches the agent as tool output. It can answer questions and guide ordinary work, but it is not a substitute for an authenticated Codex approval prompt. A verified outbound number shows where Bellhound dialed; it does not prove who answered.

The production design should add a short DTMF or spoken challenge before marking the caller as Justin-authenticated. Consequential actions still use the normal product approval channel.

## Failure behavior

- Invalid agent commands produce structured `error` events.
- Invalid Telnyx frames produce structured errors without terminating the process immediately.
- A second simultaneous WebSocket receives HTTP 409.
- A wrong URL token receives HTTP 401.
- Closing the WebSocket ends the relay; Telnyx does not reconnect automatically.
- Agent `hangup` sends Telnyx's graceful relay `end` frame, issues a Call Control hangup for the telephone leg, and then closes the socket.
- Phone numbers and raw setup payloads are omitted from stdout.
