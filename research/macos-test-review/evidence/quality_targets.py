#!/usr/bin/env python3
"""Build the independent quality-review harness inputs.

Reads (never writes):
  evidence/inventory.csv
  evidence/assignments.json
  evidence/shards/*.json
  reviews/*.jsonl

Writes:
  evidence/coverage-audit.json   -> structural audit of the review records
  evidence/quality-targets.json  -> deterministic quality-target manifest

Run with:  python3 -I research/macos-test-review/evidence/quality_targets.py
Standard library only; safe to run concurrently with reviewers (read-only inputs).
"""

from __future__ import annotations

import csv
import hashlib
import json
import math
import sys
from collections import Counter
from pathlib import Path

SCRIPT = Path(__file__).resolve()
EVIDENCE = SCRIPT.parent
ROOT = EVIDENCE.parents[2]  # evidence -> macos-test-review -> research -> repo
REVIEWS = ROOT / "research" / "macos-test-review" / "reviews"
AUDIT_OUT = EVIDENCE / "coverage-audit.json"
TARGETS_OUT = EVIDENCE / "quality-targets.json"

# Exact 19-key output record, per evidence/rubric.md.
RECORD_KEYS = {
    "test_id",
    "test_level",
    "production_refs",
    "behavior",
    "failure_caught",
    "criticality",
    "signal",
    "uniqueness",
    "fidelity",
    "criticality_reason",
    "signal_reason",
    "uniqueness_reason",
    "fidelity_reason",
    "confidence",
    "recommendation",
    "recommendation_reason",
    "overlap_test_ids",
    "cost_drivers",
    "reviewer",
}
SCORE_KEYS = ("criticality", "signal", "uniqueness", "fidelity")
REASON_KEYS = (
    "behavior",
    "failure_caught",
    "criticality_reason",
    "signal_reason",
    "uniqueness_reason",
    "fidelity_reason",
    "recommendation_reason",
)
TEST_LEVELS = {"unit", "integration", "acceptance"}
CONFIDENCES = {"high", "medium", "low"}
RECOMMENDATIONS = {
    "investigate",
    "consolidate",
    "prune_candidate",
    "improve",
    "keep",
}
PLACEHOLDER = None  # compiled below, after importing re lazily

import re  # noqa: E402  (kept next to PLACEHOLDER definition)

PLACEHOLDER = re.compile(r"\b(?:TBD|TODO|FIXME|XXX|N/A)\b", re.IGNORECASE)


# --------------------------------------------------------------------------- #
# Inputs
# --------------------------------------------------------------------------- #


def load_assignments():
    with open(EVIDENCE / "assignments.json", encoding="utf-8") as handle:
        data = json.load(handle)
    assignments = []
    for entry in data:
        shard_id = entry["shard_id"]
        test_ids = list(dict.fromkeys(entry["test_ids"]))
        assignments.append(
            {
                "shard_id": shard_id,
                "workstream": entry.get("workstream", ""),
                "test_ids": test_ids,
            }
        )
    return assignments


