#!/usr/bin/env python3
"""Build exclusive review shards for the 3171-row test-value review.

Reads inventory.csv (and optional inventory_rust_test_level.csv), classifies
every row into one workstream by FIRST-MATCH on source_file, groups rows by
(source_file, source_start, source_end), greedily packs groups into shards
(<=80 groups and <=8000 distinct body lines per shard), writes one shard JSON
per shard plus assignments.json, and prints the gate table.

Deterministic and idempotent: rerunning regenerates identical files.
"""

import csv
import json
import os
import sys
from collections import Counter, OrderedDict, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
INVENTORY = os.path.join(HERE, "inventory.csv")
TEST_LEVELS = os.path.join(HERE, "inventory_rust_test_level.csv")
SHARD_DIR = os.path.join(HERE, "shards")
ASSIGNMENTS = os.path.join(HERE, "assignments.json")

MAX_GROUPS = 80
MAX_BODY_LINES = 8000
EXPECTED_ROWS = 3171

WORKSTREAMS = [
    "shared-fixtures",
    "protocol-terminal",
    "runtime-delegation",
    "runtime-server-session",
    "runtime-storage-settings-tasks",
    "tui-library",
    "app-report",
    "app-operator",
    "app-gui",
    "app-cli",
    "app-remote",
    "app-core",
    "public-delegation",
    "lifecycle-terminal-acceptance",
    "public-dashboard",
    "other-rust-integration",
    "extensions-installed-pi",
    "packaging-installers",
]

APP_AREAS = ("report", "operator", "gui", "cli", "remote")

SHARD_ROW_KEYS = [
    "test_id",
    "runner",
    "ci_step",
    "suite",
    "target_kind",
    "test_name",
    "variant",
    "source_file",
    "source_start",
    "source_end",
    "test_level",
    "parent_test_id",
    "first_attempt_seconds",
    "total_attempt_seconds",
    "attempt_count",
    "failed_attempt_count",
    "timing_basis",
    "timing_source",
]


def classify(source_file, runner):
    """FIRST-MATCH workstream classification (rule order is normative)."""
    base = os.path.basename(source_file)

    if source_file.startswith("tests/support/"):
        return "shared-fixtures"
    if source_file.startswith("crates/ovrcr-protocol/") or source_file.startswith(
        "crates/ovrcr-terminal/"
    ):
        return "protocol-terminal"
    if source_file.startswith("crates/ovrcr-runtime/"):
        if "/src/server/delegation" in source_file or base.startswith("delegation_"):
            return "runtime-delegation"
        if "/src/server/" in source_file or "/src/session/" in source_file:
            return "runtime-server-session"
        return "runtime-storage-settings-tasks"
    if source_file.startswith("crates/ovrcr-tui/"):
        return "tui-library"
    if source_file.startswith("src/"):
        for area in APP_AREAS:
            if source_file.startswith("src/" + area):
                return "app-" + area
        return "app-core"
    if source_file == "tests/pi_delegation.rs" or source_file.startswith(
        "tests/pi_delegation/"
    ):
        return "public-delegation"
    if source_file in (
        "tests/server_lifecycle.rs",
        "tests/terminal_acceptance.rs",
    ) or source_file.startswith("tests/terminal_acceptance/"):
        return "lifecycle-terminal-acceptance"
    if source_file.startswith("tests/tui/"):
        return "public-dashboard"
    if runner in ("nextest", "cargo-test"):
        return "other-rust-integration"
    if runner == "node-test":
        return "extensions-installed-pi"
    if runner in ("python-unittest", "python-check"):
        return "packaging-installers"
    raise SystemExit("unclassified row: %r (%r)" % (source_file, runner))


def load_rows():
    with open(INVENTORY, newline="") as fh:
        rows = list(csv.DictReader(fh))
    levels = {}
    if os.path.exists(TEST_LEVELS):
        with open(TEST_LEVELS, newline="") as fh:
            for rec in csv.DictReader(fh):
                levels[rec["test_id"]] = rec["test_level"]
    for i, row in enumerate(rows):
        row["test_level"] = levels.get(row["test_id"], "")
        row["_index"] = i
    return rows


def build_shards(rows):
    by_ws = defaultdict(list)
    for row in rows:
        row["_workstream"] = classify(row["source_file"], row["runner"])
        by_ws[row["_workstream"]].append(row)

    shards = []
    for ws in WORKSTREAMS:
        ws_rows = by_ws.get(ws, [])
        groups = OrderedDict()
        for row in ws_rows:
            key = (row["source_file"], int(row["source_start"]), int(row["source_end"]))
            groups.setdefault(key, []).append(row)
        ordered = sorted(groups)

        packed = []
        current = []
        current_lines = 0
        for key in ordered:
            body_lines = key[2] - key[1] + 1
            if current and (
                len(current) + 1 > MAX_GROUPS
                or current_lines + body_lines > MAX_BODY_LINES
            ):
                packed.append((current, current_lines))
                current = []
                current_lines = 0
            current.append(key)
            current_lines += body_lines
        if current:
            packed.append((current, current_lines))

        for number, (keys, body_lines) in enumerate(packed, start=1):
            shard_rows = []
            for key in keys:
                shard_rows.extend(sorted(groups[key], key=lambda r: r["_index"]))
            shards.append(
                {
                    "shard_id": "%s-%03d" % (ws, number),
                    "workstream": ws,
                    "group_count": len(keys),
                    "body_lines": body_lines,
                    "rows": shard_rows,
                }
            )
    return shards


