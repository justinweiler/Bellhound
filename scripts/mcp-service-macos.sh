#!/bin/sh
set -eu

umask 077

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
root=$(CDPATH='' cd -- "$script_dir/.." && pwd -P)
label=com.justinweiler.bellhound-mcp
domain="gui/$(id -u)"
launch_agents=$HOME/Library/LaunchAgents
plist=$launch_agents/$label.plist
state_dir=$root/work/mcp-service
supervisor=$root/scripts/mcp-service-supervisor.py
python=$(command -v python3)
cargo=$(command -v cargo)
cargo_dir=$(dirname "$cargo")

render_plist() {
  mkdir -p "$launch_agents" "$state_dir"
  chmod 700 "$launch_agents" "$state_dir"
  "$python" - "$plist" "$label" "$python" "$supervisor" "$root" "$state_dir" "$cargo_dir" <<'PY'
import plistlib
from pathlib import Path
import sys

destination, label, python, supervisor, root, state, cargo_dir = sys.argv[1:]
payload = {
    "Label": label,
    "ProgramArguments": [python, supervisor, "--root", root],
    "WorkingDirectory": root,
    "RunAtLoad": True,
    "KeepAlive": {"SuccessfulExit": False},
    "ProcessType": "Background",
    "ThrottleInterval": 10,
    "EnvironmentVariables": {
        "PATH": f"{cargo_dir}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    },
    "StandardOutPath": str(Path(state) / "launchd.out.log"),
    "StandardErrorPath": str(Path(state) / "launchd.err.log"),
}
with open(destination, "wb") as output:
    plistlib.dump(payload, output, sort_keys=True)
PY
  chmod 600 "$plist"
  plutil -lint "$plist" >/dev/null
}

case "${1:-}" in
  check)
    exec "$python" "$supervisor" --root "$root" --check
    ;;
  render)
    render_plist
    printf '%s\n' "$plist"
    ;;
  install)
    "$cargo" build --manifest-path "$root/Cargo.toml" --release --locked
    "$python" "$supervisor" --root "$root" --check >/dev/null
    render_plist
    launchctl bootout "$domain/$label" >/dev/null 2>&1 || true
    launchctl bootstrap "$domain" "$plist"
    launchctl enable "$domain/$label"
    launchctl kickstart -k "$domain/$label"
    printf '%s\n' "installed $label"
    ;;
  status)
    exec launchctl print "$domain/$label"
    ;;
  stop)
    launchctl bootout "$domain/$label"
    printf '%s\n' "stopped $label; plist retained at $plist"
    ;;
  uninstall)
    launchctl bootout "$domain/$label" >/dev/null 2>&1 || true
    rm -f "$plist"
    printf '%s\n' "uninstalled $label"
    ;;
  *)
    echo 'usage: mcp-service-macos.sh {check|render|install|status|stop|uninstall}' >&2
    exit 2
    ;;
esac
