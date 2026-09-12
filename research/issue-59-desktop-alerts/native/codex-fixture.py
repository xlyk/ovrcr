#!/usr/bin/env python3
"""Deterministic provider fixture for native desktop delivery, not Codex certification.

Copy this file to an executable named `codex` in the disposable GUI directory.
Set OVRCR_ACCEPTANCE_BINARY to the reviewed OVRCR binary. Launch through
`ovrcr agent run codex -- /disposable/path/codex` in a managed terminal.
Input is EVENT:CONVERSATION:TURN[:child], or `exit`.
"""

import json
import os
import subprocess
import sys

if sys.argv[1:] == ["--version"]:
    print("codex-cli 0.153.0")
    raise SystemExit(0)

binary = os.environ["OVRCR_ACCEPTANCE_BINARY"]
print(f"ISSUE59_NATIVE_FIXTURE_READY pid={os.getpid()}", flush=True)
for number, line in enumerate(sys.stdin, 1):
    value = line.strip()
    if value == "exit":
        print("ISSUE59_NATIVE_FIXTURE_EXIT", flush=True)
        raise SystemExit(0)
    event, conversation, turn, *extra = value.split(":")
    payload = {
        "hook_event_name": event,
        "session_id": conversation,
        "turn_id": turn,
        "transcript_path": "/unused/issue59-fixture.jsonl",
        "prompt": "ISSUE59_PRIVATE_PROMPT_MUST_NOT_APPEAR_IN_ALERT",
        "last_assistant_message": "ISSUE59_PRIVATE_RESPONSE_MUST_NOT_APPEAR_IN_ALERT",
    }
    if extra == ["child"]:
        payload["agent_id"] = "issue59-child"
    result = subprocess.run(
        [binary, "report", "codex", "--stdin"],
        input=json.dumps(payload).encode(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=5,
        check=True,
    )
    assert result.stdout == b"", "reporter must preserve native no-op output"
    print(f"ISSUE59_CALLBACK_{number}_OK {event} {turn}", flush=True)
