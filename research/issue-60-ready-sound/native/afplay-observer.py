#!/usr/bin/env python3
"""Send fixture callbacks to a managed terminal and record real afplay processes.

Usage: afplay-observer.py BINARY ROOT TERMINAL_ID TURN OUTPUT.json [--no-sound-expected] [--stop-only]
Records afplay and osascript processes; --stop-only repeats a Stop for an already closed turn.
The observer only reads the process table; it never spawns or replaces any host tool.
"""
import json, subprocess, sys, threading, time

binary, root, tid, turn, output = sys.argv[1:6]
expect_none = "--no-sound-expected" in sys.argv
stop_only = "--stop-only" in sys.argv
env = {"OVRCR_CONFIG": f"{root}/config.toml", "OVRCR_SOCKET": f"{root}/server.sock", "PATH": "/usr/bin:/bin"}
seen = {}
stop = threading.Event()

def poll():
    while not stop.is_set():
        ps = subprocess.run(["ps", "-axo", "pid,pgid,ppid,command"], capture_output=True, text=True).stdout
        now = time.time()
        for line in ps.splitlines():
            if ("afplay" in line or "osascript" in line) and "afplay-observer" not in line:
                pid = line.split()[0]
                entry = seen.setdefault(pid, {"first_seen": now, "line": line.strip()})
                entry["last_seen"] = now
        time.sleep(0.02)

def cli(*args):
    return subprocess.run([binary, *args], env=env, capture_output=True, text=True, timeout=15)

def read():
    return json.loads(cli("terminal", "read", tid, "--json").stdout)

def wait_marker(marker, seconds=15):
    deadline = time.time() + seconds
    while time.time() < deadline:
        text = read()
        if marker in json.dumps(text):
            return text
        time.sleep(0.1)
    raise SystemExit(f"marker {marker} missing: {read()}")

def summary():
    rows = json.loads(cli("terminal", "list", "--json").stdout)
    rows = rows if isinstance(rows, list) else rows.get("terminals", rows)
    return next(t for t in rows if str(t["id"]) == tid)

thread = threading.Thread(target=poll, daemon=True)
thread.start()
before = summary()
started = time.time()
callbacks = json.dumps(read()).count("_OK ")
if not stop_only:
    cli("terminal", "send", tid, "--text", f"UserPromptSubmit:root:{turn}")
    wait_marker(f"OK UserPromptSubmit {turn}")
    callbacks += 1
busy = summary()
stop_sent = time.time()
cli("terminal", "send", tid, "--text", f"Stop:root:{turn}")
wait_marker(f"ISSUE60_CALLBACK_{callbacks + 1}_OK Stop {turn}")
ready = summary()
# Give the dashboard host queue time to start and finish the player.
time.sleep(8)
stop.set(); thread.join()
players = [{"pid": pid, "tool": "osascript" if "osascript" in e["line"] else "afplay", "started_after_stop_s": round(e["first_seen"] - stop_sent, 3), "observed_for_s": round(e["last_seen"] - e["first_seen"], 3), "ps": e["line"]} for pid, e in seen.items()]
record = {"turn": turn, "terminal_id": int(tid), "sent_unix": {"prompt": started, "stop": stop_sent},
          "before": before["agent"], "busy": busy["agent"], "ready": ready["agent"],
          "ready_activity": ready["activity"], "host_processes": players, "expected_none": expect_none, "stop_only": stop_only}
json.dump(record, open(output, "w"), indent=2)
print(json.dumps({"ready": ready["activity"], "revision": ready["agent"]["activity_revision"], "turn": ready["agent"]["activity"]["turn"], "hosts": players}, indent=2))
