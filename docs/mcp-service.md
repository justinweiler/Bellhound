# Always-on MCP service

Bellhound `0.6.0` ships a macOS user-service supervisor for private, always-on MCP use.
It runs inside the signed-in user's session so it can read the three existing
Keychain entries without copying secrets into a plist, environment file, shell
profile, repository file, or command line.

The first supported service host is macOS. The foreground supervisor keeps its
process and secret boundaries portable, but native Linux systemd and Windows
service installers remain follow-up packaging work.

## Owned processes

The service owns one process tree:

```text
launchd user agent
  -> mcp-service-supervisor.py
       -> session.sh run
            -> bellhound relay
            -> cloudflared temporary WSS tunnel
       -> tunnel-client
            -> bellhound mcp
```

The supervisor starts the telephone relay first, waits for its generated live
configuration, and only then starts the OpenAI tunnel. If either child exits,
the supervisor stops the other path, requests telephone cleanup, writes a
mode-600 receipt, and exits nonzero so launchd can restart it.

## Credentials

The service reads these macOS Keychain entries at startup:

| Service | Account | Purpose |
|---|---|---|
| `Telnyx API key` | Configured in the private Bellhound TOML | Carrier Dial and Call Control |
| `Bellhound relay token` | `default` | Public relay URL and private control authentication |
| `Bellhound OpenAI tunnel runtime key` | `tunnel-client` | OpenAI Secure MCP Tunnel registration |

Only the two runtime secrets are inherited by the supervisor's children. The
Telnyx key remains resolved by Bellhound's existing credential-store path.
Logs are redacted defensively and stored under ignored `work/mcp-service/`.

## Call-spend traps

`relay.max_call_seconds` is the single configured limit. Bellhound validates it
between 30 and 3600 seconds. Justin's current private configuration uses 300
seconds.

Three independent enforcement layers use that value:

1. The Telnyx Dial request sets `time_limit_secs`, so the carrier owns the primary hard stop even if every local process fails.
2. Bellhound's relay timer sends Conversation Relay `end`, invokes Telnyx Call Control hangup, and closes the WebSocket at the configured limit.
3. The service watchdog observes the local active-call flag. At the configured limit plus five seconds it issues `service_watchdog` hangup. If the call remains active or the control path disappears, the service fails closed, cleans the process tree, and lets launchd restart it.

The owner cleanup path also attempts hangup before stopping the relay. These
traps do not justify leaving a known active call unattended; every normal agent
conversation still ends with explicit hangup and an `ended` event.

## Validate without installing

From a source checkout, build before validating:

```sh
cargo build --release --locked
```

A prebuilt release archive already contains the executable at its root and does
not require Rust or Cargo. From either layout, validate configuration, binaries,
profiles, and Keychain availability without opening a network connection:

```sh
scripts/mcp-service-macos.sh check
```

Render and lint the LaunchAgent without loading it:

```sh
scripts/mcp-service-macos.sh render
plutil -lint "$HOME/Library/LaunchAgents/com.justinweiler.bellhound-mcp.plist"
```

The generated plist contains only paths and launch policy. It never contains a
credential or telephone number.

## Install

```sh
scripts/mcp-service-macos.sh install
scripts/mcp-service-macos.sh status
```

Installation builds a source checkout when necessary, or uses the executable at
the root of a prebuilt release archive. It then writes one mode-600 user LaunchAgent at
`~/Library/LaunchAgents/com.justinweiler.bellhound-mcp.plist`, loads it in the
current GUI domain, and starts it. The service is available after login and is
restarted when it exits unexpectedly. Reinstallation and stop operations wait up
up to 120 seconds for launchd to remove the prior process, for live readiness to
disappear, and for the supervisor's service lock to become acquirable before a
new bootstrap. The LaunchAgent gives the supervisor the same 120-second exit
allowance so launchd cannot apply its short default while bounded child-process
teardown is still running. This avoids overlap with an agent finishing cleanup
and preserves the terminal receipt.

The LaunchAgent runtime never invokes Cargo. `install` owns compilation before
bootstrap; supervised startup removes Cargo from `PATH`, sets
`BELLHOUND_AUTO_BUILD=0`, and fails fast with an instruction to rerun `install`
if a source checkout becomes newer than its release binary. This keeps a cold or
lock-contended build outside the 60-second owner-readiness window.

Keep a prebuilt archive in a stable directory before installing. The LaunchAgent
records absolute paths. To move or upgrade that directory, uninstall from the old
directory and install from the new one.

Runtime readiness is recorded at `work/mcp-service/ready.json`. Each run carries
one `service_run_id` plus its start timestamp. When a new run acquires the service
lock it removes the previous terminal receipt, preventing an old failure from
appearing current beside live readiness. A normal service stop removes the ready
file and writes `work/mcp-service/receipt.json` with the same run ID plus start
and finish timestamps.

## Stop or remove

```sh
scripts/mcp-service-macos.sh stop
scripts/mcp-service-macos.sh uninstall
```

`stop` unloads the current service but retains the plist. `uninstall` unloads it
and removes that exact plist. Neither operation deletes Keychain entries,
Bellhound configuration, the OpenAI app, or tunnel metadata.
