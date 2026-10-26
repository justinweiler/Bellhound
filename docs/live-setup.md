# Live setup

This is the detailed operator reference after installation. New operators should begin with the start-to-finish [installation guide](../INSTALL.md).

## Prerequisites

- A Telnyx paid or otherwise Programmable-Voice-enabled account. An AI-only Pretrial account must be upgraded before using Bellhound.
- An owned voice-capable number.
- A verified destination number while the Telnyx trial restriction applies.
- An active outbound voice profile.
- A dedicated Call Control application attached to that profile.
- A public WSS endpoint forwarding to Bellhound's loopback listener.

Telnyx Call Control applications require a primary webhook URL. Use the HTTPS form of the same stable relay host:

```text
https://host.example/webhooks/telnyx/<token>
```

If a temporary tunnel changes hostname, update both this webhook URL and `telnyx.public_wss_base` before dialing.

## Credentials

Bellhound checks the configured environment variable first. If absent, it reads the configured operating-system credential entry.

Example macOS Keychain metadata:

```toml
[credentials]
api_key_env = "TELNYX_API_KEY"
keyring_service = "Telnyx API key"
keyring_account = "your-account@example.com"
```

The API key is never emitted in events or diagnostics. The in-memory value is zeroed when dropped.

Use `bellhound credentials` to prove resolution without printing the value.

## Relay URL token

Create a separate random token for the public URL path. Keep it outside the TOML file and expose it through `BELLHOUND_RELAY_TOKEN`.

The public paths are:

```text
wss://host.example/conversation-relay/<token>
https://host.example/webhooks/telnyx/<token>
```

The token prevents unauthenticated connections during the prototype. A production webhook should additionally verify Telnyx's Ed25519 signature.

## Run

Start the local relay:

```sh
bellhound --config bellhound.toml relay
```

Bellhound binds its public-tunnel listener and private model-control listener separately. Forward only `relay.bind` through the TLS tunnel. Keep `relay.control_bind` on loopback.

Bellhound requests interim STT results and the default configuration waits for 4000 ms of transcript quiet so final fragments separated by natural pauses become one caller turn. The four-second window preserves a deliberate three-second human pause observed in the October 26 canary. A partial frame received after an apparent final reopens the continuation buffer and suppresses the holding acknowledgement while the caller continues. Bellhound then speaks `Got it. Give me a moment.` when no targeted agent reply arrives within another 500 ms. Configure `relay.continuation_window_ms` and `[acknowledgement]` to tune these delays. `acknowledgement.suppress_phrases` prevents the holding phrase from answering brief or compound social closings, including a short possessive term of address such as “goodbye, my dear”; `suppress_for_session_phrases` disables further acknowledgements after a voicemail announcement. Bellhound cancels an unfired acknowledgement when the real reply arrives.

`mode = "sound"` sends Telnyx a preemptible, interruptible `play` frame. The default `sound_url` is the public Bellhound cue hosted in `justinweiler/Bellhound-Sounds`; replace it with any public HTTPS audio asset. Without `sound_url`, Bellhound serves its built-in MP3 at a token-authenticated path under the same HTTPS/WSS host. Use sound mode only after proving that Telnyx accepts the configured media URL. The accountless `trycloudflare.com` quick tunnel carried the relay WebSocket but Telnyx did not fetch its embedded-audio URL during the October 11 canary.

Start a TLS tunnel or reverse proxy to the configured loopback address. Put its WSS URL in the ignored local `[telnyx]` section.

In another process, dial:

```sh
bellhound --config bellhound.toml dial \
  --opening "Justin, I found something and need your judgment."
```

The Dial request embeds Conversation Relay. Telnyx starts STT/TTS automatically when the destination answers and connects to the WSS URL. A short “Bellhound” primer completes and warms Telnyx's greeting path; Bellhound queues the requested opening locally and dispatches it after Telnyx sends `setup`, producing a timestamped `spoken` event for the call-specific introduction.

Bellhound defaults to `Telnyx.NaturalHD.astra`, a voice verified against Telnyx's standalone TTS endpoint. Ultra voices require a real Ultra voice ID; a friendly display name such as `Telnyx.Ultra.Callie` is invalid and can produce a connected but silent call.

## Agent loop

Read one JSON line at a time from Bellhound stdout, or block on the model-neutral CLI adapter:

```sh
bellhound --config bellhound.toml wait --after 0
```

Speak with:

```json
{"command":"speak","text":"I found a mismatch between the live configuration and the expected release."}
```

End with:

```json
{"command":"hangup","reason":"conversation_complete"}
```

Bellhound uses the `call_control_id` from Telnyx's setup frame to hang up the telephone leg after ending Conversation Relay. Its duration limit follows the same path, so a stuck relay cannot leave a live call behind.

Conversation Relay does not return a WebSocket event when TTS playback completes.
Bellhound therefore reports a conservative `playback_remaining_ms` estimate on
each `spoken` event and exposes the remaining estimate in control/MCP status.
MCP hangup fails closed until it reaches zero. Continue waiting for caller
activity during that interval; caller speech or an interruption clears the
estimate immediately.

Do not log the ignored local configuration: it contains telephone numbers and account resource IDs even though it contains no password.

For direct stdio clients, the proven private OpenAI Secure MCP Tunnel path, credential separation, and the optional macOS user service, see [MCP integration](mcp.md). The normal live setup does not require MCP.
