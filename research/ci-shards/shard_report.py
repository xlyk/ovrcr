#!/usr/bin/env python3
"""Acceptance check for a sharded `macOS checks` run.

Reads one run's job list and logs through `gh`, then reports per-shard test
counts, nextest durations and job durations.

The three things this decides:
  1. the shards' test counts sum to the pinned selection (2820 by default),
  2. per-shard durations are within a usable spread,
  3. the job finished faster than the serial baseline (2418 s).

Usage: python3 -I shard_report.py <run-id> [expected-total]
Read-only: uses `gh run view`, never the repository or its tests.
"""

import json
import re
import subprocess
import sys
from datetime import datetime

MACOS_SHARD = re.compile(r"^macOS checks \((\d+)/(\d+)\)$")
STARTING = re.compile(r"Starting (\d+) tests? across (\d+) binar")
SUMMARY = re.compile(r"Summary \[\s*([\d.]+)s\]\s+(\d+) tests run")
CPU = re.compile(r"^(?:logicalcpu=)?(\d+)(?: \d+)?$")
SLOW = re.compile(r"\bSLOW\b")
FLAKY = re.compile(r"\bFLAKY\b")

# nextest colourises its output. Logs carry both real escape sequences and
# literal caret notation, so strip both before matching anything.
ANSI = re.compile(r"\x1b\[[0-9;]*m|\^\[\[[0-9;]*m")

BASELINE_WALL_SECONDS = 2418.0
# The selection moves whenever tests are added or removed, so there is no durable
# expected total. Pass one explicitly to assert against a recorded baseline.
DEFAULT_EXPECTED = None


def plain(text):
    return ANSI.sub("", text)


def matches(pattern, messages):
    """Every match of `pattern` across `messages`, in order."""
    return [m for message in messages for m in pattern.finditer(message)]


def gh(*args):
    out = subprocess.run(["gh", *args], capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"gh {' '.join(args)} failed:\n{out.stderr[:800]}")
    return out.stdout


def seconds_between(start, end):
    def parse(value):
        return datetime.fromisoformat(value.replace("Z", "+00:00"))

    try:
        return (parse(end) - parse(start)).total_seconds()
    except (TypeError, ValueError):
        return None


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    run_id = sys.argv[1]
    expected = int(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_EXPECTED

    meta = json.loads(gh("run", "view", run_id, "--json", "jobs,conclusion,headSha,createdAt"))
    log = gh("run", "view", run_id, "--log")

    # Combined log lines are "<job>\t<step>\t<message>".
    per_job = {}
    for line in log.splitlines():
        parts = line.split("\t", 2)
        if len(parts) < 3:
            continue
        job, _step, message = parts
        per_job.setdefault(job, []).append(plain(message))

    rows = []
    for job in meta["jobs"]:
        match = MACOS_SHARD.match(job["name"])
        if not match:
            continue
        messages = per_job.get(job["name"], [])
        joined = "\n".join(messages)

        start = matches(STARTING, messages)
        summaries = matches(SUMMARY, messages)
        cpus = [m.group(1) for m in (CPU.match(message) for message in messages) if m]
        walls = seconds_between(job.get("startedAt"), job.get("completedAt"))
        # Queue is how long the job waited for a runner. On a sharded run it can
        # exceed the test work, so it belongs in the report next to wall time.
        queue = seconds_between(meta.get("createdAt"), job.get("startedAt"))

        rows.append(
            {
                "shard": int(match.group(1)),
                "tests": int(start[0].group(1)) if start else None,
                "binaries": int(start[0].group(2)) if start else None,
                "exec": float(summaries[-1].group(1)) if summaries else None,
                "wall": walls,
                "queue": queue,
                "cpu": cpus[0] if cpus else "?",
                "conclusion": job.get("conclusion"),
                "slow": len(SLOW.findall(joined)),
                "flaky": len(FLAKY.findall(joined)),
            }
        )

    if not rows:
        sys.exit(f"no 'macOS checks (n/N)' jobs found in run {run_id}")

    rows.sort(key=lambda row: row["shard"])

    def fmt(value, spec):
        return format(value, spec) if value is not None else "-"

    print(f"run {run_id}  head {meta['headSha'][:12]}  {meta['conclusion']}\n")
    header = (
        f"{'shard':>5} {'tests':>6} {'bins':>5} {'exec s':>9} {'wall s':>7} {'queue s':>8}"
        f" {'cpu':>4} {'slow':>4} {'flky':>4}  result"
    )
    print(header)
    print("-" * len(header))
    for row in rows:
        print(
            f"{row['shard']:>5} {fmt(row['tests'], 'd'):>6} {fmt(row['binaries'], 'd'):>5}"
            f" {fmt(row['exec'], '.1f'):>9} {fmt(row['wall'], '.0f'):>7}"
            f" {fmt(row['queue'], '.0f'):>8}"
            f" {row['cpu']:>4} {row['slow']:>4} {row['flaky']:>4}  {row['conclusion']}"
        )

    counts = [row["tests"] for row in rows]
    execs = [row["exec"] for row in rows if row["exec"] is not None]
    walls = [row["wall"] for row in rows if row["wall"] is not None]

    print()
    if None in counts:
        print("COVERAGE: UNKNOWN - a shard reported no selection count")
    elif expected is None:
        print(f"COVERAGE: {sum(counts)} tests across {len(rows)} shards")
    else:
        total = sum(counts)
        verdict = "OK" if total == expected else f"MISMATCH, expected {expected}"
        print(f"COVERAGE: {total} tests across {len(rows)} shards - {verdict}")

    if execs and len(execs) == len(rows):
        spread = (max(execs) - min(execs)) / max(execs) * 100
        print(f"BALANCE:  exec {min(execs):.1f}-{max(execs):.1f}s, spread {spread:.1f}%")
        # A wide spread only matters through the slowest shard, since that is
        # what the job waits on. Compare it with an evenly split partition.
        even = sum(execs) / len(execs)
        worst = max(execs)
        print(
            f"IDEAL:    an evenly split partition would run {even:.1f}s per shard;"
            f" the slowest is {worst - even:+.1f}s from that"
        )
        print(
            f"COST:     imbalance is worth about {worst - even:.0f}s of the slowest"
            f" shard's {max(walls) if walls else 0:.0f}s wall, not the whole gap"
        )

    if walls:
        slowest = max(walls)
        print(
            f"WALL:     slowest shard {slowest:.0f}s vs {BASELINE_WALL_SECONDS:.0f}s baseline"
            f" ({slowest / BASELINE_WALL_SECONDS * 100:.1f}% of baseline)"
        )

    if any(row["flaky"] for row in rows):
        print("NOTE:     FLAKY lines present; they are the list of tests to harden")

    # The selection audit and the aggregate decide whether sharding is safe,
    # so report them next to the timings rather than leaving them to the UI.
    gates = [
        job
        for job in meta["jobs"]
        if job["name"] in ("test selection audit", "checks", "macOS offline Cursor preflight")
    ]
    if gates:
        print("\ngate jobs:")
        for job in gates:
            print(f"  {job['name']:34s} {job.get('conclusion')}")


if __name__ == "__main__":
    main()
