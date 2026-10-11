# Bellhound operator reference

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

## Start

From the repository root:

```sh
session_json=$(skills/bellhound/scripts/session.sh start)
runtime_config=$(printf '%s' "$session_json" | jq -r '.config')
cursor=$(printf '%s' "$session_json" | jq -r '.event_cursor')
```

`start` builds the release binary when necessary, launches the local relay, creates a temporary public WSS tunnel, writes an ignored mode-600 runtime config, and returns JSON. It does not place a call.

## Dial and converse

```sh
target/release/bellhound --config "$runtime_config" dial \
  --opening 'Cora here. I found something that needs your judgment.'
```

Wait for one event at a time:

```sh
target/release/bellhound --config "$runtime_config" wait \
  --after "$cursor" --timeout-seconds 120
```

Set `cursor` to every returned `event_id`, including setup, spoken, interruption, and status events. On a final `utterance`, reason in the originating session, perform any ordinary authorized work, and answer:

```sh
target/release/bellhound --config "$runtime_config" speak \
  'I checked the live state. The deployment is healthy.'
```

## Finish and clean up

```sh
target/release/bellhound --config "$runtime_config" hangup \
  --reason conversation_complete
target/release/bellhound --config "$runtime_config" wait \
  --after "$cursor" --timeout-seconds 10
skills/bellhound/scripts/session.sh stop
```

Confirm the terminal event is `ended`. `stop` validates process identity before signaling the saved PIDs and removes only runtime PID files.

## Failure handling

- `no_active_session`: wait for `setup` before speaking.
- `wait_timeout`: retain the cursor and decide whether to wait once more or end the attempt.
- `event_history_gap`: surface the gap; do not pretend no turn was lost.
- Dial failure: run `stop`; do not redial automatically.
- Tunnel or relay startup failure: inspect the ignored mode-600 logs under `work/session/`, report a sanitized cause, then stop.
