#!/bin/bash
# Run one drive.py step only after every other drive.py has been idle for 15 s,
# so this harness and a parallel worktree's never type into each other's window.
D=$(dirname "$0"); F=$DOC; name=$1; shift
for _ in $(seq 1 120); do
  others=$(pgrep -f "drive.py" | grep -v "^$$\$" | while read p; do ps -o command= -p $p | grep -v "settings-editor-2026-10-02" ; done)
  if [ -z "$others" ]; then quiet=$((quiet+1)); else quiet=0; fi
  [ "${quiet:-0}" -ge 3 ] && break; sleep 5
done
python3 "$D/drive.py" "$@" >/dev/null || exit 1
sleep 1.5; python3 "$D/drive.py" "shot:$name" >/dev/null; cp "$F" "$D/$name-dashboard.toml.txt"; echo "== $name"