def write_outputs(shards):
    os.makedirs(SHARD_DIR, exist_ok=True)
    stale = set()
    for name in os.listdir(SHARD_DIR):
        if name.endswith(".json"):
            stale.add(name)

    assignments = []
    for shard in shards:
        rows = shard["rows"]
        payload = OrderedDict()
        payload["shard_id"] = shard["shard_id"]
        payload["workstream"] = shard["workstream"]
        payload["row_count"] = len(rows)
        payload["group_count"] = shard["group_count"]
        payload["body_lines"] = shard["body_lines"]
        payload["rows"] = [
            OrderedDict((key, row[key]) for key in SHARD_ROW_KEYS) for row in rows
        ]
        name = shard["shard_id"] + ".json"
        stale.discard(name)
        with open(os.path.join(SHARD_DIR, name), "w") as fh:
            json.dump(payload, fh, indent=1)
            fh.write("\n")
        assignments.append(
            OrderedDict(
                [
                    ("shard_id", shard["shard_id"]),
                    ("workstream", shard["workstream"]),
                    ("test_ids", [row["test_id"] for row in rows]),
                ]
            )
        )

    for name in sorted(stale):
        os.remove(os.path.join(SHARD_DIR, name))

    with open(ASSIGNMENTS, "w") as fh:
        json.dump(assignments, fh, indent=1)
        fh.write("\n")


def readback_shards(shards):
    """Reload written files so gates validate the artifacts, not memory."""
    loaded = []
    for shard in shards:
        path = os.path.join(SHARD_DIR, shard["shard_id"] + ".json")
        with open(path) as fh:
            loaded.append(json.load(fh))
    with open(ASSIGNMENTS) as fh:
        assignments = json.load(fh)
    return loaded, assignments


def gates(rows, shards, loaded, assignments):
    inventory = OrderedDict((row["test_id"], row) for row in rows)
    results = []

    def check(name, ok, detail):
        results.append((name, bool(ok), detail))

    seen = Counter()
    ws_of = {}
    for shard in loaded:
        for row in shard["rows"]:
            seen[row["test_id"]] += 1
            ws_of[row["test_id"]] = shard["workstream"]

    dupes = [tid for tid, count in seen.items() if count > 1]
    missing = [tid for tid in inventory if tid not in seen]
    extra = [tid for tid in seen if tid not in inventory]
    check(
        "every test_id exactly once",
        not dupes and not missing and not extra and len(seen) == EXPECTED_ROWS,
        "distinct=%d dupes=%d missing=%d extra=%d"
        % (len(seen), len(dupes), len(missing), len(extra)),
    )

    total_rows = sum(shard["row_count"] for shard in loaded)
    check(
        "sum(row_count)==%d" % EXPECTED_ROWS,
        total_rows == EXPECTED_ROWS,
        "sum=%d" % total_rows,
    )

    bad_groups = [
        shard["shard_id"]
        for shard in loaded
        if shard["group_count"] > MAX_GROUPS
        and not (shard["group_count"] == 1 and shard["body_lines"] > MAX_BODY_LINES)
    ]
    check("group_count<=%d" % MAX_GROUPS, not bad_groups, "violations=%r" % bad_groups)

    bad_lines = [
        shard["shard_id"]
        for shard in loaded
        if shard["body_lines"] > MAX_BODY_LINES and shard["group_count"] != 1
    ]
    oversized = [
        shard["shard_id"]
        for shard in loaded
        if shard["body_lines"] > MAX_BODY_LINES and shard["group_count"] == 1
    ]
    check(
        "body_lines<=%d" % MAX_BODY_LINES,
        not bad_lines,
        "violations=%r single-oversized=%r" % (bad_lines, oversized),
    )

    mismatched = []
    for shard in loaded:
        for row in shard["rows"]:
            source = inventory.get(row["test_id"])
            if source is None:
                mismatched.append((row["test_id"], "absent"))
                continue
            for field in (
                "source_file",
                "source_start",
                "source_end",
                "runner",
                "ci_step",
                "suite",
                "target_kind",
                "test_name",
                "variant",
                "parent_test_id",
                "first_attempt_seconds",
                "total_attempt_seconds",
                "attempt_count",
                "failed_attempt_count",
                "timing_basis",
                "timing_source",
            ):
                if row[field] != source[field]:
                    mismatched.append((row["test_id"], field))
            if row["test_level"] != source["test_level"]:
                mismatched.append((row["test_id"], "test_level"))
    check(
        "row fields match inventory",
        not mismatched,
        "mismatches=%d %r" % (len(mismatched), mismatched[:5]),
    )

    mixed = []
    for shard in loaded:
        workstreams = {inventory[row["test_id"]]["_workstream"] for row in shard["rows"]}
        if len(workstreams) != 1 or workstreams != {shard["workstream"]}:
            mixed.append((shard["shard_id"], sorted(workstreams)))
    check("no shard mixes workstreams", not mixed, "violations=%r" % mixed)

    live = [
        row["test_id"]
        for row in rows
        if row["source_file"] == "tests/support/live.rs" and row["source_start"] == "568"
    ]
    live_ws = {ws_of.get(tid) for tid in live}
    live_shards = {
        shard["shard_id"]
        for shard in loaded
        for row in shard["rows"]
        if row["test_id"] in set(live)
    }
    check(
        "28 live.rs:568 rows in one shard",
        len(live) == 28 and len(live_shards) == 1,
        "rows=%d shards=%r" % (len(live), sorted(live_shards)),
    )

    cargo = [row["test_id"] for row in rows if row["runner"] == "cargo-test"]
    children = [
        row["test_id"] for row in rows if row["parent_test_id"].strip() != ""
    ]
    parent_placement = {
        tid: next(
            shard["shard_id"]
            for shard in loaded
            for row in shard["rows"]
            if row["test_id"] == tid
        )
        for tid in cargo
    }
    child_placement = {
        tid: next(
            shard["shard_id"]
            for shard in loaded
            for row in shard["rows"]
            if row["test_id"] == tid
        )
        for tid in children
    }
    check(
        "cargo-test parents + node children assigned",
        len(cargo) == 2 and len(children) == 4 and all(parent_placement.values()) and all(child_placement.values()),
        "cargo=%r children=%r" % (parent_placement, child_placement),
    )

    assignment_ok = (
        len(assignments) == len(loaded)
        and all(
            a["shard_id"] == s["shard_id"]
            and a["workstream"] == s["workstream"]
            and a["test_ids"] == [row["test_id"] for row in s["rows"]]
            for a, s in zip(assignments, loaded)
        )
    )
    check(
        "assignments.json mirrors shard rows",
        assignment_ok,
        "shards=%d assignments=%d" % (len(loaded), len(assignments)),
    )
    return results, parent_placement, child_placement


