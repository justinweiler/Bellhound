#!/bin/sh
set -eu

umask 077

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
if [ -n "${BELLHOUND_ROOT:-}" ]; then
  root=$BELLHOUND_ROOT
elif root=$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null); then
  :
else
  root=$(CDPATH='' cd -- "$script_dir/../../.." && pwd -P)
fi
base_config=${BELLHOUND_CONFIG:-$root/work/bellhound-keychain.toml}
state_dir=$root/work/session
runtime_config=$state_dir/bellhound.toml
relay_pid_file=$state_dir/relay.pid
tunnel_pid_file=$state_dir/tunnel.pid
relay_log=$state_dir/relay.log
tunnel_log=$state_dir/tunnel.log
binary=$root/target/release/bellhound

require_token() {
  if [ -z "${BELLHOUND_RELAY_TOKEN:-}" ] || [ "${#BELLHOUND_RELAY_TOKEN}" -lt 24 ]; then
    echo 'BELLHOUND_RELAY_TOKEN must contain at least 24 characters' >&2
    exit 1
  fi
}

pid_is() {
  pid_file=$1
  expected=$2
  [ -f "$pid_file" ] || return 1
  pid=$(cat "$pid_file")
  case "$pid" in *[!0-9]*|'') return 1 ;; esac
  kill -0 "$pid" 2>/dev/null || return 1
  ps -p "$pid" -o command= | grep -F "$expected" >/dev/null 2>&1
}

stop_pid() {
  pid_file=$1
  expected=$2
  if pid_is "$pid_file" "$expected"; then
    pid=$(cat "$pid_file")
    kill "$pid"
    count=0
    while kill -0 "$pid" 2>/dev/null && [ "$count" -lt 40 ]; do
      sleep 0.1
      count=$((count + 1))
    done
  fi
  rm -f "$pid_file"
}

json_status() {
  relay=false
  tunnel=false
  pid_is "$relay_pid_file" 'bellhound' && relay=true
  pid_is "$tunnel_pid_file" 'cloudflared' && tunnel=true
  python3 - "$relay" "$tunnel" "$runtime_config" <<'PY'
import json, sys
print(json.dumps({
    "relay_running": sys.argv[1] == "true",
    "tunnel_running": sys.argv[2] == "true",
    "config": sys.argv[3] if sys.argv[1] == "true" else None,
}, separators=(",", ":")))
PY
}

start() {
  require_token
  command -v cloudflared >/dev/null 2>&1 || { echo 'cloudflared is required' >&2; exit 1; }
  command -v python3 >/dev/null 2>&1 || { echo 'python3 is required' >&2; exit 1; }
  [ -f "$base_config" ] || { echo "configuration not found: $base_config" >&2; exit 1; }
  mkdir -p "$state_dir"

  if pid_is "$relay_pid_file" 'bellhound' || pid_is "$tunnel_pid_file" 'cloudflared'; then
    echo 'a Bellhound session is already running; use status or stop first' >&2
    exit 1
  fi
  rm -f "$relay_pid_file" "$tunnel_pid_file" "$runtime_config" "$relay_log" "$tunnel_log"

  if [ ! -x "$binary" ] || find "$root/src" "$root/Cargo.toml" "$root/Cargo.lock" -newer "$binary" -print -quit | grep -q .; then
    cargo build --manifest-path "$root/Cargo.toml" --release --locked >&2
  fi

  env BELLHOUND_RELAY_TOKEN="$BELLHOUND_RELAY_TOKEN" \
    nohup "$binary" --config "$base_config" relay >"$relay_log" 2>&1 &
  echo $! >"$relay_pid_file"

  ready=''
  count=0
  while [ "$count" -lt 40 ]; do
    if ! pid_is "$relay_pid_file" 'bellhound'; then
      echo 'Bellhound relay exited during startup' >&2
      tail -20 "$relay_log" >&2 || true
      stop
      exit 1
    fi
    ready=$(env BELLHOUND_RELAY_TOKEN="$BELLHOUND_RELAY_TOKEN" \
      "$binary" --config "$base_config" wait --after 0 --timeout-seconds 1 2>/dev/null || true)
    if printf '%s' "$ready" | grep -q '"event":"ready"'; then break; fi
    count=$((count + 1))
  done
  printf '%s' "$ready" | grep -q '"event":"ready"' || {
    echo 'Bellhound relay did not become ready' >&2
    stop
    exit 1
  }

  nohup cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8787 \
    >"$tunnel_log" 2>&1 &
  echo $! >"$tunnel_pid_file"

  public_url=''
  count=0
  while [ "$count" -lt 120 ]; do
    if ! pid_is "$tunnel_pid_file" 'cloudflared'; then
      echo 'cloudflared exited during startup' >&2
      tail -20 "$tunnel_log" >&2 || true
      stop
      exit 1
    fi
    public_url=$(sed -nE 's/.*(https:\/\/[a-z0-9-]+\.trycloudflare\.com).*/\1/p' "$tunnel_log" | head -1 || true)
    [ -n "$public_url" ] && break
    sleep 0.25
    count=$((count + 1))
  done
  [ -n "$public_url" ] || { echo 'temporary tunnel URL was not produced' >&2; stop; exit 1; }

  python3 - "$base_config" "$runtime_config" "$public_url" <<'PY'
from pathlib import Path
import re, sys
source = Path(sys.argv[1]).read_text()
replacement = f'public_wss_base = "{sys.argv[3].replace("https://", "wss://")}/conversation-relay"'
updated, count = re.subn(r'^public_wss_base\s*=.*$', replacement, source, count=1, flags=re.MULTILINE)
if count != 1:
    raise SystemExit('the base configuration must contain exactly one public_wss_base')
Path(sys.argv[2]).write_text(updated)
PY
  chmod 600 "$runtime_config" "$relay_log" "$tunnel_log" "$relay_pid_file" "$tunnel_pid_file"

  cursor=$(printf '%s' "$ready" | python3 -c 'import json,sys; print(json.load(sys.stdin)["event_id"])')
  python3 - "$runtime_config" "$cursor" <<'PY'
import json, sys
print(json.dumps({
    "state": "ready",
    "config": sys.argv[1],
    "event_cursor": int(sys.argv[2]),
}, separators=(",", ":")))
PY
}

stop() {
  if [ -x "$binary" ] && [ -f "$runtime_config" ] && pid_is "$relay_pid_file" 'bellhound' && [ -n "${BELLHOUND_RELAY_TOKEN:-}" ]; then
    env BELLHOUND_RELAY_TOKEN="$BELLHOUND_RELAY_TOKEN" \
      "$binary" --config "$runtime_config" hangup --reason session_cleanup >/dev/null 2>&1 || true
    sleep 0.5
  fi
  stop_pid "$tunnel_pid_file" 'cloudflared'
  stop_pid "$relay_pid_file" 'bellhound'
  python3 - <<'PY'
import json
print(json.dumps({"state":"stopped"}, separators=(",", ":")))
PY
}

case "${1:-}" in
  start) start ;;
  status) json_status ;;
  stop) stop ;;
  *) echo 'usage: session.sh {start|status|stop}' >&2; exit 2 ;;
esac
