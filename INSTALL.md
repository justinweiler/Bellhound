# Install Bellhound

This guide takes a new operator from an empty machine to a first outbound call.
Bellhound's core binary is model-neutral and runs on macOS, Linux, and Windows.
The packaged always-on service installer currently supports macOS.

## 1. Create and prepare a Telnyx account

1. [Create a Telnyx account](https://telnyx.com/sign-up) and complete any identity or payment verification requested in Mission Control Portal.
2. Upgrade an AI-only **Pretrial** account before using Bellhound. Bellhound uses Programmable Voice and Call Control; Telnyx Pretrial access is limited to its AI product suite. Telnyx may also restrict new accounts to verified destinations or add an automated-call announcement until verification is complete.
3. In **API Keys**, create a Telnyx API v2 key. Copy it directly into your operating-system credential store when prompted; never put it in this repository, a TOML file, a shell script, or chat.
4. Buy a voice-capable telephone number. This is Bellhound's `from_number`.
5. Create or select an outbound voice profile and start with an outbound channel limit of `1` while testing.

You will create the Call Control application after your public relay URL exists in step 5.

## 2. Install Bellhound

### Prebuilt archive

Download the archive and matching `.sha256` file for your machine from [GitHub Releases](https://github.com/justinweiler/Bellhound/releases):

| Machine | Release target | Archive |
|---|---|---|
| macOS, Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |
| macOS, Intel | `x86_64-apple-darwin` | `.tar.gz` |
| Linux, x86-64 glibc | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Windows, x86-64 | `x86_64-pc-windows-msvc` | `.zip` |

Verify and extract a macOS or Linux archive:

```sh
archive=bellhound-0.6.0-aarch64-apple-darwin.tar.gz
shasum -a 256 -c "$archive.sha256"
tar -xzf "$archive"
cd "${archive%.tar.gz}"
./bellhound --version
```

Replace the sample archive name with the file you downloaded. On Linux, use
`sha256sum -c` when `shasum` is unavailable. In PowerShell:

```powershell
$archive = Get-Item .\bellhound-*.zip
$expected = (Get-Content "$archive.sha256").Split()[0]
$actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "Bellhound checksum mismatch" }
Expand-Archive $archive -DestinationPath .
$folder = [IO.Path]::GetFileNameWithoutExtension([IO.Path]::GetFileNameWithoutExtension($archive.Name))
Set-Location $folder
.\bellhound.exe --version
```

Keep the extracted directory in a stable location if you install the macOS service. Its LaunchAgent records that absolute path.

### Build from source

Install the stable [Rust toolchain](https://rustup.rs/), then:

```sh
git clone https://github.com/justinweiler/Bellhound.git
cd Bellhound
cargo build --release --locked
./target/release/bellhound --version
```

In the commands below, use `./target/release/bellhound` instead of `./bellhound` when working from a source checkout. On Windows, use `bellhound.exe`.

## 3. Create a private configuration

Copy the example to a file that stays outside source control:

```sh
cp bellhound.example.toml bellhound.toml
```

Choose the credential-store account label you will use and set it in `bellhound.toml`:

```toml
[credentials]
api_key_env = "TELNYX_API_KEY"
keyring_service = "Telnyx API key"
keyring_account = "bellhound"
```

Store the API key without placing its value on the command line. On macOS, this prompts securely:

```sh
security add-generic-password -U -s "Telnyx API key" -a "bellhound" -w
```

On Windows, save the same service/account pair in Windows Credential Manager. On Linux, save it in the desktop Secret Service/keyring. For a short-lived development shell on any platform, `TELNYX_API_KEY` may be exported instead; Bellhound checks that variable before the credential store.

Create a separate, unguessable relay token. It protects Bellhound's public URL and must contain at least 24 characters:

```sh
export BELLHOUND_RELAY_TOKEN="$(openssl rand -hex 32)"
```

Keep that shell open while completing the first-call steps. The relay token is not the Telnyx API key.

Validate the local file and prove that Bellhound can resolve the API key without displaying it:

```sh
./bellhound --config bellhound.toml check
./bellhound --config bellhound.toml credentials
```

## 4. Prove the local agent loop

This simulation uses no Telnyx credit and makes no network connection:

```sh
./bellhound --config bellhound.toml simulate --script examples/crocodile-call.jsonl
```

Bellhound prints one JSON event per line. Enter a `speak` command to advance the script:

```json
{"command":"speak","text":"Tell me what happened.","responding_to_sequence":1}
```

Press `Ctrl-C` after the simulation is complete.

## 5. Give the relay a public TLS address

Bellhound listens only on `127.0.0.1:8787`. Put a TLS tunnel or reverse proxy in front of that listener and expose only that port. Never expose the private control listener on `127.0.0.1:8788`.

For durable use, choose a stable hostname such as:

```text
https://bellhound.example.com
wss://bellhound.example.com
```

For an initial experiment, install [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) and use its accountless quick-tunnel mode. Its hostname can change every time it restarts. When it changes, update both `public_wss_base` in the configuration and the Call Control application's webhook URL before dialing.

Start Bellhound first:

```sh
./bellhound --config bellhound.toml relay
```

In another terminal, start the temporary tunnel:

```sh
cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8787
```

Copy the emitted `https://...trycloudflare.com` hostname. Use `wss://` for
`public_wss_base` and `https://` for the Telnyx webhook. Keep both processes
running. A named Cloudflare Tunnel, another stable tunnel, or your own reverse
proxy is a better durable installation.

## 6. Create the Telnyx Call Control application

In Mission Control Portal:

1. Open **Real-Time Communications > Voice > Programmable Voice > Call Control Applications**.
2. Create a dedicated application named `Bellhound` and select webhook API version `2`.
3. Set its primary webhook URL to the HTTPS form of your relay host plus Bellhound's webhook path and relay token:

   ```text
   https://bellhound.example.com/webhooks/telnyx/<your-relay-token>
   ```

4. Attach the outbound voice profile from step 1.
5. Record the application's ID. Telnyx calls this `connection_id` in the Dial API.

The URL token is prototype authentication. For an Internet-facing production installation, also verify Telnyx webhook signatures at the edge.

## 7. Add the live call settings

Add this section to the private `bellhound.toml`:

```toml
[telnyx]
connection_id = "your-call-control-application-id"
from_number = "+12025550123"
to_number = "+12025550124"
public_wss_base = "wss://bellhound.example.com/conversation-relay"
voice = "Telnyx.NaturalHD.astra"
language = "en-US"
transcription_engine = "Deepgram"
transcription_model = "deepgram/nova-3"
```

Use `+E.164` telephone numbers. `from_number` must be a number your Telnyx account may use; new or trial accounts may require `to_number` to be verified.

Run the validation again:

```sh
./bellhound --config bellhound.toml check
./bellhound --config bellhound.toml credentials
```

## 8. Make the first call

With the relay and tunnel still running:

```sh
./bellhound --config bellhound.toml dial \
  --opening "Hello. Bellhound is ready for its first conversation."
```

Read the next caller turn from another terminal:

```sh
./bellhound --config bellhound.toml wait --after 0 --timeout-seconds 120
```

`wait` is cursor-based. Repeat it with `--after` set to the last returned
`event_id` until you receive an `utterance` event. Continue advancing that
cursor after every event so the agent never loses or repeats a turn.

Reply using the `sequence` from the `utterance` event:

```sh
./bellhound --config bellhound.toml speak \
  "I heard you clearly." --responding-to 1
```

End the call explicitly:

```sh
./bellhound --config bellhound.toml hangup --reason first_call_complete
```

The default `max_call_seconds = 300` is also sent to Telnyx as the carrier-enforced call limit. Keep that protection enabled while experimenting.

## 9. Connect an agent

- For any agent that can keep a process open, use Bellhound's [JSON Lines protocol](docs/protocol.md).
- For tool-using agents, use the cursor-based `wait`, `speak`, and `hangup` commands shown above.
- For local or hosted MCP clients, follow [MCP integration](docs/mcp.md).
- For the private always-on macOS service, follow [Always-on MCP service](docs/mcp-service.md).
- For a repeatable human acceptance test, use the [conversation canary](docs/conversation-canary.md).

The reasoning model remains the phone agent in every mode. Bellhound handles telephone transport, speech frames, timing, and safety limits.

## Troubleshooting

| Symptom | Check |
|---|---|
| `credential ... not found` | The TOML service/account labels exactly match the operating-system credential entry. |
| Call connects but is silent | Keep `voice = "Telnyx.NaturalHD.astra"`; arbitrary friendly names are not valid Ultra voice IDs. Confirm Telnyx can reach the public WSS URL. |
| Telnyx cannot connect | The public tunnel forwards only to `127.0.0.1:8787`, the relay token matches the process environment, and the WSS path ends in `/conversation-relay/<token>`. |
| Dial is rejected | Confirm the API key, Call Control application ID, outbound profile, owned `from_number`, verified destination, and account verification status. |
| A temporary tunnel restarted | Update `public_wss_base` and the Telnyx application's HTTPS webhook URL before trying again. |
| Agent replies to the wrong turn | Pass the exact `utterance.sequence` with `--responding-to`; Bellhound rejects stale targeted replies. |

Bellhound deliberately keeps configuration, carrier credentials, relay tokens, and personal telephone numbers out of release archives and source control.
