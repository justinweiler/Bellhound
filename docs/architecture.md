# Bellhound architecture

Bellhound is a transport process, not an agent.

```text
originating agent
  | JSON Lines on a long-lived PTY
  | or blocking CLI commands over a private loopback port
  | or focused MCP tools through a private stdio adapter
  v
Bellhound
  | bidirectional WebSocket text frames
  v
Telnyx Conversation Relay
  | STT, TTS, DTMF, interruption, and PSTN transport
  v
telephone
```

The originating agent launches Bellhound from its existing task. It can keep the process session open or use one-shot `wait`, `speak`, and `hangup` commands against Bellhound's private control port. Final caller utterances appear as `utterance` events. The agent can inspect its existing task context, use its ordinary tools, and answer directly. The call therefore remains inside the originating agent's session without introducing a proxy model.

The MCP adapter keeps the same ownership rule. It translates five named MCP tools to the existing Bellhound control and dial paths. It has no network listener; a local MCP host can launch it directly, and a private remote client may reach its stdio transport through an independently configured secure tunnel. The adapter never carries raw audio.

## Process boundaries

- **Bellhound owns:** Telnyx API requests, local WebSocket serving, frame conversion, call limits, ordering, and secret-safe credential access.
- **Telnyx owns:** the phone number, carrier transport, speech-to-text, text-to-speech, DTMF, and interruption detection.
- **The originating agent owns:** reasoning, task context, tools, decisions, and what text should be spoken.
- **The MCP adapter owns:** schema validation, cursor batching, redaction, per-tool timing, and retry suppression for its named tools.

## State machine

```text
ready -> opening queued -> dialing -> setup -> opening spoken -> utterance -> acknowledgement/speak loop -> ended
                                            |                         |
                                            +------ interrupt --------+
```

Bellhound permits one active WebSocket call. It buffers final STT fragments until the configured continuation window passes without another final or interim prompt. The merged caller turn receives one monotonically increasing sequence number and reports how many final fragments it contains. Calls stop at the configured maximum duration even when the model or caller forgets to hang up.
Every event includes a process-local event ID and an emission timestamp in Unix milliseconds for latency tracing across STT arrival, agent response, and TTS dispatch. An enabled acknowledgement timer starts after the merged caller turn is emitted. An agent reply cancels a timer that has not fired, and configured social, closing, or voicemail phrases suppress it entirely. When it fires, Bellhound sends the configured fixed speech or a preemptible, interruptible Conversation Relay audio cue, emits `acknowledged`, and keeps the turn open until the agent replies. Sound mode prefers the configured public URL and can fall back to an authenticated embedded asset. Agents target replies to the exact utterance sequence; Bellhound rejects a response when the conversation has advanced or a newer turn is still buffered. The subsequent `spoken` event records the utterance sequence, total response latency, and whether a cue preceded it. The private control listener keeps 256 recent events so a model can resume from its last event ID without polling or losing a turn.

The MCP `wait` tool reads the same retained history in batches of at most 32 events. It blocks once for a bounded interval, returns immediately when events exist, and preserves explicit history-gap behavior. It does not create a second event store.

The real introduction is not delegated to Telnyx's opaque pre-setup greeting path. `dial` queues it locally, Conversation Relay connects with a short audible “Bellhound” primer to complete and warm that path, and Bellhound sends the call-specific opening immediately after `setup`. The resulting `spoken` event makes opening dispatch observable.

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
- MCP event projections omit local session IDs and Telnyx call-control identifiers.
- Consequential MCP tools require caller-supplied UUIDs. Telnyx receives the dial UUID as its command ID, and the relay deduplicates `speak` and `hangup` IDs for its lifetime.
