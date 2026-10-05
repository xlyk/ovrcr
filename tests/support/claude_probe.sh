#!/bin/sh
STATE='@STATE@'
if [ "${1-}" = auth ]; then exec cat "$STATE"; fi
if [ "${1-}" = --version ]; then printf '2.1.285 (Claude Code)\n'; exit 0; fi
mode="${OVRCR_CLAUDE_FIXTURE_MODE:-callback}"
entries=$(ls -A | wc -l | tr -d ' ')
settings=""
prev=""
for arg in "$@"; do
  if [ "$prev" = --settings ]; then settings=$(cat "$arg"); fi
  prev=$arg
done
{
  printf 'PROBE %s\n' "$mode"
  printf 'pid=%s\n' "$$"
  pwd -P > "$OVRCR_CLAUDE_FIXTURE_LOG.cwd"
  printf 'cwd=%s\n' "$(pwd -P)"
  printf 'entries=%s\n' "$entries"
  printf 'argv=%s\n' "$*"
  printf 'arg=%s\n' "$@"
  printf 'settings=%s\n' "$settings"
} >> "$OVRCR_CLAUDE_FIXTURE_LOG"
case "$mode" in
  silent)
    printf '%s\n' "$$" > "$OVRCR_CLAUDE_FIXTURE_PID"
    exec sleep 180
    ;;
  exit) exit 0 ;;
  trust)
    if [ -f "$OVRCR_CLAUDE_FIXTURE_TRUST" ]; then
      printf 'NODIALOG\n' >> "$OVRCR_CLAUDE_FIXTURE_LOG"
    else
      printf 'DIALOG\n' >> "$OVRCR_CLAUDE_FIXTURE_LOG"
      printf 'Accessing workspace:\n%s\nYes, I trust this folder\nNo, exit\n' "$(pwd -P)"
      python3 -c 'import sys
d=b""
while True:
    b=sys.stdin.buffer.read(1)
    if not b:
        break
    d+=b
    if b"\x1b[B" in d and (b"\r" in d or b"\n" in d):
        break
assert b"\x1b[B" in d and (b"\r" in d or b"\n" in d), d'
      printf 'trusted\n' > "$OVRCR_CLAUDE_FIXTURE_TRUST"
      printf 'ANSWERED\n' >> "$OVRCR_CLAUDE_FIXTURE_LOG"
    fi
    ;;
esac
export OVRCR_PROBE_RATE=1
case "$mode" in no-limits|no-limits-hang) export OVRCR_PROBE_RATE=0;; esac
exec python3 - "$@" << 'PY'
import json, os, subprocess, sys, time
args = sys.argv[1:]
doc = json.load(open(args[args.index("--settings") + 1]))
now = int(time.time())
payload = {"session_id": "probe-fixture"}
if os.environ.get("OVRCR_PROBE_RATE") == "1":
    payload["rate_limits"] = {
        "five_hour": {"used_percentage": 12, "resets_at": now + 3600},
        "seven_day": {"used_percentage": 3, "resets_at": now + 86400},
    }
command = doc["statusLine"]["command"]
if os.environ.get("OVRCR_CLAUDE_FIXTURE_MODE") == "cold-start":
    for startup in [
        {}, {"rate_limits": None}, {"rate_limits": {}},
        {"rate_limits": {"five_hour": {}}},
        {"rate_limits": {"seven_day": {"used_percentage": None}}},
    ]:
        startup["session_id"] = "probe-fixture"
        subprocess.run(["sh", "-c", command], input=json.dumps(startup).encode(), check=True)
    with open(os.environ["OVRCR_CLAUDE_FIXTURE_LOG"], "a") as log:
        log.write("COLD_CALLBACK_ACKNOWLEDGED\n")
subprocess.run(["sh", "-c", command], input=json.dumps(payload).encode(), check=True)
if os.environ.get("OVRCR_CLAUDE_FIXTURE_MODE") == "no-limits-hang":
    with open(os.environ["OVRCR_CLAUDE_FIXTURE_PID"], "w") as pid:
        pid.write(str(os.getpid()))
    time.sleep(180)
PY
