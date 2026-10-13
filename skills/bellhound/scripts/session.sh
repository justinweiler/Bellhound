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
owner_pid_file=$state_dir/owner.pid
relay_pid_file=$state_dir/relay.pid
tunnel_pid_file=$state_dir/tunnel.pid
owner_log=$state_dir/owner.log
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
    kill -INT "$pid"
    count=0
    while kill -0 "$pid" 2>/dev/null && [ "$count" -lt 40 ]; do
      sleep 0.1
      count=$((count + 1))
    done
    if pid_is "$pid_file" "$expected"; then
      kill -KILL "$pid"
    fi
    wait "$pid" 2>/dev/null || true
  fi
  rm -f "$pid_file"
}

log_owner() {
  printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$*" >>"$owner_log"
}

call_is_active() {
  [ -f "$relay_log" ] || return 1
  lifecycle=$(grep -E '"event":"(connected|ended)"' "$relay_log" | tail -1 || true)
  printf '%s' "$lifecycle" | grep -F '"event":"connected"' >/dev/null 2>&1
}

hangup_if_active() {
  if [ -x "$binary" ] && [ -f "$runtime_config" ] && pid_is "$relay_pid_file" 'bellhound' && [ -n "${BELLHOUND_RELAY_TOKEN:-}" ] && call_is_active; then
    env BELLHOUND_RELAY_TOKEN="$BELLHOUND_RELAY_TOKEN" \
      "$binary" --config "$runtime_config" hangup --reason session_cleanup >/dev/null 2>&1 || true
    sleep 0.5
  fi
}

cleanup_children() {
  hangup_if_active
  stop_pid "$tunnel_pid_file" 'cloudflared'
  stop_pid "$relay_pid_file" 'bellhound'
}

json_status() {
  owner=false
  relay=false
  tunnel=false
  pid_is "$owner_pid_file" 'session.sh run' && owner=true
  pid_is "$relay_pid_file" 'bellhound' && relay=true
  pid_is "$tunnel_pid_file" 'cloudflared' && tunnel=true
  python3 - "$owner" "$relay" "$tunnel" "$runtime_config" <<'PY'
import json, sys
print(json.dumps({
    "owner_running": sys.argv[1] == "true",
    "relay_running": sys.argv[2] == "true",
    "tunnel_running": sys.argv[3] == "true",
    "config": sys.argv[4] if sys.argv[2] == "true" else None,
}, separators=(",", ":")))
PY
}

owner_exit() {
  code=$?
  trap - EXIT HUP INT TERM
  log_owner "owner exiting with status $code"
  cleanup_children
  rm -f "$owner_pid_file"
  exit "$code"
}

