"""Read-only receipts from this owned fixture; never query the closed GUI."""
import hashlib, json, pathlib, sqlite3, subprocess, sys, time

evidence = pathlib.Path(__file__).resolve().parent
state = json.loads((evidence / "state.json").read_text())
root = pathlib.Path(state["root"])
phase = sys.argv[1]
destination = evidence / (phase + "-boundary.json")
assert not destination.exists(), "preserve earlier observations"
database = sqlite3.connect("file:" + state["demo"] + "/registry.sqlite3?mode=ro", uri=True)
database.row_factory = sqlite3.Row
data = {"unix_ms": int(time.time() * 1000), "control_markers": sorted(path.name for path in (root / "control").iterdir())}
for table in ("operator_conversations", "operator_turns", "operator_messages"):
    data[table] = [dict(row) for row in database.execute("select * from " + table)]
data["frozen_sha256"] = {name: hashlib.sha256((pathlib.Path(state["bundle"]) / "Contents/MacOS" / name).read_bytes()).hexdigest() for name in ("ovrcr", "ovrcr-gui")}
destination.write_text(json.dumps(data, indent=2) + "\n")
rows = []
for line in subprocess.check_output(["rtk", "proxy", "ps", "-axo", "pid=,ppid=,pgid=,command="], text=True).splitlines():
    fields = line.strip().split(None, 3)
    if len(fields) == 4:
        rows.append(dict(zip(("pid", "ppid", "pgid", "command"), [*map(int, fields[:3]), fields[3]])))
owners = {json.loads((evidence / (name + "-owner.json")).read_text())["pid"] for name in ("gui", "holder")}
while True:
    descendants = owners | {row["pid"] for row in rows if row["ppid"] in owners}
    if descendants == owners:
        break
    owners = descendants
(evidence / (phase + "-processes.json")).write_text(json.dumps([row for row in rows if row["pid"] in owners], indent=2) + "\n")
print(json.dumps({"phase": phase, "turns": data["operator_turns"], "messages": len(data["operator_messages"])}))
