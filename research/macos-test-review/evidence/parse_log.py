#!/usr/bin/env python3
"""Parse the hosted macOS job log into a terminal-attempt ledger.

Only the MAIN nextest invocation (workflow step "Test") is parsed.  PASS/FAIL
attempt lines are terminal attempt records; `SLOW` progress lines and the
trailing repeated `FLAKY n/m` summary block are excluded, as are doctest and
explicit single-test cargo test steps.

Emits attempts.csv with columns:
  invocation,binary_id,test_name,attempt,result,seconds,raw_line
and prints the reconciliation gates.
"""

import csv
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOG = HERE / "macos-job.log"
OUT = HERE / "attempts.csv"

INVOCATION = "Test"

# Job-log line shape: "<job name>\t<step name>\t<ISO timestamp> <message>".
LOGLINE = re.compile(r"^[^\t]*\t[^\t]*\t(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+Z) ?(.*)$")
# Real ANSI escapes and literal displayed `^[[...m` escape text.
ESCAPES = re.compile(r"(?:\x1b|\^\[)\[[0-9;]*[A-Za-z]")

# "        PASS [   0.021s] (   1/2820) ovrcr gui::input::tests::name"
# "  TRY 1 FAIL [   4.855s] (─────────) ovrcr::codex_initial_prompt name"
# "  TRY 2 PASS [   4.372s] ( 343/2820) ovrcr::codex_initial_prompt name"
ATTEMPT = re.compile(
    r"^\s*(?:TRY\s+(?P<try>\d+)\s+)?(?P<result>PASS|FAIL)\s+"
    r"\[\s*(?P<seconds>\d+\.\d+)s\]\s+"
    r"\([^)]*\)\s+"
    r"(?P<binary>\S+)\s+(?P<test>\S+)\s*$"
)
# "   FLAKY 2/3 [   4.372s] ( 343/2820) binary test"
FLAKY = re.compile(r"^\s*FLAKY\s+\d+/\d+\s+\[")
# "   SLOW [> 60.000s] binary test"
SLOW = re.compile(r"^\s*SLOW\s+\[")


def split_message(line: str):
    m = LOGLINE.match(line)
    if not m:
        return None, line
    return m.group(1), ESCAPES.sub("", m.group(2))


def main() -> int:
    text = LOG.read_text(encoding="utf-8-sig", errors="replace")
    lines = text.splitlines()

    # Main nextest invocation: from its `##[group]Run cargo nextest` marker to
    # the next `##[group]Run ...` marker (the Doc tests step).
    start = None
    end = None
    for i, line in enumerate(lines):
        _, msg = split_message(line)
        if start is None and msg.startswith("##[group]Run cargo nextest run"):
            start = i
            continue
        if start is not None and msg.startswith("##[group]Run "):
            end = i
            break
    if start is None:
        print("FATAL: main nextest invocation group not found", file=sys.stderr)
        return 2
    if end is None:
        end = len(lines)

    rows = []
    excluded = {"flaky_summary": 0, "slow_progress": 0, "doctest_or_other": 0}
    for line in lines[start:end]:
        _, msg = split_message(line)
        if FLAKY.match(msg):
            excluded["flaky_summary"] += 1
            continue
        if SLOW.match(msg):
            excluded["slow_progress"] += 1
            continue
        m = ATTEMPT.match(msg)
        if not m:
            continue
        rows.append(
            {
                "invocation": INVOCATION,
                "binary_id": m.group("binary"),
                "test_name": m.group("test"),
                "attempt": int(m.group("try") or 1),
                "result": m.group("result"),
                "seconds": m.group("seconds"),
                "raw_line": line,
            }
        )

    with OUT.open("w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(
            fh,
            fieldnames=[
                "invocation",
                "binary_id",
                "test_name",
                "attempt",
                "result",
                "seconds",
                "raw_line",
            ],
            quoting=csv.QUOTE_MINIMAL,
        )
        writer.writeheader()
        writer.writerows(rows)

    # ---- reconciliation gates ----
    total = len(rows)
    unique = len({(r["binary_id"], r["test_name"]) for r in rows})
    binaries = len({r["binary_id"] for r in rows})
    failed = sum(1 for r in rows if r["result"] == "FAIL")
    seconds = round(sum(float(r["seconds"]) for r in rows), 3)

    def retry_row(name):
        picked = [r for r in rows if r["test_name"] == name]
        first = float(picked[0]["seconds"])
        tot = sum(float(r["seconds"]) for r in picked)
        return len(picked), first, tot, sum(1 for r in picked if r["result"] == "FAIL")

    gates = [
        ("attempt records", total, 2822, total == 2822),
        ("unique (binary_id,test_name)", unique, 2820, unique == 2820),
        ("binary IDs", binaries, 56, binaries == 56),
        ("failed attempts", failed, 2, failed == 2),
        ("cumulative seconds", seconds, 5931.968, abs(seconds - 5931.968) <= 0.001),
    ]
    codex = retry_row(
        "codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path"
    )
    pi = retry_row(
        "parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects"
    )
    gates.append(
        (
            "codex retry (attempts,first,total,failed)",
            codex,
            (2, 4.855, 9.227, 1),
            codex[0] == 2 and abs(codex[1] - 4.855) < 1e-9 and abs(codex[2] - 9.227) <= 0.001 and codex[3] == 1,
        )
    )
    gates.append(
        (
            "pi retry (attempts,first,total,failed)",
            pi,
            (2, 24.507, 46.250, 1),
            pi[0] == 2 and abs(pi[1] - 24.507) < 1e-9 and abs(pi[2] - 46.250) <= 0.001 and pi[3] == 1,
        )
    )

    print(f"wrote {OUT} ({total} rows)")
    print(f"excluded: {excluded}")
    print()
    print("| gate | observed | expected | result |")
    print("| --- | --- | --- | --- |")
    ok = True
    for label, observed, expected, passed in gates:
        ok = ok and passed
        print(f"| {label} | {observed} | {expected} | {'PASS' if passed else 'FAIL'} |")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