run() {
  require_token
  command -v cloudflared >/dev/null 2>&1 || { echo 'cloudflared is required' >&2; exit 1; }
  command -v python3 >/dev/null 2>&1 || { echo 'python3 is required' >&2; exit 1; }
  [ -f "$base_config" ] || { echo "configuration not found: $base_config" >&2; exit 1; }

  max_seconds=${BELLHOUND_OWNER_MAX_SECONDS:-420}
  case "$max_seconds" in *[!0-9]*|'') echo 'BELLHOUND_OWNER_MAX_SECONDS must be an integer' >&2; exit 1 ;; esac
  if [ "$max_seconds" -ne 0 ] && { [ "$max_seconds" -lt 30 ] || [ "$max_seconds" -gt 3600 ]; }; then
    echo 'BELLHOUND_OWNER_MAX_SECONDS must be 0 for unbounded service mode or between 30 and 3600' >&2
    exit 1
  fi

  mkdir -p "$state_dir"
  if pid_is "$owner_pid_file" 'session.sh run' || pid_is "$relay_pid_file" 'bellhound' || pid_is "$tunnel_pid_file" 'cloudflared'; then
    echo 'a Bellhound session is already running; use status or stop first' >&2
    exit 1
  fi
  rm -f "$owner_pid_file" "$relay_pid_file" "$tunnel_pid_file" "$runtime_config" "$owner_log" "$relay_log" "$tunnel_log"
  : >"$owner_log"
  echo $$ >"$owner_pid_file"
  chmod 600 "$owner_pid_file" "$owner_log"
  trap owner_exit EXIT
  trap 'exit 0' HUP INT TERM
  log_owner 'owner started'

  if [ ! -x "$binary" ] || find "$root/src" "$root/Cargo.toml" "$root/Cargo.lock" -newer "$binary" -print -quit | grep -q .; then
    cargo build --manifest-path "$root/Cargo.toml" --release --locked >&2
  fi

  env BELLHOUND_RELAY_TOKEN="$BELLHOUND_RELAY_TOKEN" \
    "$binary" --config "$base_config" relay >"$relay_log" 2>&1 &
  echo $! >"$relay_pid_file"

  ready=''
  count=0
  while [ "$count" -lt 40 ]; do
    if ! pid_is "$relay_pid_file" 'bellhound'; then
      echo 'Bellhound relay exited during startup' >&2
      tail -20 "$relay_log" >&2 || true
      exit 1
    fi
    ready=$(env BELLHOUND_RELAY_TOKEN="$BELLHOUND_RELAY_TOKEN" \
      "$binary" --config "$base_config" wait --after 0 --timeout-seconds 1 2>/dev/null || true)
    if printf '%s' "$ready" | grep -q '"event":"ready"'; then break; fi
    count=$((count + 1))
  done
  printf '%s' "$ready" | grep -q '"event":"ready"' || {
    echo 'Bellhound relay did not become ready' >&2
    exit 1
  }

  cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8787 >"$tunnel_log" 2>&1 &
  echo $! >"$tunnel_pid_file"

  public_url=''
  count=0
  while [ "$count" -lt 120 ]; do
    if ! pid_is "$tunnel_pid_file" 'cloudflared'; then
      echo 'cloudflared exited during startup' >&2
      tail -20 "$tunnel_log" >&2 || true
      exit 1
    fi
    public_url=$(sed -nE 's/.*(https:\/\/[a-z0-9-]+\.trycloudflare\.com).*/\1/p' "$tunnel_log" | head -1 || true)
    [ -n "$public_url" ] && break
    sleep 0.25
    count=$((count + 1))
  done
  [ -n "$public_url" ] || { echo 'temporary tunnel URL was not produced' >&2; exit 1; }

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
}, separators=(",", ":")), flush=True)
PY
  log_owner 'runtime ready'

  started_at=$(date +%s)
  while :; do
    if ! pid_is "$relay_pid_file" 'bellhound'; then
      log_owner 'relay exited unexpectedly'
      echo 'Bellhound relay exited unexpectedly' >&2
      exit 1
    fi
    if ! pid_is "$tunnel_pid_file" 'cloudflared'; then
      log_owner 'tunnel exited unexpectedly'
      echo 'cloudflared exited unexpectedly' >&2
      exit 1
    fi
    now=$(date +%s)
    if [ "$max_seconds" -ne 0 ] && [ $((now - started_at)) -ge "$max_seconds" ]; then
      log_owner "owner lifetime reached ${max_seconds}s"
      exit 0
    fi
    sleep 1
  done
}

stop() {
  hangup_if_active
  if pid_is "$owner_pid_file" 'session.sh run'; then
    owner_pid=$(cat "$owner_pid_file")
    kill "$owner_pid"
    count=0
    while kill -0 "$owner_pid" 2>/dev/null && [ "$count" -lt 50 ]; do
      sleep 0.1
      count=$((count + 1))
    done
    if pid_is "$owner_pid_file" 'session.sh run'; then
      kill -KILL "$owner_pid"
    fi
  fi
  cleanup_children
  rm -f "$owner_pid_file"
  python3 - <<'PY'
import json
print(json.dumps({"state":"stopped"}, separators=(",", ":")))
PY
}

case "${1:-}" in
  run) run ;;
  status) json_status ;;
  stop) stop ;;
  start) echo 'start was replaced by foreground run; use: session.sh run' >&2; exit 2 ;;
  *) echo 'usage: session.sh {run|status|stop}' >&2; exit 2 ;;
esac
