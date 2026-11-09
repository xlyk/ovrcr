#!/usr/bin/env python3
"""Merge evidence/quality-review-*.jsonl receipts into evidence/quality-review.jsonl.

Receipts are written by independent second reviewers, one file per workstream.
This script concatenates them, validates every receipt against the review
schema and the inventory, and writes one deterministic line per test_id.

Two independent reviewers may legitimately cover the same row. A duplicate
receipt is dropped and named with both locations and second reviewers (the
first file in sorted order wins) but does not block the merge. Any other reject
(unknown test_id, invalid verdict, malformed final_review, missing or
self-review second_reviewer) blocks the write.

Coverage is reported on the merged semantics: a resolved receipt's final_review
overrides the authoring row before the required-receipt gate is applied
(criticality 4, prune_candidate/consolidate, or low confidence).

Usage:
    python3 -I research/macos-test-review/evidence/merge-quality-review.py
    python3 -I research/macos-test-review/evidence/merge-quality-review.py --check

--check validates and prints the summary without writing the merged file.
"""
import argparse
import csv
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "evidence"
REVIEWS = ROOT / "reviews"
INVENTORY = EVIDENCE / "inventory.csv"
OUTPUT = EVIDENCE / "quality-review.jsonl"
PREFIX = "quality-review-"

KEYS = [
    "test_id", "test_level", "production_refs", "behavior", "failure_caught",
    "criticality", "signal", "uniqueness", "fidelity",
    "criticality_reason", "signal_reason", "uniqueness_reason", "fidelity_reason",
    "confidence", "recommendation", "recommendation_reason",
    "overlap_test_ids", "cost_drivers", "reviewer",
]
SCORE_FIELDS = ("criticality", "signal", "uniqueness", "fidelity")
CONFIDENCES = {"high", "medium", "low"}
RECOMMENDATIONS = {"investigate", "consolidate", "prune_candidate", "improve", "keep"}
VERDICTS = {"agree", "resolved"}
REQUIRED_RECOMMENDATIONS = {"prune_candidate", "consolidate"}


def load_inventory():
    with INVENTORY.open(newline="", encoding="utf-8") as handle:
        return {row["test_id"] for row in csv.DictReader(handle)}


def load_review_rows():
    """test_id -> (review filename, authoring row). Latest file wins on repeats."""
    rows = {}
    for path in sorted(REVIEWS.glob("*.jsonl")):
        with path.open(encoding="utf-8") as handle:
            for line in handle:
                line = line.strip()
                if not line:
                    continue
                record = json.loads(line)
                test_id = record.get("test_id")
                if isinstance(test_id, str):
                    rows[test_id] = (path.name, record)
    return rows


def receipt_files():
    return [
        path
        for path in sorted(EVIDENCE.glob(PREFIX + "*.jsonl"))
        if path.name != OUTPUT.name
    ]


def validate_final_review(final, test_id):
    if not isinstance(final, dict):
        return "final_review is not an object"
    missing = [key for key in KEYS if key not in final]
    extra = [key for key in final if key not in KEYS]
    if missing or extra:
        return f"final_review key set differs: missing={missing} extra={extra}"
    if final.get("test_id") != test_id:
        return f"final_review test_id {final.get('test_id')!r} differs from receipt"
    for field in SCORE_FIELDS:
        value = final.get(field)
        if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value <= 4:
            return f"final_review {field} is not an integer 0..4: {value!r}"
    return None


