# Model-neutral protocol

Bellhound does not call a language-model API. The reasoning model stays in its existing session and uses a long-lived JSON Lines process, one-shot CLI commands, or the focused MCP adapter. This keeps Bellhound compatible with hosted models, local models, coding agents, shell-capable assistants, and future runtimes.

## Event envelope

Every event is one JSON object with these common fields:

```json
{
  "event_id": 3,
  "emitted_at_unix_ms": 1791512345678,
  "event": "utterance"
}
```

`event_id` increases for the lifetime of one relay process. `emitted_at_unix_ms` records when Bellhound emitted the event. Event-specific fields share the same object.

The current event names are `ready`, `connected`, `setup`, `utterance`, `interrupted`, `dtmf`, `acknowledged`, `acknowledgement_audio_fetched`, `spoken`, `pong`, `ended`, `transport`, `call_status`, and `error`.

Bellhound requests interim STT results and merges closely spaced final transcript fragments after a configurable quiet window. Interim frames are retained as caller activity even when partial transcripts are not emitted: they extend or reopen the continuation buffer, suppress the holding acknowledgement, and make a reply to the earlier fragment stale. The resulting final `utterance` contains one `sequence` and a `fragment_count`.

With acknowledgements enabled, Bellhound starts a configurable grace timer after the merged final utterance. A matching `speak` command received during that grace period cancels it. Configured short social, closing, and voicemail phrases suppress the acknowledgement. In the default speech mode Bellhound sends the fixed phrase and emits:

```json
{"event":"acknowledged","responding_to_sequence":4,"mode":"speech","text":"Got it. Give me a moment.","response_latency_ms":501}
```

The eventual agent reply remains a normal `spoken` event with additive timing fields:

```json
{"event":"spoken","text":"I checked it. The deployment is healthy.","responding_to_sequence":4,"response_latency_ms":16220,"playback_remaining_ms":9550,"acknowledgement_preceded":true}
```

`spoken` confirms that Bellhound dispatched the text frame; Conversation Relay
does not send a TTS-complete WebSocket frame. `playback_remaining_ms` is a
conservative word-rate estimate for the queued speech. MCP clients must keep
waiting for caller activity during that interval and require status
`speech_playback_pending_ms = 0` before hangup. A caller prompt or interruption
clears the estimate immediately.

Sound mode uses a configured public `sound_url` or Bellhound's authenticated embedded-audio URL. Conversation Relay receives a `play` frame that is both interruptible by the caller and preemptible by the agent response. A fetch of the embedded asset emits `acknowledgement_audio_fetched`, allowing operators to distinguish dispatch from media retrieval without logging the secret URL. `speech` mode and the `speech_fallback` path include the configured `text` field in the event. The acknowledgement is deterministic transport behavior. It does not reason, answer the caller, or replace the originating agent.

## Commands

The JSON Lines command forms are:

```json
{"command":"speak","text":"I need your judgment.","last":true,"responding_to_sequence":4}
{"command":"hangup","reason":"conversation_complete"}
{"command":"ping"}
```

Unknown fields and unknown commands are rejected. Text size is bounded by `protocol.max_speak_chars`. An active caller turn requires `responding_to_sequence`; Bellhound emits `missing_response_sequence` when it is omitted and `stale_response` when the target is no longer active or a newer caller turn is still being buffered. Untargeted speech remains available only when no caller turn is awaiting a response.

## Blocking CLI adapter

The relay retains its latest 256 events. A client supplies the last event it processed:

```sh
bellhound --config bellhound.toml wait --after 12 --timeout-seconds 120
```

If event 13 already exists, it returns immediately. Otherwise the command blocks until a new event arrives. A timeout returns:

```json
{"event":"wait_timeout","after":12}
```

This is a blocking wait, not interval polling. It lets a tool call remain pending until human speech wakes the originating model.

If a cursor falls more than 256 events behind, Bellhound returns HTTP 409 with `event_history_gap` and the oldest retained event ID. Adapters must surface that gap instead of silently skipping conversation turns.

Commands use the same local control path:

```sh
bellhound --config bellhound.toml speak "Done." --responding-to 4
bellhound --config bellhound.toml hangup --reason conversation_complete
bellhound --config bellhound.toml ping
```

The control listener uses `relay.control_bind`, accepts only the configured token, and must remain on loopback. It is deliberately separate from the listener exposed through the public WSS tunnel.

## MCP adapter

`bellhound mcp` serves MCP over standard input/output and exposes `bellhound_status`, `bellhound_dial`, `bellhound_wait`, `bellhound_speak`, and `bellhound_hangup`. The adapter uses the same loopback control listener and event history as the CLI. It does not expose `relay.control_bind`, raw shell access, arbitrary URLs, telephone-number overrides, or raw call-control identifiers.

One `bellhound_wait` returns an ordered batch of at most 32 events and a `next_cursor`. It waits at most 30 seconds, reports timeouts without advancing the cursor, and returns an explicit `history_gap` object when the requested cursor is older than retained history.

`dial`, `speak`, and `hangup` require a UUID `request_id`. Dial uses it as Telnyx's command ID. The relay retains the latest 256 MCP command IDs and returns a duplicate receipt without redispatching the same `speak` or `hangup`; reusing an ID with different arguments returns a tool-visible conflict.

MCP tool results include a generated correlation ID or request ID plus elapsed time. They whitelist conversation fields needed by the model and omit Bellhound/Telnyx private session identifiers. Tool acceptance means the command entered Bellhound, not that the telephone action completed; clients must keep consuming events.

## Adapter contract

An adapter for a model runtime needs only four operations:

1. start or locate the Bellhound relay;
2. call `dial` with opening text, which is queued and dispatched after relay `setup`;
3. block on `wait` and pass `utterance` text to the originating model;
4. translate the model's decision into `speak` or `hangup`.

The adapter must preserve `event_id`, send concise speech, and retain the runtime's ordinary approval rules. Telephone speech is input from a remote caller; it does not become trusted system instruction merely because it arrived by voice.
