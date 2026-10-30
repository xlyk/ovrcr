"""Verify saved native receipts; never opens the GUI or a live fixture."""
import gzip
import hashlib
import json
import pathlib
import re

evidence = pathlib.Path(__file__).resolve().parent / "native-02"


def receipt(name):
    return json.loads((evidence / (name + ".json")).read_text())


def ax(name):
    return gzip.decompress((evidence / (name + ".ax.txt.gz")).read_bytes()).decode()


for phase in ("silent", "pending"):
    before, after = (receipt(phase + suffix + "-boundary") for suffix in ("-before", "-after"))
    for field in ("operator_conversations", "operator_turns", "operator_messages", "control_markers", "frozen_sha256"):
        assert before[field] == after[field], (phase, field)
    assert len(before["operator_messages"]) == (1 if phase == "silent" else 3)

for first, second, label in (("01-silent-collapsed-a", "02-silent-collapsed-b", "Responding"), ("03-pending-collapsed-a", "04-pending-collapsed-b", "Waiting for bash result")):
    glyphs = [re.search(r"([\u2800-\u28ff]) " + label, ax(name)).group(1) for name in (first, second)]
    assert glyphs[0] != glyphs[1], (label, glyphs)
    assert receipt(first)["end_unix_ms"] < receipt(second)["start_unix_ms"]

for name in ("05-history-away", "06-history-still-away", "08-history-before-progress", "09-history-after-progress"):
    assert "Waiting for bash result" not in ax(name)
assert receipt("05-history-away")["jpg_sha256"] == receipt("06-history-still-away")["jpg_sha256"]
history = lambda name: [line.split("text (settable) ", 1)[1] for line in ax(name).splitlines() if re.search(r"text \(settable\)\s+History \d\d ·", line)]
assert len(history("08-history-before-progress")) == 21
assert history("08-history-before-progress") == history("09-history-after-progress")

completed = ax("10-completed-tail")
assert "ACTIVITY final" in completed and "Enter Send" in completed
assert "Responding" not in completed and "Waiting for bash result" not in completed
assert "Retained follow-up draft 界🙂; do not send." in completed
for name in ("12-full-edit-after", "19-quick-edit-after"):
    assert "ond café" in ax(name) and "alpha 界🙂 omega" in ax(name)
for name in ("13-full-grapheme-delete", "20-quick-grapheme-delete"):
    assert "alpha " in ax(name) and " omega" in ax(name)
    assert "alpha 界🙂 omega" not in ax(name)

states = lambda phase: {row["conversation"]: row["state"] for row in receipt(phase + "-boundary")["operator_turns"]}
assert states("clear-only") == {1: "Complete", 2: "Running", 3: "Running"}
assert states("quick-empty") == {1: "Complete", 2: "Interrupted", 3: "Running"}
assert states("final") == {1: "Complete", 2: "Interrupted", 3: "Complete"}
assert len(receipt("final-boundary")["operator_conversations"]) == 3
assert receipt("provider-finished") == {"seeded": True, "a_requests": 2, "b_requests": 2, "quick_requests": 0, "activity_requests": 2}
before = receipt("terminal-before-interrupt-processes")
after = receipt("terminal-after-interrupt-processes")
assert any(row["command"] == "/bin/sleep 30" for row in before)
assert not any("sleep 30" in row["command"] for row in after)
assert any(row["pid"] == 43232 for row in after)
assert "CUA_TERMINAL_SURVIVED" in ax("25-terminal-ctrlc-survived")
assert receipt("25-terminal-ctrlc-survived")["end_unix_ms"] - receipt("terminal-before-interrupt-boundary")["unix_ms"] < 30000

captures = list(evidence.glob("*.jpg"))
for path in captures:
    name = path.stem
    image = path.read_bytes()
    text = gzip.decompress((evidence / (name + ".ax.txt.gz")).read_bytes())
    assert image[:3] == b"\xff\xd8\xff"
    assert hashlib.sha256(image).hexdigest() == receipt(name)["jpg_sha256"]
    assert hashlib.sha256(text).hexdigest() == receipt(name)["ax_sha256"]
for owner in ("holder", "gui"):
    assert receipt(owner + "-exit")["original_wait_exit"] == 0
cleanup = receipt("cleanup")
assert all(value == "absent" for field in ("pids", "pgids") for value in cleanup[field].values())
assert cleanup["hashes_unchanged"] and cleanup["root_removed"]
assert not cleanup["demo_exists"] and not cleanup["socket_exists"]
print(json.dumps({"captures": len(captures), "pids_absent": len(cleanup["pids"]), "groups_absent": len(cleanup["pgids"]), "result": "passed"}, indent=2))