def collect():
    inventory = load_inventory()
    files = receipt_files()
    by_id = {}
    rejects = []
    drops = []
    warnings = []
    per_file = []

    for path in files:
        count = 0
        with path.open(encoding="utf-8") as handle:
            for number, line in enumerate(handle, start=1):
                if not line.strip():
                    continue
                where = f"{path.name}:{number}"
                try:
                    receipt = json.loads(line)
                except json.JSONDecodeError as error:
                    rejects.append((where, None, f"invalid JSON: {error}"))
                    continue
                if not isinstance(receipt, dict):
                    rejects.append((where, None, "receipt is not an object"))
                    continue
                test_id = receipt.get("test_id")
                if not isinstance(test_id, str) or not test_id:
                    rejects.append((where, None, "missing test_id"))
                    continue
                if test_id not in inventory:
                    rejects.append((where, test_id, "test_id not in inventory.csv"))
                    continue
                if test_id in by_id:
                    kept_where, kept = by_id[test_id]
                    drops.append(
                        (
                            where,
                            test_id,
                            f"duplicate receipt; kept {kept_where} "
                            f"(second_reviewer {kept.get('second_reviewer')!r}); "
                            f"discarded {where} (second_reviewer {receipt.get('second_reviewer')!r})",
                        )
                    )
                    continue

                second = receipt.get("second_reviewer")
                if not isinstance(second, str) or not second.strip():
                    rejects.append((where, test_id, "missing second_reviewer"))
                    continue
                author = REVIEW_ROWS.get(test_id, (None, {}))[1].get("reviewer", "")
                if second.strip() == str(author).strip():
                    rejects.append((where, test_id, f"self-review: second_reviewer {second!r} equals reviewer {author!r}"))
                    continue

                verdict = receipt.get("verdict")
                if verdict not in VERDICTS:
                    rejects.append((where, test_id, f"invalid verdict {verdict!r}"))
                    continue
                findings = receipt.get("findings")
                if not (
                    (isinstance(findings, str) and findings.strip())
                    or (isinstance(findings, (list, dict)) and len(findings) > 0)
                ):
                    warnings.append((test_id, "receipt has empty findings"))
                if verdict == "agree":
                    if receipt.get("final_review") is not None:
                        rejects.append((where, test_id, "agree receipt must have final_review null"))
                        continue
                else:
                    problem = validate_final_review(receipt.get("final_review"), test_id)
                    if problem is not None:
                        rejects.append((where, test_id, problem))
                        continue
                    final = receipt["final_review"]
                    if final.get("confidence") not in CONFIDENCES:
                        warnings.append((test_id, f"final_review confidence {final.get('confidence')!r}"))
                    if final.get("recommendation") not in RECOMMENDATIONS:
                        warnings.append((test_id, f"final_review recommendation {final.get('recommendation')!r}"))
                    if not isinstance(final.get("production_refs"), list) or not isinstance(
                        final.get("overlap_test_ids"), list
                    ):
                        warnings.append((test_id, "final_review refs/overlap are not lists"))

                by_id[test_id] = (where, receipt)
                count += 1
        per_file.append((path.name, count))

    return {
        "files": files,
        "per_file": per_file,
        "receipts": by_id,
        "rejects": rejects,
        "drops": drops,
        "warnings": warnings,
    }


def merged_semantics(result):
    """Reviewed rows with each resolved receipt's final_review applied."""
    rows = {}
    for test_id, (filename, row) in REVIEW_ROWS.items():
        merged = dict(row)
        receipt = result["receipts"].get(test_id)
        if receipt is not None and receipt[1].get("verdict") == "resolved":
            merged.update(receipt[1]["final_review"])
        rows[test_id] = (filename, merged)
    return rows


def required(row):
    return (
        row.get("criticality") == 4
        or row.get("recommendation") in REQUIRED_RECOMMENDATIONS
        or row.get("confidence") == "low"
    )


def coverage(result):
    rows = merged_semantics(result)
    missing = defaultdict(list)
    required_rows = 0
    extra = 0
    for test_id, (filename, row) in rows.items():
        needs = required(row)
        if needs:
            required_rows += 1
        if needs and test_id not in result["receipts"]:
            missing[filename].append(test_id)
        if not needs and test_id in result["receipts"]:
            extra += 1
    return rows, required_rows, extra, missing


def main():
    global REVIEW_ROWS
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate and print without writing")
    args = parser.parse_args()

    if not EVIDENCE.is_dir() or not REVIEWS.is_dir():
        print(f"error: expected evidence/ and reviews/ under {ROOT}", file=sys.stderr)
        return 2

    REVIEW_ROWS = load_review_rows()
    result = collect()
    receipts = result["receipts"]
    verdicts = Counter(receipt["verdict"] for _, receipt in receipts.values())
    rows, required_rows, extra, missing = coverage(result)

    print("receipts per file:")
    for name, count in result["per_file"]:
        print(f"  {name}: {count}")
    print(f"total receipts: {len(receipts)}")
    print(f"distinct test_ids: {len(receipts)}")
    print(
        "verdicts: "
        + ", ".join(f"{name}={verdicts.get(name, 0)}" for name in sorted(VERDICTS))
    )
    print(f"duplicates dropped: {len(result['drops'])}")
    for where, test_id, reason in result["drops"]:
        print(f"  {where} {test_id or '-'}: {reason}")
    print(f"rejects: {len(result['rejects'])}")
    for where, test_id, reason in result["rejects"]:
        print(f"  {where} {test_id or '-'}: {reason}")
    print(f"warnings: {len(result['warnings'])}")
    for test_id, reason in result["warnings"]:
        print(f"  {test_id}: {reason}")
    print(f"required receipts: {required_rows - sum(len(v) for v in missing.values())} of {required_rows}")
    print(f"extra receipts on non-required rows: {extra}")
    total_missing = sum(len(v) for v in missing.values())
    print(f"missing required receipts: {total_missing}")
    for filename in sorted(missing):
        print(f"  {filename}: {len(missing[filename])}")
        for test_id in sorted(missing[filename]):
            print(f"    {test_id}")

    if result["rejects"]:
        print("merge refused: fix the rejects above", file=sys.stderr)
        return 1

    if args.check:
        print("check only; quality-review.jsonl not written")
        return 0

    ordered = [receipts[test_id][1] for test_id in sorted(receipts)]
    with OUTPUT.open("w", encoding="utf-8", newline="\n") as handle:
        for receipt in ordered:
            handle.write(json.dumps(receipt, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
            handle.write("\n")
    print(f"wrote {len(ordered)} receipts to {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    REVIEW_ROWS = {}
    raise SystemExit(main())