def main():
    rows = load_rows()
    if len(rows) != EXPECTED_ROWS:
        raise SystemExit("inventory has %d rows, expected %d" % (len(rows), EXPECTED_ROWS))
    shards = build_shards(rows)
    write_outputs(shards)
    loaded, assignments = readback_shards(shards)
    results, parents, children = gates(rows, shards, loaded, assignments)

    print("GATES")
    failed = 0
    for name, ok, detail in results:
        print("%-6s %-42s %s" % ("PASS" if ok else "FAIL", name, detail))
        failed += 0 if ok else 1

    by_ws = defaultdict(list)
    for shard in shards:
        by_ws[shard["workstream"]].append(shard)

    print()
    print("SHARDS")
    print("%-34s %6s %7s %10s" % ("shard_id", "rows", "groups", "body_lines"))
    for shard in shards:
        print(
            "%-34s %6d %7d %10d"
            % (
                shard["shard_id"],
                len(shard["rows"]),
                shard["group_count"],
                shard["body_lines"],
            )
        )

    print()
    print("PER WORKSTREAM")
    for ws in WORKSTREAMS:
        group = by_ws.get(ws, [])
        shard_rows = sum(len(s["rows"]) for s in group)
        print(
            "%-34s shards=%2d rows=%4d groups=%4d body_lines=%6d"
            % (
                ws,
                len(group),
                shard_rows,
                sum(s["group_count"] for s in group),
                sum(s["body_lines"] for s in group),
            )
        )

    largest_rows = max(shards, key=lambda s: len(s["rows"]))
    largest_lines = max(shards, key=lambda s: s["body_lines"])
    smallest = min(shards, key=lambda s: len(s["rows"]))
    print()
    print(
        "largest by rows: %s (%d rows, %d groups, %d lines)"
        % (
            largest_rows["shard_id"],
            len(largest_rows["rows"]),
            largest_rows["group_count"],
            largest_rows["body_lines"],
        )
    )
    print(
        "largest by body_lines: %s (%d rows, %d groups, %d lines)"
        % (
            largest_lines["shard_id"],
            len(largest_lines["rows"]),
            largest_lines["group_count"],
            largest_lines["body_lines"],
        )
    )
    print(
        "minimum shard size: %s (%d rows)"
        % (smallest["shard_id"], len(smallest["rows"]))
    )
    print("total shards: %d" % len(shards))
    print()
    print("PARENT/CHILD PLACEMENT")
    for tid, shard_id in parents.items():
        print("cargo-test parent  %-40s -> %s" % (tid, shard_id))
    for tid, shard_id in children.items():
        print("node child         %-40s -> %s" % (tid, shard_id))

    if failed:
        print("\n%d GATE(S) FAILED" % failed, file=sys.stderr)
        return 1
    print("\nALL GATES PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
