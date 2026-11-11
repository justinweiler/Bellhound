# Bellhound operator reference

For a structured end-to-end test, use [the conversation canary](../../../docs/conversation-canary.md).

## Local installation

- Repository: `/Users/justinweiler/Bellhound`
- Base configuration: `/Users/justinweiler/Bellhound/work/bellhound-keychain.toml`
- API key: operating-system credential store, as configured in the base TOML
- Relay token on Justin's Mac: macOS Keychain service `Bellhound relay token`, account `default`

Do not display either credential. On Justin's Mac, resolve the relay token inside the command that needs it:

```sh
export BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password \
  -s 'Bellhound relay token' -a 'default' -w)
```

Separate execution-tool calls do not inherit shell exports. Resolve the token again inside every later tool call that invokes `bellhound`; never echo it.

## Start with a foreground owner

Launch the supervisor as a long-running execution tool call from the repository root:

```sh
export BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password \
  -s 'Bellhound relay token' -a 'default' -w)
skills/bellhound/scripts/session.sh run
```

The tool should yield a running-session identifier after printing one `ready` JSON object. Retain that identifier; do not terminate or abandon the owner while the call is active. The owner builds the release binary when necessary, launches the local relay and temporary WSS tunnel as children, writes an ignored mode-600 runtime config, and remains in the foreground. It applies a default seven-minute lifetime and cleans both children on signal, child failure, or timeout.

Read `config` and `event_cursor` from the ready JSON. On Justin's installation the runtime config is `/Users/justinweiler/Bellhound/work/session/bellhound.toml`; use that literal path in later tool calls because shell variables do not persist between them. Before dialing, prove ownership across at least three separate tool turns:

```sh
skills/bellhound/scripts/session.sh status
BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password -s 'Bellhound relay token' -a 'default' -w) \
  target/release/bellhound --config work/session/bellhound.toml ping
BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password -s 'Bellhound relay token' -a 'default' -w) \
  target/release/bellhound --config work/session/bellhound.toml wait \
  --after 1 --timeout-seconds 5
```

Status must report `owner_running`, `relay_running`, and `tunnel_running` as true. The final command must return `pong`. Repeat status once more in a later tool turn. This survival check is required after changing execution environments or ownership behavior.

## Dial and converse

For hosted MCP, call `bellhound_status` immediately before dial in the same agent
turn. Require a new `correlation_id`, current `server_observed_at_unix_ms`,
`service_state=ready`, `control_reachable=true`, `active_call=false`, `opening_pending=false`, and
`speech_playback_pending_ms=0`. Do not reuse an earlier result. If this hosted
round trip fails, times out, or returns `retryable=true`, the tunnel is not yet
proven fresh. Wait `retry_after_seconds` (10 seconds by default) and retry status
only, up to six total attempts over one minute. Never retry dial. Use normal text
fallback only after that bounded recovery window expires without a safe status.

```sh
BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password -s 'Bellhound relay token' -a 'default' -w) \
  target/release/bellhound --config work/session/bellhound.toml dial \
  --opening 'Cora here. I found something that needs your judgment.'
```

Wait for one event at a time. Replace the sample `5` with the last processed numeric event ID:

```sh
BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password -s 'Bellhound relay token' -a 'default' -w) \
  target/release/bellhound --config work/session/bellhound.toml wait \
  --after 5 --timeout-seconds 120
```

Set `cursor` to every returned `event_id`, including setup, spoken, interruption, and status events. On a final `utterance`, reason in the originating session, perform any ordinary authorized work, and answer:

```sh
BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password -s 'Bellhound relay token' -a 'default' -w) \
  target/release/bellhound --config work/session/bellhound.toml speak \
  'I checked the live state. The deployment is healthy.' \
  --responding-to 6
```

Use the `sequence` from the final utterance being answered. Bellhound rejects unsequenced speech while a caller turn is active. If it emits `missing_response_sequence` or `stale_response`, do not resend blindly: advance through buffered events, reassess the latest caller turn, and answer that sequence. Bellhound requests interim STT frames so resumed speech reopens the continuation buffer and suppresses the holding acknowledgement even when partial transcripts remain hidden. By default, Bellhound merges continuation fragments after 4000 ms of transcript quiet, preserving an observed three-second human pause, and says the fixed phrase `Got it. Give me a moment.` when the targeted reply does not arrive within another 500 ms. Advance the cursor through its `acknowledged` event and continue the same agent turn. Social, closing, and voicemail phrases may intentionally receive no acknowledgement.

The eventual `spoken` event reports `response_latency_ms`, `acknowledgement_preceded`, and `playback_remaining_ms`. It confirms dispatch, not audible completion. Before a farewell hangup, call `bellhound_wait` with a timeout at least as long as the remaining playback estimate. If it returns caller activity, handle that turn instead of hanging up. After a quiet timeout, require `bellhound_status.speech_playback_pending_ms` to be zero before calling hangup. The acknowledgement confirms that Bellhound heard the caller; it is not an answer from another model.

The optional `sound` mode uses a short cue that is interruptible by the caller and preemptible by the agent reply. Bellhound defaults `sound_url` to the public `justinweiler/Bellhound-Sounds` asset and permits any other public HTTPS cue. Without a URL, Bellhound exposes its embedded cue through the relay host. Enable sound mode only after proving that Telnyx fetches the media URL. The accountless quick tunnel did not receive Telnyx's embedded media fetch during the October 11 pilot.

## Finish and clean up

```sh
BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password -s 'Bellhound relay token' -a 'default' -w) \
  target/release/bellhound --config work/session/bellhound.toml hangup \
  --reason conversation_complete
BELLHOUND_RELAY_TOKEN=$(/usr/bin/security find-generic-password -s 'Bellhound relay token' -a 'default' -w) \
  target/release/bellhound --config work/session/bellhound.toml wait \
  --after 5 --timeout-seconds 10
skills/bellhound/scripts/session.sh stop
```

Confirm the terminal event is `ended`. `stop` first requests telephone hangup while the relay is alive, signals the saved foreground owner, waits for its cleanup trap, validates child process identity before fallback signaling, and removes only runtime PID files. Poll the original owner execution session to completion, then require:

```json
{"owner_running":false,"relay_running":false,"tunnel_running":false,"config":null}
```

## Failure handling

- `no_active_session`: wait for `setup` before speaking.
- `wait_timeout`: retain the cursor and decide whether to wait once more or end the attempt.
- `event_history_gap`: surface the gap; do not pretend no turn was lost.
- Dial failure: run `stop`; do not redial automatically.
- Tunnel, relay, or owner failure: inspect the ignored mode-600 logs under `work/session/`, report a sanitized cause, issue direct telephone cleanup if the relay is unavailable, then stop. Do not redial automatically.