def load_inventory():
    rows = {}
    with open(EVIDENCE / "inventory.csv", newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            rows[row["test_id"]] = row
    return rows


def load_shard_file(shard_id):
    path = EVIDENCE / "shards" / f"{shard_id}.json"
    if not path.exists():
        return None
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def load_review(path):
    """Return (records, errors). A malformed line becomes an error record."""
    records = []
    errors = []
    if not path.exists():
        return records, errors
    with open(path, encoding="utf-8") as handle:
        for number, line in enumerate(handle, 1):
            text = line.strip()
            if not text:
                continue
            try:
                record = json.loads(text)
            except json.JSONDecodeError as error:
                errors.append(
                    {
                        "test_id": f"<line {number}>",
                        "errors": [f"invalid json: {error.msg}"],
                    }
                )
                continue
            if not isinstance(record, dict):
                errors.append(
                    {
                        "test_id": f"<line {number}>",
                        "errors": ["record is not a JSON object"],
                    }
                )
                continue
            records.append(record)
    return records, errors


# --------------------------------------------------------------------------- #
# Ref bounds
# --------------------------------------------------------------------------- #

_LINE_COUNT_CACHE = {}


def line_count(relative_path):
    if relative_path in _LINE_COUNT_CACHE:
        return _LINE_COUNT_CACHE[relative_path]
    path = ROOT / relative_path
    if not path.is_file():
        _LINE_COUNT_CACHE[relative_path] = None
        return None
    count = 0
    last_byte = b"\n"
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(1 << 20)
            if not chunk:
                break
            count += chunk.count(b"\n")
            last_byte = chunk[-1:]
    if last_byte != b"\n":
        count += 1
    _LINE_COUNT_CACHE[relative_path] = count
    return count


REF_PATTERN = re.compile(r"^(?P<path>[^\s:]+):(?P<start>\d+)-(?P<end>\d+)$")


def check_refs(refs):
    problems = []
    if not isinstance(refs, list) or not refs:
        return ["production_refs is not a non-empty array"]
    for ref in refs:
        if not isinstance(ref, str):
            problems.append(f"production_refs entry is not a string: {ref!r}")
            continue
        match = REF_PATTERN.match(ref)
        if not match:
            problems.append(f"production_refs entry is not path:start-end: {ref!r}")
            continue
        path = match.group("path")
        start = int(match.group("start"))
        end = int(match.group("end"))
        if start < 1 or end < start:
            problems.append(f"production_refs range is invalid: {ref}")
            continue
        total = line_count(path)
        if total is None:
            problems.append(f"production_refs path does not exist: {ref}")
        elif end > total:
            problems.append(f"production_refs range exceeds {total} lines: {ref}")
    return problems


# --------------------------------------------------------------------------- #
# A. Structural audit
# --------------------------------------------------------------------------- #


def audit_shard(assignment, inventory, review_records, review_load_errors):
    shard_id = assignment["shard_id"]
    expected = assignment["test_ids"]
    expected_set = set(expected)
    seen = [record.get("test_id") for record in review_records]
    counts = Counter(seen)
    duplicate_ids = sorted(tid for tid, count in counts.items() if count > 1)
    present = [tid for tid in seen if tid is not None]
    extra_ids = sorted(set(tid for tid in present if tid not in expected_set))
    missing_ids = sorted(expected_set - set(present))

    schema_errors = []
    for error in review_load_errors:
        schema_errors.append(error)
    for record in review_records:
        test_id = record.get("test_id")
        problems = []
        keys = set(record)
        if keys != RECORD_KEYS:
            missing_keys = sorted(RECORD_KEYS - keys)
            extra_keys = sorted(keys - RECORD_KEYS)
            if missing_keys:
                problems.append(f"missing keys: {', '.join(missing_keys)}")
            if extra_keys:
                problems.append(f"extra keys: {', '.join(extra_keys)}")
        if record.get("reviewer") != shard_id:
            problems.append(f"reviewer is not the shard id: {record.get('reviewer')!r}")
        if record.get("test_level") not in TEST_LEVELS:
            problems.append(f"test_level invalid: {record.get('test_level')!r}")
        for key in SCORE_KEYS:
            value = record.get(key)
            if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= 4:
                problems.append(f"{key} is not an int 0..4: {value!r}")
        if record.get("confidence") not in CONFIDENCES:
            problems.append(f"confidence invalid: {record.get('confidence')!r}")
        if record.get("recommendation") not in RECOMMENDATIONS:
            problems.append(f"recommendation invalid: {record.get('recommendation')!r}")
        for key in REASON_KEYS:
            value = record.get(key)
            if not isinstance(value, str):
                problems.append(f"{key} is not a string")
                continue
            if len(value.strip()) < 12:
                problems.append(f"{key} is shorter than 12 characters")
            if PLACEHOLDER.search(value):
                problems.append(f"{key} contains placeholder text")
        if test_id is not None and test_id not in inventory:
            problems.append("test_id is not in inventory.csv")
        problems.extend(check_refs(record.get("production_refs")))
        overlaps = record.get("overlap_test_ids")
        if not isinstance(overlaps, list):
            problems.append("overlap_test_ids is not an array")
        else:
            for other in overlaps:
                if not isinstance(other, str):
                    problems.append(f"overlap_test_ids entry is not a string: {other!r}")
                elif other not in inventory:
                    problems.append(f"overlap_test_ids id is not in inventory: {other}")
                elif test_id is not None and other == test_id:
                    problems.append("overlap_test_ids contains the row itself")
        if problems:
            schema_errors.append({"test_id": test_id, "errors": problems})

    shard_file = load_shard_file(shard_id)
    shard_file_present = shard_file is not None
    shard_file_rows = len(shard_file["rows"]) if shard_file_present else 0
    shard_file_ids = (
        [row["test_id"] for row in shard_file["rows"]] if shard_file_present else []
    )
    assignment_vs_shard_file = {
        "shard_file_present": shard_file_present,
        "shard_file_rows": shard_file_rows,
        "only_in_assignments": sorted(expected_set - set(shard_file_ids)),
        "only_in_shard_file": sorted(set(shard_file_ids) - expected_set),
    }

    return {
        "shard_id": shard_id,
        "workstream": assignment["workstream"],
        "review_file": f"reviews/{shard_id}.jsonl",
        "review_present": (REVIEWS / f"{shard_id}.jsonl").exists(),
        "records": len(review_records),
        "expected": len(expected),
        "missing_ids": missing_ids,
        "extra_ids": extra_ids,
        "duplicate_ids": duplicate_ids,
        "schema_errors": schema_errors,
        "schema_ok": not schema_errors,
        "assignment_vs_shard_file": assignment_vs_shard_file,
    }


def build_audit(assignments, inventory):
    shards = []
    for assignment in sorted(assignments, key=lambda item: item["shard_id"]):
        path = REVIEWS / f"{assignment['shard_id']}.jsonl"
        records, errors = load_review(path)
        shards.append(audit_shard(assignment, inventory, records, errors))

    reviewed = [shard for shard in shards if shard["review_present"]]
    missing = sorted(shard["shard_id"] for shard in shards if not shard["review_present"])
    totals = {
        "expected": sum(shard["expected"] for shard in shards),
        "records": sum(shard["records"] for shard in shards),
        "missing_ids": sum(len(shard["missing_ids"]) for shard in shards),
        "extra_ids": sum(len(shard["extra_ids"]) for shard in shards),
        "duplicate_ids": sum(len(shard["duplicate_ids"]) for shard in shards),
        "schema_error_records": sum(len(shard["schema_errors"]) for shard in shards),
        "reviewed_shards": len(reviewed),
        "shards_missing_reviews": len(missing),
        "shards_total": len(shards),
    }
    return {
        "status": "complete" if not missing else "incomplete",
        "generated_by": "evidence/quality_targets.py",
        "inventory_rows": len(inventory),
        "totals": totals,
        "shards_without_review_file": missing,
        "shards": shards,
    }


# --------------------------------------------------------------------------- #
# B. Quality-target manifest
# --------------------------------------------------------------------------- #


def ceil_ten_percent(n):
    """ceil(0.10 * n) in exact integer arithmetic (0.1 * 80 is 8.0000000000000004)."""
    return (n * 10 + 99) // 100


def hash_sample(test_ids, k):
    ordered = sorted(
        test_ids, key=lambda tid: hashlib.sha256(tid.encode("utf-8")).hexdigest()
    )
    return ordered[:k]


def build_review_index(assignments):
    """test_id -> (record, shard_id). Later shards cannot duplicate ids."""
    index = {}
    for assignment in assignments:
        shard_id = assignment["shard_id"]
        records, _ = load_review(REVIEWS / f"{shard_id}.jsonl")
        for record in records:
            test_id = record.get("test_id")
            if isinstance(test_id, str) and test_id not in index:
                index[test_id] = (record, shard_id)
    return index


def target(record, shard_id, reason_kind, **extra):
    reviewer = record.get("reviewer") if record else None
    item = {
        "test_id": record["test_id"] if record else extra.pop("test_id"),
        "reviewer": reviewer,
        "shard_id": shard_id,
        "reason_kind": reason_kind,
        "must_differ_from": reviewer,
    }
    for key, value in extra.items():
        item[key] = value
    return item


def build_targets(assignments, inventory, audit):
    index = build_review_index(assignments)
    assigned_shard = {
        test_id: assignment["shard_id"]
        for assignment in assignments
        for test_id in assignment["test_ids"]
    }

    def all_records():
        for assignment in sorted(assignments, key=lambda item: item["shard_id"]):
            records, _ = load_review(REVIEWS / f"{assignment['shard_id']}.jsonl")
            for record in records:
                yield record, assignment["shard_id"]

    prune_consolidate = []
    criticality4 = []
    low_confidence = []
    for record, shard_id in all_records():
        recommendation = record.get("recommendation")
        if recommendation in ("prune_candidate", "consolidate"):
            prune_consolidate.append(
                target(
                    record,
                    shard_id,
                    recommendation,
                    recommendation=recommendation,
                    criticality=record.get("criticality"),
                )
            )
        if record.get("criticality") == 4:
            criticality4.append(
                target(
                    record,
                    shard_id,
                    "criticality4",
                    signal=record.get("signal"),
                    fidelity=record.get("fidelity"),
                )
            )
        if record.get("confidence") == "low":
            low_confidence.append(
                target(record, shard_id, "low_confidence")
            )

    prune_consolidate.sort(key=lambda item: (item["shard_id"], item["test_id"]))
    criticality4.sort(key=lambda item: (item["shard_id"], item["test_id"]))
    low_confidence.sort(key=lambda item: (item["shard_id"], item["test_id"]))

    def seconds(row):
        try:
            return float(row.get("total_attempt_seconds") or 0.0)
        except ValueError:
            return 0.0

    ranked = sorted(
        inventory.items(), key=lambda item: (-seconds(item[1]), item[0])
    )[:30]
    slowest30 = []
    for rank, (test_id, row) in enumerate(ranked, 1):
        record = index.get(test_id)
        entry = target(
            record[0] if record else None,
            assigned_shard.get(test_id, record[1] if record else None),
            "slowest30",
            test_id=test_id,
            rank=rank,
            total_attempt_seconds=seconds(row),
            source_file=row.get("source_file"),
        )
        entry["has_review"] = record is not None
        slowest30.append(entry)

    def sample_shard(assignment):
        test_ids = assignment["test_ids"]
        n = len(test_ids)
        k = ceil_ten_percent(n)
        sampled = hash_sample(test_ids, k)
        return {
            "shard_id": assignment["shard_id"],
            "n": n,
            "k": k,
            "sampled": [
                target(
                    index[tid][0] if tid in index else None,
                    assignment["shard_id"],
                    "sample_10pct",
                    test_id=tid,
                )
                for tid in sampled
            ],
        }

    samples = [sample_shard(item) for item in sorted(assignments, key=lambda i: i["shard_id"])]
    sample_ids = [entry["test_id"] for shard in samples for entry in shard["sampled"]]

    summary = {
        "shards": len(assignments),
        "inventory_rows": len(inventory),
        "audit_status": audit["status"],
        "reviewed_shards": audit["totals"]["reviewed_shards"],
        "prune_consolidate": len(prune_consolidate),
        "criticality4": len(criticality4),
        "low_confidence": len(low_confidence),
        "slowest30": len(slowest30),
        "sample_10pct_total": len(sample_ids),
        "sample_10pct_by_shard": {shard["shard_id"]: shard["k"] for shard in samples},
    }

    return {
        "generated_by": "evidence/quality_targets.py",
        "summary": summary,
        "prune_consolidate": prune_consolidate,
        "criticality4": criticality4,
        "low_confidence": low_confidence,
        "slowest30": slowest30,
        "sample_10pct": samples,
    }


# --------------------------------------------------------------------------- #
# C. Self-checks
# --------------------------------------------------------------------------- #


def print_table(rows):
    width = max(len(name) for name, _, _ in rows)
    print(f"{'CHECK'.ljust(width)}  RESULT  DETAIL")
    for name, ok, detail in rows:
        print(f"{name.ljust(width)}  {'PASS' if ok else 'FAIL'}    {detail}")


def self_checks(assignments, inventory, audit, targets):
    checks = []

    # 1. Sampling determinism (two independent runs of the sampler in-process).
    first = {
        assignment["shard_id"]: hash_sample(
            assignment["test_ids"], ceil_ten_percent(len(assignment["test_ids"]))
        )
        for assignment in assignments
    }
    second = {
        assignment["shard_id"]: hash_sample(
            assignment["test_ids"], ceil_ten_percent(len(assignment["test_ids"]))
        )
        for assignment in assignments
    }
    checks.append(
        (
            "sample sha256 deterministic",
            first == second,
            "two in-process runs produced identical samples",
        )
    )

    # 2. ceil arithmetic inspection.
    expected = {3: 1, 13: 2, 19: 2, 80: 8}
    actual = {n: ceil_ten_percent(n) for n in expected}
    checks.append(
        (
            "ceil(0.10*n) arithmetic",
            actual == expected,
            f"{actual} (expected {expected})",
        )
    )

    # 3. Every emitted target id exists in inventory.
    emitted = []
    for key in ("prune_consolidate", "criticality4", "low_confidence", "slowest30"):
        emitted.extend(entry["test_id"] for entry in targets[key])
    for shard in targets["sample_10pct"]:
        emitted.extend(entry["test_id"] for entry in shard["sampled"])
    unknown = sorted({tid for tid in emitted if tid not in inventory})
    checks.append(
        (
            "target ids in inventory",
            not unknown,
            f"{len(set(emitted))} distinct target ids, {len(unknown)} unknown",
        )
    )

    # 4. slowest30 sorted descending; boundary visible.
    values = [entry["total_attempt_seconds"] for entry in targets["slowest30"]]
    descending = all(left >= right for left, right in zip(values, values[1:]))
    boundary = values[-1] if values else None
    checks.append(
        (
            "slowest30 sorted + boundary",
            descending and len(values) == 30,
            f"30th value={boundary!r}s, top={values[0] if values else None!r}s",
        )
    )

    # 5. No duplicate ids inside any single list.
    duplicate_lists = []
    for key in ("prune_consolidate", "criticality4", "low_confidence", "slowest30"):
        ids = [entry["test_id"] for entry in targets[key]]
        if len(ids) != len(set(ids)):
            duplicate_lists.append(key)
    for shard in targets["sample_10pct"]:
        ids = [entry["test_id"] for entry in shard["sampled"]]
        if len(ids) != len(set(ids)):
            duplicate_lists.append(f"sample_10pct:{shard['shard_id']}")
    checks.append(
        (
            "no duplicate ids per list",
            not duplicate_lists,
            "clean" if not duplicate_lists else f"duplicates in {duplicate_lists}",
        )
    )

    # 6. Reviewed shards have zero duplicates and zero extras.
    offenders = [
        {
            "shard_id": shard["shard_id"],
            "duplicate_ids": shard["duplicate_ids"],
            "extra_ids": shard["extra_ids"],
        }
        for shard in audit["shards"]
        if shard["review_present"] and (shard["duplicate_ids"] or shard["extra_ids"])
    ]
    checks.append(
        (
            "reviewed shards: 0 dup / 0 extra",
            not offenders,
            f"{audit['totals']['reviewed_shards']} reviewed shards, {len(offenders)} offenders",
        )
    )

    return checks


# --------------------------------------------------------------------------- #
# Main
# --------------------------------------------------------------------------- #


def main():
    assignments = load_assignments()
    inventory = load_inventory()
    audit = build_audit(assignments, inventory)
    targets = build_targets(assignments, inventory, audit)
    checks = self_checks(assignments, inventory, audit, targets)

    with open(AUDIT_OUT, "w", encoding="utf-8") as handle:
        json.dump(audit, handle, indent=2, ensure_ascii=False)
        handle.write("\n")
    with open(TARGETS_OUT, "w", encoding="utf-8") as handle:
        json.dump(targets, handle, indent=2, ensure_ascii=False)
        handle.write("\n")

    print_table(checks)
    failed = [name for name, ok, _ in checks if not ok]

    totals = audit["totals"]
    print()
    print(f"audit status      : {audit['status']}")
    print(
        f"shards            : {totals['reviewed_shards']}/{totals['shards_total']} reviewed, "
        f"{totals['shards_missing_reviews']} missing review file"
    )
    print(
        f"records           : {totals['records']} / {totals['expected']} expected "
        f"(missing ids {totals['missing_ids']}, extra {totals['extra_ids']}, "
        f"duplicates {totals['duplicate_ids']}, schema-error records {totals['schema_error_records']})"
    )
    if audit["shards_without_review_file"]:
        print("missing reviews   : " + ", ".join(audit["shards_without_review_file"]))
    print(
        "targets           : "
        f"prune_consolidate={targets['summary']['prune_consolidate']} "
        f"criticality4={targets['summary']['criticality4']} "
        f"low_confidence={targets['summary']['low_confidence']} "
        f"slowest30={targets['summary']['slowest30']} "
        f"sample_10pct={targets['summary']['sample_10pct_total']}"
    )
    if targets["slowest30"]:
        print("slowest30 boundary: " f"{targets['slowest30'][-1]['total_attempt_seconds']}s")
        print("slowest30 top5    :")
        for entry in targets["slowest30"][:5]:
            print(
                f"  {entry['rank']:>2}. {entry['total_attempt_seconds']:>8.3f}s "
                f"{entry['test_id']}"
            )
    print("sample sizes      : " + ", ".join(
        f"{shard['shard_id']}={shard['k']}/{shard['n']}"
        for shard in targets["sample_10pct"]
    ))

    if failed:
        print(f"\nFAILED checks: {', '.join(failed)}")
        return 1
    print("\nall self-checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
