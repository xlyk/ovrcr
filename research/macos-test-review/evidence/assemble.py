#!/usr/bin/env python3
"""Assemble and validate the macOS test-value review report.

Modes
-----
    python3 -I .../evidence/assemble.py            # assemble: write tests.csv + evidence/validation.json
    python3 -I .../evidence/assemble.py --check    # read-only validation, no files written
    python3 -I .../evidence/assemble.py --selftest # prove the validator fails/passes on disposable fixtures

Inputs (relative to research/macos-test-review/):
    evidence/inventory.csv          canonical identity/timing rows (exact header below)
    evidence/inventory_rust.csv     compatibility fallback when inventory.csv is absent
    reviews/*.jsonl                 worker semantic records
    evidence/overlap-groups.json    confirmed overlap groups
    evidence/quality-review.jsonl   independent-review receipts
    evidence/calibration.jsonl      calibration receipts (reporting only)
    evidence/attempts.csv, evidence/manifest.json, evidence/macos-job.log
                                    hosted-evidence gates (optional; asserted when present)

Python 3 standard library only. No product code, tests, workflow, or config is read/written.
"""

from __future__ import annotations

import argparse
import contextlib
import csv
import hashlib
import io
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

# --------------------------------------------------------------------------------------
# Constants
# --------------------------------------------------------------------------------------

BASE = Path(__file__).resolve().parent.parent          # research/macos-test-review
REPO_ROOT = BASE.parent.parent                          # repository root

INVENTORY_HEADER = [
    "test_id", "parent_test_id", "revision", "ci_step", "runner", "suite", "target_kind",
    "test_name", "variant", "source_file", "source_start", "source_end",
    "first_attempt_seconds", "total_attempt_seconds", "attempt_count", "failed_attempt_count",
    "timing_basis", "timing_source",
]

# Peer intermediate emitted by the inventory workstream (documented compatibility path).
INVENTORY_RUST_HEADER = [
    "test_id", "input_test_name", "invocation", "runner", "suite", "target_kind",
    "test_level", "test_name", "variant", "source_file", "source_start", "source_end",
    "first_attempt_seconds", "total_attempt_seconds", "attempt_count", "failed_attempt_count",
    "timing_basis", "timing_source",
]

OUTPUT_HEADER = [
    "test_id", "parent_test_id", "revision", "ci_step", "runner", "suite", "target_kind",
    "test_level", "test_name", "variant", "source_file", "source_start", "source_end",
    "production_refs", "behavior", "failure_caught",
    "criticality", "signal", "uniqueness", "fidelity",
    "criticality_score", "usefulness_score", "retention_score",
    "criticality_reason", "signal_reason", "uniqueness_reason", "fidelity_reason",
    "confidence", "recommendation", "recommendation_reason",
    "overlap_test_ids", "cost_drivers",
    "first_attempt_seconds", "total_attempt_seconds", "attempt_count", "failed_attempt_count",
    "timing_basis", "timing_source", "reviewer", "second_reviewer",
]

IDENTITY_COLUMNS = [
    "parent_test_id", "revision", "ci_step", "runner", "suite", "target_kind",
    "test_name", "variant", "source_file", "source_start", "source_end",
]

SEMANTIC_FIELDS = [
    "test_level", "production_refs", "behavior", "failure_caught",
    "criticality", "signal", "uniqueness", "fidelity",
    "criticality_reason", "signal_reason", "uniqueness_reason", "fidelity_reason",
    "confidence", "recommendation", "recommendation_reason",
    "overlap_test_ids", "cost_drivers", "reviewer",
]

DERIVED_SCORES = ["criticality_score", "usefulness_score", "retention_score"]
OPTIONAL_SEMANTIC_FIELDS = {"cost_drivers"}
# Reviewer identity is provenance, never a judgment: a resolved receipt may not rewrite it.
FINAL_REVIEW_OVERRIDE_FIELDS = [field for field in SEMANTIC_FIELDS if field != "reviewer"]
SCORE_DIMENSIONS = ["criticality", "signal", "uniqueness", "fidelity"]

RUNNERS = {"nextest", "cargo-test", "node-test", "python-unittest", "python-check"}
TARGET_KINDS = {"lib", "bin", "integration", "node", "unittest", "script-check", "doctest"}
TEST_LEVELS = {"unit", "integration", "acceptance"}
CONFIDENCES = {"high", "medium", "low"}
RECOMMENDATIONS = {"investigate", "consolidate", "prune_candidate", "improve", "keep"}
REMOVAL = {"prune_candidate", "consolidate"}
RECEIPT_VERDICTS = {"agree", "resolved"}

PLACEHOLDER_RE = re.compile(r"^(tbd|todo|n/?a|none|unknown|placeholder|-|)$", re.IGNORECASE)
MIN_RATIONALE_CHARS = 12
RATIONALE_FIELDS_MIN_LEN = [
    "behavior", "failure_caught",
    "criticality_reason", "signal_reason", "uniqueness_reason", "fidelity_reason",
]
RATIONALE_FIELDS_PLACEHOLDER = RATIONALE_FIELDS_MIN_LEN + ["recommendation_reason"]

# Hosted-execution gates for run 42429838269 / job 128576086691.
EXPECTED_ATTEMPT_RECORDS = 2822
EXPECTED_UNIQUE_TESTS = 2820
EXPECTED_BINARIES = 56
EXPECTED_CUMULATIVE_SECONDS = 5931.968
GATE_TOLERANCE = 0.001
RETRY_GATES = [
    (
        "codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path",
        4.855, 9.227, 2, 1,
    ),
    (
        "parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects",
        24.507, 46.250, 2, 1,
    ),
]
ATTEMPTS_HEADER = ["invocation", "binary_id", "test_name", "attempt", "result", "seconds", "raw_line"]

CHECK_ROWCOUNTS = "1 row-count-and-identity-join"
CHECK_SCORES = "2 scores-and-formulas"
CHECK_RATIONALE = "3 rationale-quality"
CHECK_SOURCES = "4 source-ranges"
CHECK_GATES = "5 recommendation-gates-and-reviews"
CHECK_GROUPS = "6 overlap-groups-and-reference-integrity"
CHECK_HOSTED = "7 hosted-evidence-gates"
CHECK_ROUNDTRIP = "8 csv-round-trip"
CHECK_ORDER = "9 deterministic-order"

FIXTURE_REVISION = "992a1da5c59a865ebb48051e4d9d8ff7184c8dc5"


# --------------------------------------------------------------------------------------
# Small helpers
# --------------------------------------------------------------------------------------

class InputError(Exception):
    """Missing or malformed required input: fail loudly, write nothing."""


def rel(path: Path, base: Path) -> str:
    try:
        return str(path.relative_to(base))
    except ValueError:
        return str(path)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def check(name: str, status: str, detail: str, offenders=None) -> dict:
    return {
        "check": name,
        "status": status,
        "detail": detail,
        "offending_ids": sorted({str(x) for x in (offenders or [])}),
    }


def is_int(value) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def as_int(value):
    if is_int(value):
        return value
    if isinstance(value, str) and re.fullmatch(r"[+-]?\d+", value.strip()):
        return int(value.strip())
    return None


def as_text(value) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    return str(value)


def serialize_cost_drivers(value) -> str:
    """Empty forms mean 'no observed cost driver' and serialize as a JSON [].

    A non-empty string stays verbatim; a non-empty array becomes a JSON array.
    """
    if value is None:
        return "[]"
    if isinstance(value, str):
        return value if value.strip() else "[]"
    if isinstance(value, list):
        return json.dumps(value, ensure_ascii=False) if value else "[]"
    return as_text(value)


def ids_summary(offenders, limit=25) -> str:
    if not offenders:
        return ""
    shown = offenders[:limit]
    extra = len(offenders) - len(shown)
    return ", ".join(shown) + (f" (+{extra} more)" if extra > 0 else "")


def write_atomic(path: Path, data: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    with tmp.open("w", encoding="utf-8", newline="") as handle:
        handle.write(data)
    os.replace(tmp, path)


def repo_revision(repo_root: Path):
    """Best-effort pinned revision: manifest first, then read-only git."""
    manifest = repo_root / "research" / "macos-test-review" / "evidence" / "manifest.json"
    if manifest.is_file():
        try:
            data = json.loads(manifest.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            data = None
        if data is not None:
            for key, value in flatten_json(data):
                last = key.rsplit(".", 1)[-1].lower()
                if last in {"revision", "headsha", "head_sha", "commit"} and isinstance(value, str):
                    return value
    try:
        proc = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
            capture_output=True, text=True, timeout=10,
        )
        if proc.returncode == 0 and re.fullmatch(r"[0-9a-f]{40}", proc.stdout.strip()):
            return proc.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return None


# --------------------------------------------------------------------------------------
# Input loading
# --------------------------------------------------------------------------------------

def _read_csv_rows(path: Path, expected_header, label: str):
    with path.open(newline="", encoding="utf-8-sig") as handle:
        reader = csv.reader(handle)
        try:
            header = next(reader)
        except StopIteration:
            raise InputError(f"{label} is empty (no header row)")
        if header != list(expected_header):
            missing = [c for c in expected_header if c not in header]
            unexpected = [c for c in header if c not in expected_header]
            raise InputError(
                f"{label} header mismatch; expected {len(expected_header)} columns, got {len(header)}; "
                f"missing={missing}; unexpected={unexpected}"
            )
        rows = []
        for lineno, cells in enumerate(reader, start=2):
            if not cells or all(cell == "" for cell in cells):
                continue
            if len(cells) != len(header):
                raise InputError(
                    f"{label}:{lineno}: expected {len(header)} columns, got {len(cells)}"
                )
            rows.append(dict(zip(header, cells)))
    return rows


def load_inventory(root: Path, notes):
    canonical = root / "evidence" / "inventory.csv"
    fallback = root / "evidence" / "inventory_rust.csv"
    if canonical.is_file():
        rows = _read_csv_rows(canonical, INVENTORY_HEADER, rel(canonical, root))
        return rows, "evidence/inventory.csv", False
    if fallback.is_file():
        fragments = sorted(
            name for name in ("inventory_node.csv", "inventory_python.csv")
            if (root / "evidence" / name).is_file()
        )
        if fragments:
            raise InputError(
                "evidence/inventory.csv is missing but fragment inventories exist "
                f"({', '.join(fragments)}); build the merged canonical inventory.csv instead. "
                "The Rust-only compatibility import would silently drop those rows."
            )
        raw = _read_csv_rows(fallback, INVENTORY_RUST_HEADER, rel(fallback, root))
        revision = repo_revision(REPO_ROOT)
        notes.append(
            "inventory.csv absent; imported evidence/inventory_rust.csv compatibility rows "
            "(ci_step<-invocation, revision<-"
            + (revision or "unresolved")
            + ", parent_test_id=''; input_test_name/test_level ignored)"
        )
        rows = []
        for row in raw:
            rows.append({
                "test_id": row["test_id"],
                "parent_test_id": "",
                "revision": revision or "",
                "ci_step": row["invocation"],
                "runner": row["runner"],
                "suite": row["suite"],
                "target_kind": row["target_kind"],
                "test_name": row["test_name"],
                "variant": row["variant"],
                "source_file": row["source_file"],
                "source_start": row["source_start"],
                "source_end": row["source_end"],
                "first_attempt_seconds": row["first_attempt_seconds"],
                "total_attempt_seconds": row["total_attempt_seconds"],
                "attempt_count": row["attempt_count"],
                "failed_attempt_count": row["failed_attempt_count"],
                "timing_basis": row["timing_basis"],
                "timing_source": row["timing_source"],
            })
        return rows, "evidence/inventory_rust.csv", True
    raise InputError(
        "missing required inventory input: evidence/inventory.csv "
        "(compatibility fallback evidence/inventory_rust.csv is also absent)"
    )


def _load_jsonl(path: Path, label: str, errors):
    records = []
    text = path.read_text(encoding="utf-8")
    for lineno, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            obj = json.loads(line)
        except ValueError as exc:
            errors.append(f"{label}:{lineno}: invalid JSON: {exc}")
            continue
        if not isinstance(obj, dict):
            errors.append(f"{label}:{lineno}: record is not a JSON object")
            continue
        records.append({"_file": label, "_line": lineno, "_record": obj})
    return records


def load_model(root: Path, *, require_groups: bool = True,
               merge_segmented_receipts: bool = False,
               scan_receipt_segments: bool = False) -> dict:
    notes = []
    errors = []
    inventory, inventory_source, compatibility = load_inventory(root, notes)

    reviews_dir = root / "reviews"
    if not reviews_dir.is_dir():
        raise InputError(f"missing required input directory: {rel(reviews_dir, root)}/ (no reviews land yet)")
    review_files = sorted(reviews_dir.glob("*.jsonl"))
    if not review_files:
        raise InputError(
            f"no review records: {rel(reviews_dir, root)}/ contains no *.jsonl files"
        )
    review_records = []
    review_errors = []
    for path in review_files:
        review_records.extend(_load_jsonl(path, rel(path, root), review_errors))

    groups_path = root / "evidence" / "overlap-groups.json"
    groups = None
    groups_error = None
    if not groups_path.is_file():
        if require_groups:
            raise InputError(
                "missing required input: evidence/overlap-groups.json "
                "(write [] when there are no confirmed groups)"
            )
        groups = []
        notes.append("overlap-groups.json absent; treated as [] for this diagnostic audit")
    else:
        try:
            groups = json.loads(groups_path.read_text(encoding="utf-8"))
            if not isinstance(groups, list):
                groups_error = "top-level value is not a JSON array"
        except ValueError as exc:
            groups_error = f"invalid JSON: {exc}"

    receipts_path = root / "evidence" / "quality-review.jsonl"
    receipt_records = []
    receipt_errors = []
    receipt_files_used = []
    if receipts_path.is_file():
        receipt_records = _load_jsonl(receipts_path, rel(receipts_path, root), receipt_errors)
        receipt_files_used = [rel(receipts_path, root)]
    elif merge_segmented_receipts:
        segment_paths = sorted((root / "evidence").glob("quality-review-*.jsonl"))
        for path in segment_paths:
            receipt_records.extend(_load_jsonl(path, rel(path, root), receipt_errors))
            receipt_files_used.append(rel(path, root))
        if receipt_files_used:
            notes.append(
                "quality-review.jsonl absent; merged "
                f"{len(segment_paths)} segmented receipt files in memory for this diagnostic audit"
            )

    calibration_path = root / "evidence" / "calibration.jsonl"
    calibration_count = 0
    calibration_note = "absent"
    if calibration_path.is_file():
        cal_errors = []
        calibration_count = len(_load_jsonl(calibration_path, rel(calibration_path, root), cal_errors))
        calibration_note = f"{calibration_count} receipts" + (
            f"; {len(cal_errors)} malformed lines" if cal_errors else ""
        )

    model = build_model(
        root, inventory, inventory_source, compatibility, notes,
        review_records, review_errors, groups, groups_error,
        receipt_records, receipt_errors, calibration_count, calibration_note,
    )
    review_sources = {}
    for record in review_records:
        tid = record["_record"].get("test_id")
        if isinstance(tid, str):
            review_sources[tid] = {"file": record["_file"], "line": record["_line"]}
    receipt_sources = {}
    for record in receipt_records:
        tid = record["_record"].get("test_id")
        if isinstance(tid, str):
            receipt_sources.setdefault(tid, []).append(
                {"file": record["_file"], "line": record["_line"]}
            )
    segment_receipt_sources = {}
    segment_files_used = []
    if scan_receipt_segments:
        for path in sorted((root / "evidence").glob("quality-review-*.jsonl")):
            segment_files_used.append(rel(path, root))
            for record in _load_jsonl(path, rel(path, root), []):
                tid = record["_record"].get("test_id")
                if isinstance(tid, str):
                    segment_receipt_sources.setdefault(tid, []).append({
                        "file": record["_file"],
                        "line": record["_line"],
                        "second_reviewer": record["_record"].get("second_reviewer"),
                        "verdict": record["_record"].get("verdict"),
                    })
    model["review_sources"] = review_sources
    model["receipt_sources"] = receipt_sources
    model["segment_receipt_sources"] = segment_receipt_sources
    model["segment_files_used"] = segment_files_used
    model["receipt_files_used"] = receipt_files_used
    model["hash_paths"] = sorted(
        path for path in list(root.glob("evidence/*")) + review_files
        if path.is_file() and path.name not in {"validation.json", "audit-details.json"}
    )
    model["input_hashes"] = {
        rel(path, root): {"sha256": sha256_file(path), "bytes": path.stat().st_size}
        for path in model["hash_paths"]
    }
    return model


def build_model(root, inventory, inventory_source, compatibility, notes,
                review_records, review_errors, groups, groups_error,
                receipt_records, receipt_errors, calibration_count, calibration_note) -> dict:
    inv_by_id = {}
    duplicate_inventory = []
    for row in inventory:
        tid = row.get("test_id", "")
        if tid in inv_by_id:
            duplicate_inventory.append(tid or "<empty test_id>")
            continue
        inv_by_id[tid] = row

    reviews_by_id = {}
    duplicate_reviews = []
    unkeyed_reviews = []
    shape_errors = []
    for record in review_records:
        obj = record["_record"]
        tid = obj.get("test_id")
        if not isinstance(tid, str) or not tid.strip():
            unkeyed_reviews.append(f"{record['_file']}:{record['_line']}")
            continue
        if tid in reviews_by_id:
            duplicate_reviews.append(tid)
            continue
        reviews_by_id[tid] = obj
        for field, message in record_shape_errors(obj):
            shape_errors.append(f"{tid}:{field} {message}")

    receipts_by_id = {}
    duplicate_receipts = []
    for record in receipt_records:
        obj = record["_record"]
        tid = obj.get("test_id")
        if not isinstance(tid, str) or not tid.strip():
            receipt_errors.append(f"{record['_file']}:{record['_line']}: missing test_id")
            continue
        if tid in receipts_by_id:
            duplicate_receipts.append(tid)
            continue
        receipts_by_id[tid] = obj

    unknown_reviews = sorted(tid for tid in reviews_by_id if tid not in inv_by_id)
    missing_reviews = sorted(tid for tid in inv_by_id if tid not in reviews_by_id)

    merged = {}
    conflicts = []
    for tid, inventory_row in inv_by_id.items():
        worker = reviews_by_id.get(tid)
        if worker is None:
            continue
        receipt = receipts_by_id.get(tid)
        semantic = {}
        for field in SEMANTIC_FIELDS:
            if field in worker:
                semantic[field] = worker[field]
        provided = {}

        def note_conflicts(source):
            for column in IDENTITY_COLUMNS:
                if column in source:
                    review_value = source[column]
                    inventory_value = inventory_row.get(column, "")
                    if not identity_equal(column, inventory_value, review_value):
                        conflicts.append(
                            f"{tid}:{column} inventory={inventory_value!r} review={review_value!r}"
                        )

        note_conflicts(worker)
        for derived in DERIVED_SCORES:
            if derived in worker:
                provided[derived] = worker[derived]
        if receipt is not None and receipt.get("verdict") == "resolved":
            final_review = receipt.get("final_review")
            if isinstance(final_review, dict):
                note_conflicts(final_review)
                if "test_id" in final_review and final_review["test_id"] != tid:
                    conflicts.append(
                        f"{tid}:test_id receipt={final_review['test_id']!r}"
                    )
                for field in FINAL_REVIEW_OVERRIDE_FIELDS:
                    if field in final_review:
                        semantic[field] = final_review[field]
                for derived in DERIVED_SCORES:
                    if derived in final_review:
                        provided[derived] = final_review[derived]
        missing = [
            field for field in SEMANTIC_FIELDS
            if field not in semantic and field not in OPTIONAL_SEMANTIC_FIELDS
        ]
        for field, message in record_type_errors(semantic):
            shape_errors.append(f"{tid}:{field} merged value {message}")
        merged[tid] = {
            "semantic": semantic,
            "provided_scores": provided,
            "missing_fields": missing,
            "receipt": receipt,
        }

    return {
        "root": root,
        "inventory": inventory,
        "inventory_by_id": inv_by_id,
        "inventory_source": inventory_source,
        "compatibility": compatibility,
        "notes": notes,
        "review_records": review_records,
        "review_errors": review_errors,
        "reviews_by_id": reviews_by_id,
        "duplicate_reviews": sorted(set(duplicate_reviews)),
        "unkeyed_reviews": unkeyed_reviews,
        "shape_errors": shape_errors,
        "unknown_reviews": unknown_reviews,
        "missing_reviews": missing_reviews,
        "duplicate_inventory_ids": sorted(set(duplicate_inventory)),
        "receipts_by_id": receipts_by_id,
        "duplicate_receipts": sorted(set(duplicate_receipts)),
        "receipt_errors": receipt_errors,
        "receipt_count": len(receipts_by_id),
        "groups": groups,
        "groups_error": groups_error,
        "merged": merged,
        "conflicts": sorted(set(conflicts)),
        "calibration_count": calibration_count,
        "calibration_note": calibration_note,
    }


def identity_equal(column, inventory_value, review_value) -> bool:
    if column in {"source_start", "source_end"}:
        left, right = as_int(inventory_value), as_int(review_value)
        if left is not None and right is not None:
            return left == right
        return as_text(inventory_value).strip() == as_text(review_value).strip()
    return as_text(inventory_value).strip() == as_text(review_value).strip()


def record_type_errors(record: dict):
    errors = []
    string_fields = [
        "test_level", "behavior", "failure_caught",
        "criticality_reason", "signal_reason", "uniqueness_reason", "fidelity_reason",
        "confidence", "recommendation", "recommendation_reason", "reviewer",
    ]
    for field in string_fields:
        if field in record and not isinstance(record[field], str):
            errors.append((field, "must be a string"))
    if "cost_drivers" in record:
        value = record["cost_drivers"]
        allowed = (
            value is None
            or isinstance(value, str)
            or (
                isinstance(value, list)
                and all(isinstance(item, str) and bool(item.strip()) for item in value)
            )
        )
        if not allowed:
            errors.append((
                "cost_drivers",
                "must be null, a string, or an array of non-empty strings "
                "(an empty array/string means no observed cost driver)",
            ))
    for field in ("production_refs", "overlap_test_ids"):
        if field in record:
            value = record[field]
            if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
                errors.append((field, "must be an array of strings"))
    return errors


def record_shape_errors(record: dict):
    errors = [
        (field, "missing") for field in SEMANTIC_FIELDS
        if field not in record and field not in OPTIONAL_SEMANTIC_FIELDS
    ]
    return errors + record_type_errors(record)


def compute_scores(semantic: dict):
    values = []
    for field in SCORE_DIMENSIONS:
        value = semantic.get(field)
        if not is_int(value) or not 0 <= value <= 4:
            return None, field
        values.append(value)
    criticality, signal, uniqueness, fidelity = values
    return {
        "criticality_score": 25 * criticality,
        "usefulness_score": 10 * signal + 10 * uniqueness + 5 * fidelity,
        "retention_score": 10 * criticality + 6 * signal + 6 * uniqueness + 3 * fidelity,
    }, None


# --------------------------------------------------------------------------------------
# Checks
# --------------------------------------------------------------------------------------

def check_rowcounts_and_join(model: dict) -> dict:
    offenders = []
    inventory = model["inventory"]
    notes = []

    for tid in model["duplicate_inventory_ids"]:
        offenders.append(f"{tid}:duplicate test_id")
    for tid in model["duplicate_reviews"]:
        offenders.append(f"{tid}:duplicate review record")
    for tid in model["missing_reviews"]:
        offenders.append(f"{tid}:missing review")
    for tid in model["unknown_reviews"]:
        offenders.append(f"{tid}:review without inventory row")
    for marker in model["unkeyed_reviews"]:
        offenders.append(f"{marker}:review missing test_id")
    offenders.extend(model["shape_errors"])
    offenders.extend(model["conflicts"])
    for message in model["review_errors"]:
        offenders.append(message)

    for row in inventory:
        tid = row.get("test_id", "") or "<empty test_id>"
        for column in ("test_id", "revision", "ci_step", "runner", "suite",
                       "target_kind", "test_name", "variant", "source_file", "timing_basis"):
            if not as_text(row.get(column, "")).strip():
                offenders.append(f"{tid}:empty {column}")
        if row.get("runner", "") not in RUNNERS:
            offenders.append(f"{tid}:invalid runner {row.get('runner')!r}")
        if row.get("target_kind", "") not in TARGET_KINDS:
            offenders.append(f"{tid}:invalid target_kind {row.get('target_kind')!r}")

    for tid, entry in model["merged"].items():
        semantic = entry["semantic"]
        if semantic.get("test_level") not in TEST_LEVELS:
            offenders.append(f"{tid}:invalid test_level {semantic.get('test_level')!r}")
        if not as_text(semantic.get("reviewer", "")).strip():
            offenders.append(f"{tid}:empty reviewer")
        for field in entry["missing_fields"]:
            offenders.append(f"{tid}:{field} missing from semantic record")
        receipt = entry["receipt"] or {}
        second = as_text(receipt.get("second_reviewer", "")).strip()
        author = as_text(semantic.get("reviewer", "")).strip()
        if second and author and second == author:
            offenders.append(f"{tid}:reviewer equals second_reviewer")

    # Revision consistency (single frozen revision across the report).
    revisions = {}
    for row in inventory:
        revisions.setdefault(as_text(row.get("revision", "")).strip(), []).append(
            row.get("test_id", "") or "<empty test_id>"
        )
    if len(revisions) > 1:
        ordered = sorted(revisions)
        expected = ordered[0]
        for revision in ordered[1:]:
            for tid in revisions[revision]:
                offenders.append(f"{tid}:revision {revision!r} != {expected!r}")
        notes.append(f"{len(revisions)} distinct revisions")

    # Parent references resolve in this CSV, never self.
    inventory_ids = set(model["inventory_by_id"])
    for row in inventory:
        tid = row.get("test_id", "")
        parent = as_text(row.get("parent_test_id", "")).strip()
        if not parent:
            continue
        if parent == tid:
            offenders.append(f"{tid}:self-parent")
        elif parent not in inventory_ids:
            offenders.append(f"{tid}:parent {parent} does not resolve")

    by_runner, by_step, by_variant = {}, {}, {}
    for row in inventory:
        for bucket, key in (
            (by_runner, row.get("runner", "")),
            (by_step, row.get("ci_step", "")),
            (by_variant, row.get("variant", "")),
        ):
            bucket[key] = bucket.get(key, 0) + 1
    runner_sum = sum(by_runner.values())
    if runner_sum != len(inventory):
        offenders.append(
            f"<counts>:sum(per-runner)={runner_sum} != rows={len(inventory)}"
        )

    detail = (
        f"rows={len(inventory)}; unique_test_ids={len(model['inventory_by_id'])}; "
        f"reviews={len(model['reviews_by_id'])}; per_runner={by_runner}; "
        f"per_ci_step={by_step}; per_variant={by_variant}"
    )
    if notes:
        detail += "; " + "; ".join(notes)
    if model["compatibility"]:
        detail += "; inventory_source=evidence/inventory_rust.csv (compatibility import)"
    status = "failed" if offenders else "passed"
    return check(CHECK_ROWCOUNTS, status, detail, offenders)


def check_scores(model: dict) -> dict:
    offenders = []
    checked = 0
    provided_checked = 0
    for tid, entry in sorted(model["merged"].items()):
        semantic = entry["semantic"]
        scores, bad_field = compute_scores(semantic)
        if scores is None:
            offenders.append(f"{tid}:{bad_field} missing or not an integer 0..4")
            continue
        checked += 1
        for derived, value in entry["provided_scores"].items():
            provided_checked += 1
            if not is_int(value) or value != scores[derived]:
                offenders.append(
                    f"{tid}:{derived}={value!r} but recomputation gives {scores[derived]}"
                )
    detail = (
        f"recomputed formulas for {checked} rows "
        f"(criticality_score=25*C, usefulness_score=10*S+10*U+5*F, "
        f"retention_score=10*C+6*S+6*U+3*F); "
        f"cross-checked {provided_checked} provided derived scores"
    )
    status = "failed" if offenders else "passed"
    return check(CHECK_SCORES, status, detail, offenders)


def check_rationale(model: dict) -> dict:
    offenders = []
    checked = 0
    placeholder_count = 0
    short_count = 0
    for tid, entry in sorted(model["merged"].items()):
        semantic = entry["semantic"]
        for field in RATIONALE_FIELDS_PLACEHOLDER:
            value = semantic.get(field)
            if not isinstance(value, str):
                continue
            stripped = value.strip()
            checked += 1
            if PLACEHOLDER_RE.match(stripped):
                placeholder_count += 1
                offenders.append(f"{tid}:{field} placeholder {stripped!r}")
            elif field in RATIONALE_FIELDS_MIN_LEN and len(stripped) < MIN_RATIONALE_CHARS:
                short_count += 1
                offenders.append(
                    f"{tid}:{field} shorter than {MIN_RATIONALE_CHARS} chars ({len(stripped)})"
                )
    detail = (
        f"checked {checked} rationale cells; placeholders={placeholder_count}; "
        f"too_short={short_count} (minimum {MIN_RATIONALE_CHARS} chars for behavior, "
        f"failure_caught, and the four score reasons)"
    )
    status = "failed" if offenders else "passed"
    return check(CHECK_RATIONALE, status, detail, offenders)


def check_sources(model: dict) -> dict:
    offenders = []
    cache = {}
    repo_root = REPO_ROOT.resolve()
    valid = 0
    for row in model["inventory"]:
        tid = row.get("test_id", "") or "<empty test_id>"
        source_file = as_text(row.get("source_file", "")).strip()
        if not source_file:
            offenders.append(f"{tid}:empty source_file")
            continue
        candidate = Path(source_file)
        if candidate.is_absolute():
            offenders.append(f"{tid}:source_file must be repository-relative")
            continue
        resolved = (repo_root / candidate).resolve()
        if resolved != repo_root and repo_root not in resolved.parents:
            offenders.append(f"{tid}:source_file escapes repository ({source_file})")
            continue
        if not resolved.is_file():
            offenders.append(f"{tid}:source_file missing ({source_file})")
            continue
        start, end = as_int(row.get("source_start")), as_int(row.get("source_end"))
        if start is None or end is None:
            offenders.append(f"{tid}:non-integer source range")
            continue
        if resolved not in cache:
            cache[resolved] = len(
                resolved.read_text(encoding="utf-8", errors="replace").splitlines()
            )
        line_count = cache[resolved]
        if start < 1 or end < start or end > line_count:
            offenders.append(
                f"{tid}:range {source_file}:{start}-{end} outside 1..{line_count}"
            )
            continue
        valid += 1
    ref_re = re.compile(r"^(?P<path>\S+?):(?P<start>\d+)-(?P<end>\d+)$")
    ref_single = re.compile(r"^\S+?:\d+$")
    refs_valid = 0
    for tid, entry in sorted(model["merged"].items()):
        refs = entry["semantic"].get("production_refs")
        if refs is None:
            continue
        if not isinstance(refs, list):
            offenders.append(f"{tid}:production_refs is not an array")
            continue
        for item in refs:
            text = as_text(item).strip()
            match = ref_re.match(text)
            if match is None:
                shape = "single-line" if ref_single.match(text) else "malformed"
                offenders.append(
                    f"{tid}:production_ref {text!r} is {shape}; requires path:start-end"
                )
                continue
            ref_path = Path(match.group("path"))
            ref_start, ref_end = int(match.group("start")), int(match.group("end"))
            if ref_path.is_absolute():
                offenders.append(f"{tid}:production_ref {text!r} must be repository-relative")
                continue
            ref_resolved = (repo_root / ref_path).resolve()
            if ref_resolved != repo_root and repo_root not in ref_resolved.parents:
                offenders.append(f"{tid}:production_ref {text!r} escapes repository")
                continue
            if not ref_resolved.is_file():
                offenders.append(f"{tid}:production_ref {text!r} missing")
                continue
            if ref_resolved not in cache:
                cache[ref_resolved] = len(
                    ref_resolved.read_text(encoding="utf-8", errors="replace").splitlines()
                )
            ref_lines = cache[ref_resolved]
            if ref_start < 1 or ref_end < ref_start or ref_end > ref_lines:
                offenders.append(
                    f"{tid}:production_ref {text!r} outside 1..{ref_lines}"
                )
                continue
            refs_valid += 1
    detail = (
        f"validated {valid}/{len(model['inventory'])} source ranges and "
        f"{refs_valid} production_refs against {len(cache)} files under {repo_root}"
    )
    status = "failed" if offenders else "passed"
    return check(CHECK_SOURCES, status, detail, offenders)


def _group_members(group: dict):
    members = group.get("member_test_ids")
    if not isinstance(members, list):
        return None
    return [item for item in members if isinstance(item, str)]


def check_recommendations(model: dict) -> dict:
    offenders = []
    merged = model["merged"]
    inventory_ids = set(model["inventory_by_id"])
    groups = model["groups"] if isinstance(model["groups"], list) else []
    groups_ok = model["groups_error"] is None
    object_groups = [group for group in groups if isinstance(group, dict)]
    member_index = {}
    for group in object_groups:
        members = _group_members(group)
        if members is None:
            continue
        for member in set(members):
            member_index.setdefault(member, []).append(group)

    prune_rows = consolidate_rows = c4_rows = low_rows = 0
    for tid, entry in sorted(merged.items()):
        semantic = entry["semantic"]
        recommendation = semantic.get("recommendation")
        confidence = semantic.get("confidence")
        if confidence not in CONFIDENCES:
            offenders.append(f"{tid}:invalid confidence {confidence!r}")
        if recommendation not in RECOMMENDATIONS:
            offenders.append(f"{tid}:invalid recommendation {recommendation!r}")
            continue
        criticality = semantic.get("criticality")
        scores, _ = compute_scores(semantic)
        usefulness = scores["usefulness_score"] if scores else None
        overlap = semantic.get("overlap_test_ids")
        overlap_list = [item for item in overlap if isinstance(item, str)] if isinstance(overlap, list) else []

        if recommendation == "prune_candidate":
            prune_rows += 1
            if not is_int(criticality) or criticality > 2:
                offenders.append(f"{tid}:prune_candidate with criticality={criticality!r} (requires C<=2)")
            if usefulness is None or usefulness > 25:
                offenders.append(
                    f"{tid}:prune_candidate with usefulness_score={usefulness!r} (requires <=25)"
                )
            if not overlap_list:
                offenders.append(f"{tid}:prune_candidate without overlap_test_ids")
            elif groups_ok:
                qualifying = False
                for group in member_index.get(tid, []):
                    members = set(_group_members(group) or [])
                    if tid in members and set(overlap_list) <= members:
                        qualifying = True
                        break
                if not qualifying:
                    offenders.append(
                        f"{tid}:prune_candidate overlap ids not co-membered in a confirmed group"
                    )
        elif recommendation == "consolidate":
            consolidate_rows += 1
            if not overlap_list:
                offenders.append(f"{tid}:consolidate without overlap_test_ids")
            elif groups_ok and not member_index.get(tid):
                offenders.append(f"{tid}:consolidate without a confirmed group containing the row")
        if is_int(criticality) and criticality == 4:
            c4_rows += 1
        if confidence == "low":
            low_rows += 1

    # Independent-review receipts.
    receipts = model["receipts_by_id"]
    for err in model["receipt_errors"]:
        offenders.append(err)
    for tid in model["duplicate_receipts"]:
        offenders.append(f"{tid}:duplicate quality-review receipt")
    for tid, receipt in sorted(receipts.items()):
        if tid not in inventory_ids:
            offenders.append(f"{tid}:quality-review receipt without inventory row")
            continue
        second_reviewer = receipt.get("second_reviewer")
        if not isinstance(second_reviewer, str) or not second_reviewer.strip():
            offenders.append(f"{tid}:receipt missing second_reviewer")
        verdict = receipt.get("verdict")
        if verdict not in RECEIPT_VERDICTS:
            offenders.append(f"{tid}:receipt invalid verdict {verdict!r}")
        findings = receipt.get("findings")
        findings_ok = (
            (isinstance(findings, str) and findings.strip())
            or (isinstance(findings, (list, dict)) and len(findings) > 0)
        )
        if not findings_ok:
            offenders.append(f"{tid}:receipt without findings")
        if verdict == "resolved":
            if not isinstance(receipt.get("final_review"), dict):
                offenders.append(f"{tid}:resolved receipt without final_review object")
        elif verdict == "agree":
            if receipt.get("final_review") is not None:
                offenders.append(f"{tid}:agree receipt must have final_review null")
        worker = model["reviews_by_id"].get(tid) or {}
        author = as_text(worker.get("reviewer", "")).strip()
        if author and isinstance(second_reviewer, str):
            if second_reviewer.strip() == author:
                offenders.append(f"{tid}:second_reviewer equals reviewer (self-review)")

    required = []
    for tid, entry in sorted(merged.items()):
        semantic = entry["semantic"]
        criticality = semantic.get("criticality")
        if (
            (is_int(criticality) and criticality == 4)
            or semantic.get("recommendation") in REMOVAL
            or semantic.get("confidence") == "low"
        ):
            required.append(tid)
    for tid in required:
        if tid not in receipts:
            offenders.append(f"{tid}:missing required quality-review receipt")

    detail = (
        f"prune_rows={prune_rows}; consolidate_rows={consolidate_rows}; c4_rows={c4_rows}; "
        f"low_confidence_rows={low_rows}; receipts_required={len(required)}; "
        f"receipts_present={len(receipts)}; confirmed_groups={len(object_groups)}"
        + ("" if groups_ok else "; overlap-groups.json unreadable so group gates were not evaluated")
    )
    status = "failed" if offenders else "passed"
    return check(CHECK_GATES, status, detail, offenders)


def check_groups(model: dict) -> dict:
    offenders = []
    if model["groups_error"]:
        return check(
            CHECK_GROUPS, "failed",
            f"evidence/overlap-groups.json: {model['groups_error']}", ["<overlap-groups.json>"],
        )
    groups = model["groups"]
    inventory_ids = set(model["inventory_by_id"])
    merged = model["merged"]

    def recommendation_of(tid):
        entry = merged.get(tid)
        return entry["semantic"].get("recommendation") if entry else None

    seen_group_ids = set()
    group_member_index = {}
    valid_groups = 0
    candidates = sorted(
        tid for tid, entry in merged.items()
        if entry["semantic"].get("recommendation") in REMOVAL
    )

    for index, group in enumerate(groups):
        if not isinstance(group, dict):
            offenders.append(f"group[{index}]:not a JSON object")
            continue
        group_id = group.get("group_id")
        if not isinstance(group_id, str) or not group_id.strip():
            offenders.append(f"group[{index}]:missing group_id")
            group_id = f"group[{index}]"
        elif group_id in seen_group_ids:
            offenders.append(f"{group_id}:duplicate group_id")
            continue
        else:
            seen_group_ids.add(group_id)
        members = group.get("member_test_ids")
        if not isinstance(members, list) or not members:
            offenders.append(f"{group_id}:member_test_ids empty or not an array")
            members = []
        else:
            members = [item for item in members if isinstance(item, str)]
            if len(set(members)) != len(members):
                offenders.append(f"{group_id}:duplicate member_test_ids")
            for member in sorted(set(members)):
                if member not in inventory_ids:
                    offenders.append(f"{group_id}:member does not resolve ({member})")
                else:
                    group_member_index.setdefault(member, []).append(group)

        retained = group.get("retained_test_ids")
        if not isinstance(retained, list) or not retained:
            offenders.append(f"{group_id}:retained_test_ids empty or not an array")
            retained = []
        else:
            retained = [item for item in retained if isinstance(item, str)]
            for target in retained:
                if target not in set(members):
                    offenders.append(f"{group_id}:retained id outside member_test_ids ({target})")
        evidence = group.get("preservation_evidence")
        evidence_ok = (
            (isinstance(evidence, str) and evidence.strip())
            or (isinstance(evidence, (list, dict)) and len(evidence) > 0)
        )
        if not evidence_ok:
            offenders.append(f"{group_id}:preservation_evidence empty")

        resolvable_retained = [target for target in retained if target in inventory_ids]
        safe_retained = [target for target in resolvable_retained if recommendation_of(target) not in REMOVAL]
        if retained and not safe_retained:
            offenders.append(
                f"{group_id}:no retained test outside removal candidates"
            )
        if retained and resolvable_retained and safe_retained:
            valid_groups += 1

    # Every overlap_test_ids entry must resolve to a row and never self-reference.
    for tid, entry in sorted(merged.items()):
        overlap = entry["semantic"].get("overlap_test_ids")
        if not isinstance(overlap, list):
            continue
        for target in overlap:
            if not isinstance(target, str):
                continue
            if target == tid:
                offenders.append(f"{tid}:self-overlap")
            elif target not in inventory_ids:
                offenders.append(f"{tid}:overlap id does not resolve ({target})")

    # Removal candidates must keep a surviving protection chain.
    direct = {tid: set() for tid in candidates}
    for tid in candidates:
        for group in group_member_index.get(tid, []):
            retained = group.get("retained_test_ids")
            if not isinstance(retained, list):
                continue
            for target in retained:
                if target == tid or not isinstance(target, str) or target not in inventory_ids:
                    continue
                direct[tid].add(target)

    safe = set()
    changed = True
    while changed:
        changed = False
        for tid in candidates:
            if tid in safe:
                continue
            for protector in direct[tid]:
                if recommendation_of(protector) not in REMOVAL or protector in safe:
                    safe.add(tid)
                    changed = True
                    break

    dead = [tid for tid in candidates if tid not in safe]
    graph = {
        tid: {p for p in direct[tid] if recommendation_of(p) in REMOVAL}
        for tid in candidates
    }
    cycle = find_cycle([tid for tid in dead], graph)
    for tid in dead:
        offenders.append(f"{tid}:no surviving retained protection")
    if cycle:
        offenders.append("circular sole-survivor protection: " + " -> ".join(cycle))

    detail = (
        f"groups={len(groups)}; structurally_valid_groups={valid_groups}; "
        f"removal_candidates={len(candidates)}; candidates_without_surviving_protection={len(dead)}"
    )
    status = "failed" if offenders else "passed"
    return check(CHECK_GROUPS, status, detail, offenders)


def find_cycle(nodes, graph):
    color = {}
    stack = []
    result = []

    def visit(node):
        color[node] = 1
        stack.append(node)
        for successor in sorted(graph.get(node, ())):
            if successor not in graph:
                continue
            state = color.get(successor, 0)
            if state == 1:
                start = stack.index(successor)
                result.extend(stack[start:] + [successor])
                return True
            if state == 0 and visit(successor):
                return True
        stack.pop()
        color[node] = 2
        return False

    for node in sorted(nodes):
        if color.get(node, 0) == 0 and visit(node):
            return result
    return []


def check_hosted(root: Path, inventory_revision: str = "") -> dict:
    attempts_path = root / "evidence" / "attempts.csv"
    manifest_path = root / "evidence" / "manifest.json"
    log_path = root / "evidence" / "macos-job.log"

    if not attempts_path.is_file():
        return check(
            CHECK_HOSTED, "unverified",
            "evidence/attempts.csv is absent, so the hosted attempt ledger gates "
            "(2822 attempts / 2820 unique tests / 56 binaries / 5931.968s / retry rows) "
            "cannot be recomputed",
            [],
        )

    offenders = []
    notes = []
    try:
        attempts = _read_csv_rows(attempts_path, ATTEMPTS_HEADER, rel(attempts_path, root))
    except InputError as exc:
        return check(CHECK_HOSTED, "failed", str(exc), ["<attempts.csv>"])

    total = len(attempts)
    pairs = set()
    binaries = set()
    cumulative = 0.0
    seconds_errors = []
    for row in attempts:
        pair = (row.get("binary_id", ""), row.get("test_name", ""))
        pairs.add(pair)
        binaries.add(row.get("binary_id", ""))
        try:
            cumulative += float(row.get("seconds", ""))
        except ValueError:
            seconds_errors.append(f"{pair[0]}::{pair[1]}")
    if total != EXPECTED_ATTEMPT_RECORDS:
        offenders.append(f"attempt_records={total} expected {EXPECTED_ATTEMPT_RECORDS}")
    if len(pairs) != EXPECTED_UNIQUE_TESTS:
        offenders.append(f"unique_tests={len(pairs)} expected {EXPECTED_UNIQUE_TESTS}")
    if len(binaries) != EXPECTED_BINARIES:
        offenders.append(f"binaries={len(binaries)} expected {EXPECTED_BINARIES}")
    if abs(cumulative - EXPECTED_CUMULATIVE_SECONDS) > GATE_TOLERANCE:
        offenders.append(
            f"cumulative_seconds={cumulative:.3f} expected {EXPECTED_CUMULATIVE_SECONDS:.3f}"
        )
    for marker in seconds_errors[:25]:
        offenders.append(f"{marker}:unparseable attempt seconds")

    for test_name, first_seconds, total_seconds, attempt_count, failed_count in RETRY_GATES:
        rows = [row for row in attempts if row.get("test_name") == test_name]
        if not rows:
            offenders.append(f"{test_name}:retry rows missing")
            continue
        try:
            ordered = sorted(rows, key=lambda row: int(row.get("attempt", "0")))
        except ValueError:
            offenders.append(f"{test_name}:non-integer attempt number")
            continue
        first = float(ordered[0]["seconds"])
        summed = sum(float(row["seconds"]) for row in ordered)
        failures = sum(1 for row in ordered if "FAIL" in row.get("result", "").upper())
        expected = (first_seconds, total_seconds, attempt_count, failed_count)
        observed = (first, summed, len(ordered), failures)
        if (
            abs(observed[0] - expected[0]) > GATE_TOLERANCE
            or abs(observed[1] - expected[1]) > GATE_TOLERANCE
            or observed[2] != expected[2]
            or observed[3] != expected[3]
        ):
            offenders.append(
                f"{test_name}:first/total/attempts/failed="
                f"{observed[0]:.3f}/{observed[1]:.3f}/{observed[2]}/{observed[3]} "
                f"expected {expected[0]:.3f}/{expected[1]:.3f}/{expected[2]}/{expected[3]}"
            )

    # Log cross-check (presence only; attempts.csv carries the gate values).
    if log_path.is_file():
        normalized = normalize_log(log_path.read_text(encoding="utf-8", errors="replace"))
        if "2820 tests run" not in normalized:
            offenders.append("log summary '2820 tests run' missing")
        for test_name, *_ in RETRY_GATES:
            if test_name not in normalized:
                offenders.append(f"{test_name}:absent from macos-job.log")
    else:
        notes.append("macos-job.log absent; log cross-check skipped")

    # Manifest consistency: revision, recorded hashes, recorded gate claims.
    if manifest_path.is_file():
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except ValueError as exc:
            manifest = None
            offenders.append(f"manifest.json invalid JSON: {exc}")
        if manifest is not None:
            observed_gates = {
                "attempt_records": total,
                "unique_tests": len(pairs),
                "binaries": len(binaries),
                "cumulative_seconds": cumulative,
            }
            offenders.extend(manifest_gate_mismatches(manifest, observed_gates))
            if inventory_revision:
                manifest_revisions = sorted({
                    value.strip()
                    for key, value in flatten_json(manifest)
                    if isinstance(value, str)
                    and value.strip()
                    and re.sub(r"[^a-z0-9]+", "_", key.rsplit(".", 1)[-1].lower()).strip("_")
                    in {"revision", "pinned_revision", "headsha", "head_sha", "commit"}
                })
                if manifest_revisions and inventory_revision not in manifest_revisions:
                    offenders.append(
                        f"manifest revision {manifest_revisions} != inventory {inventory_revision}"
                    )
            verified, missing, mismatched = verify_manifest_hashes(manifest, root)
            offenders.extend(mismatched)
            if missing:
                notes.append(f"manifest hashes not checked for {len(missing)} absent file(s)")
            if verified:
                notes.append(f"manifest sha256 verified for {verified} file(s)")
    else:
        notes.append("manifest.json absent; revision/hash/gate cross-checks unverified")
    if attempts:
        notes.append(f"attempt ledger rows={total}")

    detail = "hosted gates recomputed from evidence/attempts.csv: " + ", ".join(
        [
            f"attempts={total}",
            f"unique_tests={len(pairs)}",
            f"binaries={len(binaries)}",
            f"cumulative_seconds={cumulative:.3f}",
        ]
    )
    if notes:
        detail += "; " + "; ".join(notes)
    status = "failed" if offenders else "passed"
    return check(CHECK_HOSTED, status, detail, offenders)


def normalize_log(text: str) -> str:
    text = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", text)
    text = re.sub(r"\^\[\[[0-9;]*m", "", text)
    text = re.sub(r"^[^\t]*\t[^\t]*\t\d{4}-\d\d-\d\dT[0-9:.]+Z ", "", text, flags=re.MULTILINE)
    return text


def flatten_json(obj, prefix=""):
    if isinstance(obj, dict):
        for key, value in obj.items():
            yield from flatten_json(value, f"{prefix}.{key}" if prefix else str(key))
    elif isinstance(obj, list):
        for index, value in enumerate(obj):
            yield from flatten_json(value, f"{prefix}[{index}]")
    else:
        yield prefix, obj


GATE_KEY_NAMES = {
    "attempt_records": "attempt_records",
    "attempts": "attempt_records",
    "attempt_rows": "attempt_records",
    "attempt_total": "attempt_records",
    "total_attempts": "attempt_records",
    "total_attempt_records": "attempt_records",
    "attempt_count": "attempt_records",
    "unique_tests": "unique_tests",
    "unique_test_ids": "unique_tests",
    "unique_executed_tests": "unique_tests",
    "executed_tests": "unique_tests",
    "unique_ids": "unique_tests",
    "unique_rust_ids": "unique_tests",
    "unique_rust_tests": "unique_tests",
    "unique_rust_test_ids": "unique_tests",
    "binaries": "binaries",
    "binary_ids": "binaries",
    "binary_count": "binaries",
    "binaries_count": "binaries",
    "unique_binaries": "binaries",
    "cumulative_seconds": "cumulative_seconds",
    "cumulative_test_seconds": "cumulative_seconds",
    "sum_seconds": "cumulative_seconds",
    "accumulated_seconds": "cumulative_seconds",
    "total_test_seconds": "cumulative_seconds",
}


def manifest_gate_mismatches(manifest, observed):
    offenders = []
    matched = 0
    for key_path, value in flatten_json(manifest):
        leaf = key_path.rsplit(".", 1)[-1]
        leaf = re.sub(r"[^a-z0-9]+", "_", leaf.lower()).strip("_")
        gate = GATE_KEY_NAMES.get(leaf)
        if not gate or not isinstance(value, (int, float)) or isinstance(value, bool):
            continue
        matched += 1
        expected = observed[gate]
        if isinstance(expected, float):
            if abs(float(value) - expected) > GATE_TOLERANCE:
                offenders.append(
                    f"manifest {key_path}={value!r} != recomputed {expected:.3f}"
                )
        elif int(value) != int(expected):
            offenders.append(f"manifest {key_path}={value!r} != recomputed {expected}")
    # Retry claims recorded as structured lists.
    for entry in iter_json_dicts(manifest):
        name = next(
            (entry[key] for key in ("test", "test_name", "name", "test_id", "id")
             if isinstance(entry.get(key), str)),
            None,
        )
        if not name:
            continue
        gate = next((gate for gate in RETRY_GATES if gate[0] in name or name in gate[0]), None)
        if gate is None:
            continue
        pairs = [
            (("first", "first_attempt_seconds", "first_seconds"), gate[1]),
            (("total", "total_attempt_seconds", "total_seconds"), gate[2]),
            (("attempts", "attempt_count"), gate[3]),
            (("failed", "failed_attempts", "failed_count"), gate[4]),
        ]
        for keys, expected in pairs:
            for key in keys:
                if key in entry and isinstance(entry[key], (int, float)):
                    value = entry[key]
                    if isinstance(expected, float):
                        if abs(float(value) - expected) > GATE_TOLERANCE:
                            offenders.append(
                                f"manifest retry {name} {key}={value!r} expected {expected}"
                            )
                    elif int(value) != int(expected):
                        offenders.append(
                            f"manifest retry {name} {key}={value!r} expected {expected}"
                        )
                    break
    return offenders


def iter_json_dicts(obj):
    if isinstance(obj, dict):
        yield obj
        for value in obj.values():
            yield from iter_json_dicts(value)
    elif isinstance(obj, list):
        for value in obj:
            yield from iter_json_dicts(value)


def collect_hash_records(obj, prefix=""):
    """Yield (path, sha256) pairs from common manifest shapes."""
    if isinstance(obj, dict):
        digest = obj.get("sha256")
        if isinstance(digest, str):
            path = next(
                (obj[key] for key in ("path", "file", "relative_path", "name", "filename")
                 if isinstance(obj.get(key), str)),
                prefix,
            )
            if isinstance(path, str) and path:
                yield path, digest
        for key, value in obj.items():
            if isinstance(value, str):
                continue
            if isinstance(value, dict) and isinstance(value.get("sha256"), str):
                yield str(key), value["sha256"]
            yield from collect_hash_records(value, str(key))
    elif isinstance(obj, list):
        for index, value in enumerate(obj):
            yield from collect_hash_records(value, f"{prefix}[{index}]")


def verify_manifest_hashes(manifest, root: Path):
    repo_root = REPO_ROOT
    verified = 0
    missing = []
    mismatched = []
    seen = set()
    for relative, digest in collect_hash_records(manifest):
        if not re.search(r"\.(json|jsonl|csv|log|txt|md)$", relative):
            continue
        candidates = [root / relative, repo_root / relative, root / "evidence" / Path(relative).name]
        target = next((path for path in candidates if path.is_file()), None)
        if target is None:
            missing.append(relative)
            continue
        resolved = target.resolve()
        if resolved != repo_root and repo_root not in resolved.parents:
            missing.append(f"{relative} (outside repository)")
            continue
        key = (str(target), digest.lower())
        if key in seen:
            continue
        seen.add(key)
        actual = sha256_file(target)
        if actual.lower() != digest.lower():
            mismatched.append(
                f"manifest sha256 mismatch for {rel(target, repo_root)}: "
                f"recorded {digest} actual {actual}"
            )
        else:
            verified += 1
    return verified, missing, mismatched


# --------------------------------------------------------------------------------------
# Assembly
# --------------------------------------------------------------------------------------

def build_output_rows(model: dict):
    rows = []
    problems = []
    receipts = model["receipts_by_id"]
    for inventory_row in model["inventory"]:
        tid = inventory_row.get("test_id", "")
        entry = model["merged"].get(tid)
        if entry is None:
            problems.append(f"{tid}:no semantic record")
            continue
        semantic = entry["semantic"]
        scores, bad_field = compute_scores(semantic)
        if scores is None:
            problems.append(f"{tid}:{bad_field} invalid")
            continue
        receipt = receipts.get(tid) or {}
        second_reviewer = receipt.get("second_reviewer")
        cells = {}
        for column in INVENTORY_HEADER:
            if column in IDENTITY_COLUMNS or column in {
                "test_id", "first_attempt_seconds", "total_attempt_seconds",
                "attempt_count", "failed_attempt_count", "timing_basis", "timing_source",
            }:
                cells[column] = as_text(inventory_row.get(column, ""))
        production_refs = semantic.get("production_refs")
        overlap = semantic.get("overlap_test_ids")
        cells["test_level"] = as_text(semantic.get("test_level"))
        cells["production_refs"] = json.dumps(
            production_refs if isinstance(production_refs, list) else [], ensure_ascii=False
        )
        for field in (
            "behavior", "failure_caught",
            "criticality_reason", "signal_reason", "uniqueness_reason", "fidelity_reason",
            "confidence", "recommendation", "recommendation_reason", "reviewer",
        ):
            cells[field] = as_text(semantic.get(field))
        cells["cost_drivers"] = serialize_cost_drivers(semantic.get("cost_drivers"))
        for field in SCORE_DIMENSIONS:
            cells[field] = str(semantic.get(field))
        for field, value in scores.items():
            cells[field] = str(value)
        cells["overlap_test_ids"] = json.dumps(
            overlap if isinstance(overlap, list) else [], ensure_ascii=False
        )
        cells["second_reviewer"] = as_text(second_reviewer)
        rows.append(cells)
    rows.sort(key=lambda row: (int(row["retention_score"]), row["test_id"]))
    return rows, problems


def render_csv(rows) -> str:
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(OUTPUT_HEADER)
    for row in rows:
        writer.writerow([row.get(column, "") for column in OUTPUT_HEADER])
    return buffer.getvalue()


def parse_csv_text(text: str):
    reader = csv.reader(io.StringIO(text, newline=""))
    rows = list(reader)
    return rows[0], rows[1:]


def check_roundtrip(rows, model: dict, problems) -> dict:
    if problems:
        return check(
            CHECK_ROUNDTRIP, "unverified",
            f"{len(problems)} row(s) could not be assembled, so round-trip was not asserted; "
            f"first: {problems[0]}",
            [problem.split(":", 1)[0] for problem in problems[:25]],
        )
    text = render_csv(rows)
    if "\r" in text:
        return check(CHECK_ROUNDTRIP, "failed", "rendered CSV contains CR; LF-only required", ["<csv>"])
    header, parsed = parse_csv_text(text)
    offenders = []
    if header != OUTPUT_HEADER:
        offenders.append("<header>:mismatch")
    expected = [[row.get(column, "") for column in OUTPUT_HEADER] for row in rows]
    if parsed != expected:
        for index, (left, right) in enumerate(zip(parsed, expected)):
            if left != right:
                offenders.append(f"{rows[index]['test_id']}:cell mismatch")
                break
        if len(parsed) != len(expected):
            offenders.append(f"<rows>:{len(parsed)} != {len(expected)}")
    detail = f"in-memory render parsed back equal for {len(rows)} rows, {len(OUTPUT_HEADER)} columns"
    status = "failed" if offenders else "passed"
    return check(CHECK_ROUNDTRIP, status, detail, offenders)


def check_file_roundtrip(path: Path, rows, base: Path) -> dict:
    raw = path.read_bytes()
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        return check(CHECK_ROUNDTRIP, "failed", f"written CSV is not UTF-8: {exc}", ["<csv>"])
    header, parsed = parse_csv_text(text)
    expected = [[row.get(column, "") for column in OUTPUT_HEADER] for row in rows]
    offenders = []
    if header != OUTPUT_HEADER:
        offenders.append("<header>:mismatch")
    if parsed != expected:
        for index in range(min(len(parsed), len(expected))):
            if parsed[index] != expected[index]:
                offenders.append(f"{rows[index]['test_id']}:cell mismatch")
                break
        if len(parsed) != len(expected):
            offenders.append(f"<rows>:{len(parsed)} != {len(expected)}")
    if b"\r" in raw:
        offenders.append("<csv>:contains CR")
    detail = (
        f"read back {rel(path, base)} ({len(raw)} bytes, sha256 {sha256_bytes(raw)[:16]}…) "
        f"and matched all {len(rows)} rows × {len(OUTPUT_HEADER)} cells"
    )
    status = "failed" if offenders else "passed"
    return check(CHECK_ROUNDTRIP, status, detail, offenders)


def check_order(rows) -> dict:
    keys = [(int(row["retention_score"]), row["test_id"]) for row in rows]
    if keys != sorted(keys):
        for index in range(1, len(keys)):
            if keys[index - 1] > keys[index]:
                return check(
                    CHECK_ORDER, "failed",
                    "rows are not sorted by (retention_score, test_id) ascending",
                    [keys[index - 1][1], keys[index][1]],
                )
    return check(
        CHECK_ORDER, "passed",
        f"{len(rows)} rows sorted by (retention_score, test_id) ascending",
        [],
    )


def aggregate_status(checks) -> str:
    statuses = {entry["status"] for entry in checks}
    if "failed" in statuses:
        return "failed"
    if "unverified" in statuses:
        return "complete-with-evidence-gaps"
    return "complete"


def row_counts(model: dict, rows) -> dict:
    by_runner, by_step, by_variant, by_recommendation, by_level, by_kind = {}, {}, {}, {}, {}, {}
    for row in rows:
        for bucket, key in (
            (by_runner, row.get("runner", "")),
            (by_step, row.get("ci_step", "")),
            (by_variant, row.get("variant", "")),
            (by_recommendation, row.get("recommendation", "")),
            (by_level, row.get("test_level", "")),
            (by_kind, row.get("target_kind", "")),
        ):
            bucket[key] = bucket.get(key, 0) + 1
    return {
        "inventory_rows": len(model["inventory"]),
        "csv_rows": len(rows),
        "by_runner": dict(sorted(by_runner.items())),
        "by_ci_step": dict(sorted(by_step.items())),
        "by_variant": dict(sorted(by_variant.items())),
        "by_recommendation": dict(sorted(by_recommendation.items())),
        "by_test_level": dict(sorted(by_level.items())),
        "by_target_kind": dict(sorted(by_kind.items())),
    }


def evaluate(model: dict, *, file_path=None):
    checks = [
        check_rowcounts_and_join(model),
        check_scores(model),
        check_rationale(model),
        check_sources(model),
        check_recommendations(model),
        check_groups(model),
        check_hosted(model["root"], revision_of(model)),
    ]
    rows, problems = build_output_rows(model)
    checks.append(check_roundtrip(rows, model, problems))
    checks.append(check_order(rows))
    return checks, rows, problems


def revision_of(model: dict) -> str:
    values = sorted({
        as_text(row.get("revision", "")).strip()
        for row in model["inventory"] if as_text(row.get("revision", "")).strip()
    })
    return values[0] if values else ""


def score_summary(rows) -> dict:
    summary = {}
    for field in DERIVED_SCORES:
        values = sorted(int(row[field]) for row in rows)
        if not values:
            summary[field] = {"min": None, "median": None, "max": None}
            continue
        count = len(values)
        middle = count // 2
        median = values[middle] if count % 2 else (values[middle - 1] + values[middle]) / 2
        summary[field] = {"min": values[0], "median": median, "max": values[-1]}
    return summary


def build_validation(model: dict, checks, rows, status: str, csv_info=None, mode="assemble") -> dict:
    return {
        "schema": "macos-test-review/validation@1",
        "revision": revision_of(model),
        "status": status,
        "mode": mode,
        "command_line": " ".join([sys.executable, "-I"] + sys.argv),
        "command_argv": list(sys.argv),
        "inventory_source": model["inventory_source"],
        "input_hashes": model["input_hashes"],
        "row_counts": row_counts(model, rows),
        "score_summary": score_summary(rows),
        "weights": {
            "criticality_score": "25*C",
            "usefulness_score": "10*S + 10*U + 5*F",
            "retention_score": "10*C + 6*S + 6*U + 3*F",
        },
        "quality_review": {
            "receipts": model["receipt_count"],
            "calibration": model["calibration_note"],
        },
        "notes": model["notes"],
        "csv": csv_info,
        "checks": checks,
    }


def print_report(checks, status, rows, model, output_path=None, stream=None):
    stream = stream or sys.stdout
    for entry in checks:
        stream.write(f"[{entry['status'].upper():10}] {entry['check']}\n")
        if entry["status"] != "passed" or entry["offending_ids"]:
            stream.write(f"           {entry['detail']}\n")
        if entry["offending_ids"]:
            stream.write(f"           offenders: {ids_summary(entry['offending_ids'])}\n")
    stream.write(
        f"status: {status}; rows: {len(rows)}; revision: {revision_of(model) or '<none>'}\n"
    )
    if output_path is not None:
        stream.write(f"wrote: {output_path}\n")
    if status == "complete-with-evidence-gaps":
        stream.write(
            "note: complete-with-evidence-gaps means structural validation passed; "
            "hosted-evidence assertions were unavailable (see check 7).\n"
        )


# --------------------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------------------

def build_audit_details(model: dict, checks) -> dict:
    merged = model["merged"]
    receipts = model["receipts_by_id"]
    review_sources = model["review_sources"]
    receipt_sources = model["receipt_sources"]
    groups = model["groups"] if isinstance(model["groups"], list) else []
    member_index = {}
    for group in groups:
        if not isinstance(group, dict):
            continue
        members = group.get("member_test_ids")
        if not isinstance(members, list):
            continue
        for member in set(item for item in members if isinstance(item, str)):
            member_index.setdefault(member, []).append(group)

    def shard_id(test_id):
        source = review_sources.get(test_id) or {}
        name = source.get("file") or ""
        return Path(name).stem

    no_protection = set()
    for entry in checks:
        for offender in entry["offending_ids"]:
            if offender.endswith(":no surviving retained protection"):
                no_protection.add(offender[: -len(":no surviving retained protection")])

    required = sorted(
        test_id for test_id, entry in merged.items()
        if (
            (is_int(entry["semantic"].get("criticality")) and entry["semantic"].get("criticality") == 4)
            or entry["semantic"].get("recommendation") in REMOVAL
            or entry["semantic"].get("confidence") == "low"
        )
    )

    missing_receipt = []
    for test_id in required:
        if test_id in receipts:
            continue
        semantic = merged[test_id]["semantic"]
        source = review_sources.get(test_id) or {}
        worker = model["reviews_by_id"].get(test_id) or {}
        missing_receipt.append({
            "test_id": test_id,
            "shard_id": shard_id(test_id),
            "review_file": source.get("file"),
            "review_line": source.get("line"),
            "author_reviewer": worker.get("reviewer"),
            "reviewer": semantic.get("reviewer"),
            "criticality": semantic.get("criticality"),
            "recommendation": semantic.get("recommendation"),
            "confidence": semantic.get("confidence"),
        })

    self_review = []
    for test_id, receipt in sorted(receipts.items()):
        worker = model["reviews_by_id"].get(test_id) or {}
        author = as_text(worker.get("reviewer", "")).strip()
        second = as_text(receipt.get("second_reviewer", "")).strip()
        if author and second and second == author:
            self_review.append({
                "test_id": test_id,
                "reviewer": author,
                "second_reviewer": second,
                "receipt_files": sorted({src["file"] for src in receipt_sources.get(test_id, [])}),
            })

    duplicate_receipt = []
    duplicate_sources = model.get("segment_receipt_sources") or receipt_sources
    for test_id, sources in sorted(duplicate_sources.items()):
        if len(sources) < 2:
            continue
        merged_receipt = receipts.get(test_id) or {}
        duplicate_receipt.append({
            "test_id": test_id,
            "receipt_count": len(sources),
            "receipt_files": sorted({src["file"] for src in sources}),
            "receipt_locations": sources,
            "merged_kept_second_reviewer": merged_receipt.get("second_reviewer"),
            "merged_kept_verdict": merged_receipt.get("verdict"),
        })

    consolidate_rows = []
    for test_id, entry in sorted(merged.items()):
        semantic = entry["semantic"]
        if semantic.get("recommendation") != "consolidate":
            continue
        overlap = semantic.get("overlap_test_ids")
        has_group = bool(member_index.get(test_id))
        worker = model["reviews_by_id"].get(test_id) or {}
        consolidate_rows.append({
            "test_id": test_id,
            "shard_id": shard_id(test_id),
            "author_reviewer": worker.get("reviewer"),
            "overlap_test_ids": overlap if isinstance(overlap, list) else [],
            "reviewer": semantic.get("reviewer"),
            "criticality": semantic.get("criticality"),
            "confirmed_group_containing_row": has_group,
            "surviving_protection": test_id not in no_protection,
        })

    self_overlap = []
    for test_id, entry in sorted(merged.items()):
        overlap = entry["semantic"].get("overlap_test_ids")
        if isinstance(overlap, list) and test_id in overlap:
            self_overlap.append({"test_id": test_id, "overlap_test_ids": overlap})

    consolidate_ids = {item["test_id"] for item in consolidate_rows}
    covered = {
        "missing required quality-review receipt": {item["test_id"] for item in missing_receipt},
        "second_reviewer equals reviewer": {item["test_id"] for item in self_review},
        "duplicate quality-review receipt": {item["test_id"] for item in duplicate_receipt},
        "consolidate without": consolidate_ids,
        "self-overlap": {item["test_id"] for item in self_overlap},
        "no surviving retained protection": consolidate_ids,
    }
    other_offenders = []
    for entry in checks:
        for offender in entry["offending_ids"]:
            mapped = False
            for marker, ids in covered.items():
                if offender.endswith(marker):
                    if offender[: -len(marker)].rstrip(":") in ids:
                        mapped = True
                    break
                if marker in offender and offender.split(marker, 1)[0].rstrip(":") in ids:
                    mapped = True
                    break
            if not mapped:
                other_offenders.append({"check": entry["check"], "offender": offender})

    return {
        "schema": "macos-test-review/audit-details@1",
        "revision": revision_of(model),
        "inventory_source": model["inventory_source"],
        "receipt_input_files": model["receipt_files_used"],
        "segment_files_scanned": model.get("segment_files_used", []),
        "notes": model["notes"],
        "check_statuses": [
            {"check": entry["check"], "status": entry["status"], "detail": entry["detail"]}
            for entry in checks
        ],
        "counts": {
            "missing_required_receipt": len(missing_receipt),
            "self_review": len(self_review),
            "duplicate_receipt": len(duplicate_receipt),
            "consolidate_rows": len(consolidate_rows),
            "consolidate_without_group": sum(
                1 for item in consolidate_rows if not item["confirmed_group_containing_row"]
            ),
            "self_overlap": len(self_overlap),
            "other_offenders": len(other_offenders),
        },
        "missing_required_receipt": missing_receipt,
        "self_review": self_review,
        "duplicate_receipt": duplicate_receipt,
        "consolidate_rows": consolidate_rows,
        "self_overlap": self_overlap,
        "other_offenders": other_offenders,
    }


def run_audit() -> int:
    try:
        model = load_model(
            BASE, require_groups=False, merge_segmented_receipts=True,
            scan_receipt_segments=True,
        )
    except InputError as exc:
        sys.stderr.write(f"error: {exc}\nNo audit written.\n")
        return 2
    checks, rows, problems = evaluate(model)
    audit = build_audit_details(model, checks)
    path = BASE / "evidence" / "audit-details.json"
    write_atomic(path, json.dumps(audit, indent=2, ensure_ascii=False) + "\n")
    for entry in checks:
        print(f"[{entry['status'].upper():10}] {entry['check']}")
    print("counts:", json.dumps(audit["counts"], sort_keys=True))
    print(f"wrote: {rel(path, BASE)}")
    return 0


def run_assemble(check_only: bool, root: Path = BASE) -> int:
    try:
        model = load_model(root)
    except InputError as exc:
        sys.stderr.write(f"error: {exc}\nNo output was written.\n")
        return 2

    checks, rows, problems = evaluate(model)
    status = aggregate_status(checks)

    csv_path = root / "tests.csv"
    csv_info = None
    if not check_only and status == "failed" and csv_path.exists():
        csv_path.unlink()
        sys.stderr.write(
            "note: existing tests.csv removed because this assembly failed validation "
            "(a stale report is not left in place).\n"
        )
    if not check_only and status != "failed" and not problems:
        text = render_csv(rows)
        write_atomic(csv_path, text)
        file_check = check_file_roundtrip(csv_path, rows, root)
        index = next(
            i for i, entry in enumerate(checks) if entry["check"] == CHECK_ROUNDTRIP
        )
        checks[index] = file_check
        status = aggregate_status(checks)
        if status == "failed":
            try:
                csv_path.unlink()
            except OSError:
                pass
            sys.stderr.write(
                "error: tests.csv failed post-write verification and was removed.\n"
            )
        else:
            raw = csv_path.read_bytes()
            csv_info = {
                "path": rel(csv_path, BASE),
                "rows": len(rows),
                "sha256": sha256_bytes(raw),
                "bytes": len(raw),
            }

    if check_only:
        print_report(checks, status, rows, model)
        return 0 if status != "failed" else 1

    validation = build_validation(model, checks, rows, status, csv_info)
    validation_path = root / "evidence" / "validation.json"
    write_atomic(
        validation_path,
        json.dumps(validation, indent=2, ensure_ascii=False) + "\n",
    )
    print_report(checks, status, rows, model, output_path=csv_path if csv_info else None)
    sys.stderr.write(f"wrote: {rel(validation_path, root)}\n")
    return 0 if status != "failed" else 1


# --------------------------------------------------------------------------------------
# Selftest fixtures
# --------------------------------------------------------------------------------------

def fixture_inventory_row(test_id, **overrides):
    row = {
        "test_id": test_id,
        "parent_test_id": "",
        "revision": FIXTURE_REVISION,
        "ci_step": "Run macOS checks",
        "runner": "nextest",
        "suite": "ovrcr::bin/ovrcr",
        "target_kind": "integration",
        "test_name": "module::case",
        "variant": "default",
        "source_file": "Cargo.toml",
        "source_start": "1",
        "source_end": "3",
        "first_attempt_seconds": "0.010",
        "total_attempt_seconds": "0.010",
        "attempt_count": "1",
        "failed_attempt_count": "0",
        "timing_basis": "hosted-single-run",
        "timing_source": "fixture ledger line 1",
    }
    row.update(overrides)
    return row


def fixture_review(test_id, **overrides):
    record = {
        "test_id": test_id,
        "test_level": "unit",
        "production_refs": ["Cargo.toml:1-3"],
        "behavior": "Asserts the concrete behavior of the production path under review.",
        "failure_caught": "A wrong branch fails the state assertion and is detected.",
        "criticality": 2,
        "signal": 3,
        "uniqueness": 3,
        "fidelity": 3,
        "criticality_reason": "Bounded feature failure with a straightforward workaround.",
        "signal_reason": "Boundary assertions reject the plausible wrong state.",
        "uniqueness_reason": "Distinct failure mode from reviewed neighboring coverage.",
        "fidelity_reason": "Real component through its public API with a test fixture.",
        "confidence": "high",
        "recommendation": "keep",
        "recommendation_reason": "Grounded coverage of a distinct boundary; retain it.",
        "overlap_test_ids": [],
        "cost_drivers": "in-process fixture; no fixed sleeps",
        "reviewer": "fixture-worker",
    }
    record.update(overrides)
    return record


def write_fixture(root: Path, inventory_rows, review_rows, groups, receipts=None):
    (root / "evidence").mkdir(parents=True, exist_ok=True)
    (root / "reviews").mkdir(parents=True, exist_ok=True)
    inventory_text = io.StringIO()
    writer = csv.writer(inventory_text, lineterminator="\n")
    writer.writerow(INVENTORY_HEADER)
    for row in inventory_rows:
        writer.writerow([row.get(column, "") for column in INVENTORY_HEADER])
    (root / "evidence" / "inventory.csv").write_text(inventory_text.getvalue(), encoding="utf-8")
    review_text = "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in review_rows)
    (root / "reviews" / "shard-a.jsonl").write_text(review_text, encoding="utf-8")
    (root / "evidence" / "overlap-groups.json").write_text(
        json.dumps(groups, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    if receipts is not None:
        receipt_text = "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in receipts)
        (root / "evidence" / "quality-review.jsonl").write_text(receipt_text, encoding="utf-8")


def fixture_snapshot(base: Path, skip: Path):
    snapshot = {}
    for path in sorted(base.rglob("*")):
        if not path.is_file():
            continue
        if path == skip or skip in path.parents:
            continue
        stat = path.stat()
        snapshot[str(path.relative_to(base))] = (stat.st_size, stat.st_mtime_ns)
    return snapshot


def run_selftest() -> int:
    selftest_root = BASE / "evidence" / "selftest"
    before = fixture_snapshot(BASE, selftest_root)
    if selftest_root.exists():
        shutil.rmtree(selftest_root)
    selftest_root.mkdir(parents=True)

    results = []

    def execute(name, builder, expected_check=None, expected_ids=(), expect_pass=False,
                extra_checks=None, expected_label=None):
        root = selftest_root / name
        builder(root)
        try:
            model = load_model(root)
        except InputError as exc:
            results.append((name, "load fixture", f"InputError: {exc}", "FAIL"))
            return
        checks, rows, problems = evaluate(model)
        status = aggregate_status(checks)
        failing = [entry for entry in checks if entry["status"] == "failed"]
        observed = status
        if failing:
            observed += "; failing=" + ", ".join(entry["check"] for entry in failing)
            observed += "; offenders=" + ids_summary(
                [oid for entry in failing for oid in entry["offending_ids"]], limit=8
            )
        else:
            observed += "; structural checks passed"
        ok = True
        if expect_pass:
            if status not in {"complete", "complete-with-evidence-gaps"}:
                ok = False
            structural = {
                entry["check"]: entry["status"] for entry in checks
                if entry["check"] != CHECK_HOSTED
            }
            if any(value != "passed" for value in structural.values()):
                ok = False
            if problems:
                ok = False
            if extra_checks is not None and not extra_checks(checks, rows, root):
                ok = False
        else:
            if status != "failed":
                ok = False
            matching = [entry for entry in failing if entry["check"] == expected_check]
            if not matching:
                ok = False
            else:
                present = set()
                for offender in matching[0]["offending_ids"]:
                    present.add(offender)
                    present.add(offender.split(":", 1)[0])
                if not set(expected_ids) <= present:
                    ok = False
        label = expected_label or expected_description(expected_check, expected_ids, expect_pass)
        results.append((name, label, observed, "PASS" if ok else "FAIL"))

    # 1. duplicate test_id
    def build_duplicate(root):
        write_fixture(
            root,
            [fixture_inventory_row("dup-id"), fixture_inventory_row("dup-id", test_name="module::other")],
            [fixture_review("dup-id")],
            [],
        )
    execute("duplicate-test-id", build_duplicate, CHECK_ROWCOUNTS, ["dup-id"])

    # 2. missing review
    def build_missing_review(root):
        write_fixture(
            root,
            [fixture_inventory_row("has-review"), fixture_inventory_row("no-review")],
            [fixture_review("has-review")],
            [],
        )
    execute("missing-review", build_missing_review, CHECK_ROWCOUNTS, ["no-review"])

    # 3. invalid score
    def build_invalid_score(root):
        write_fixture(
            root,
            [fixture_inventory_row("bad-score")],
            [fixture_review("bad-score", criticality=5)],
            [],
        )
    execute("invalid-score", build_invalid_score, CHECK_SCORES, ["bad-score"])

    # 3b. invalid cost_drivers shapes are rejected by ID (empty forms are allowed)
    def build_invalid_cost_drivers(root):
        write_fixture(
            root,
            [
                fixture_inventory_row("bad-cost-empty-element"),
                fixture_inventory_row("bad-cost-wrong-type"),
            ],
            [
                fixture_review("bad-cost-empty-element", cost_drivers=["valid driver", ""]),
                fixture_review("bad-cost-wrong-type", cost_drivers=42),
            ],
            [],
        )
    execute(
        "invalid-cost-drivers", build_invalid_cost_drivers, CHECK_ROWCOUNTS,
        ["bad-cost-empty-element", "bad-cost-wrong-type"],
    )

    # 3c. a receipt claiming the author as its own checker fails provenance
    def build_self_review(root):
        write_fixture(
            root,
            [fixture_inventory_row("self-review-row")],
            [fixture_review("self-review-row")],
            [],
            receipts=[{
                "test_id": "self-review-row",
                "second_reviewer": "fixture-worker",
                "verdict": "agree",
                "findings": "Recorded by the author instead of an independent checker.",
                "final_review": None,
            }],
        )
    execute(
        "self-review-provenance", build_self_review, CHECK_ROWCOUNTS,
        ["self-review-row"],
    )

    # 4. formula mismatch supplied by an adjudicated final_review
    def build_formula_mismatch(root):
        bad = fixture_review("bad-formula")
        final = dict(bad)
        final["retention_score"] = 999
        write_fixture(
            root,
            [fixture_inventory_row("bad-formula")],
            [bad],
            [],
            receipts=[{
                "test_id": "bad-formula",
                "second_reviewer": "independent-reviewer",
                "verdict": "resolved",
                "findings": "Recomputed scores disagree with the supplied retention_score.",
                "final_review": final,
            }],
        )
    execute("formula-mismatch", build_formula_mismatch, CHECK_SCORES, ["bad-formula"])

    # 5. dangling overlap id
    def build_dangling_overlap(root):
        write_fixture(
            root,
            [fixture_inventory_row("dangling")],
            [fixture_review("dangling", overlap_test_ids=["ghost-id"])],
            [],
        )
    execute("dangling-overlap", build_dangling_overlap, CHECK_GROUPS, ["dangling"])

    # 6. missing required second review (prune, C=4, low confidence)
    def build_missing_receipt(root):
        write_fixture(
            root,
            [
                fixture_inventory_row("prune-no-receipt"),
                fixture_inventory_row("c4-no-receipt"),
                fixture_inventory_row("low-no-receipt"),
                fixture_inventory_row("protector"),
            ],
            [
                fixture_review(
                    "prune-no-receipt", criticality=1, signal=1, uniqueness=1, fidelity=1,
                    recommendation="prune_candidate", overlap_test_ids=["protector"],
                ),
                fixture_review(
                    "c4-no-receipt", criticality=4, signal=4, uniqueness=4, fidelity=4,
                ),
                fixture_review("low-no-receipt", confidence="low", recommendation="investigate"),
                fixture_review("protector", criticality=2, signal=2, uniqueness=2, fidelity=2),
            ],
            [{
                "group_id": "g-protect",
                "member_test_ids": ["prune-no-receipt", "protector"],
                "retained_test_ids": ["protector"],
                "preservation_evidence": "protector asserts the same boundary at Cargo.toml:1-3",
            }],
        )
    execute(
        "missing-second-review", build_missing_receipt, CHECK_GATES,
        ["prune-no-receipt", "c4-no-receipt", "low-no-receipt"],
    )

    # 7. circular sole-survivor overlap group
    def build_cycle(root):
        write_fixture(
            root,
            [fixture_inventory_row("cycle-a"), fixture_inventory_row("cycle-b")],
            [
                fixture_review(
                    "cycle-a", criticality=1, signal=1, uniqueness=1, fidelity=1,
                    recommendation="prune_candidate", overlap_test_ids=["cycle-b"],
                ),
                fixture_review(
                    "cycle-b", criticality=1, signal=1, uniqueness=1, fidelity=1,
                    recommendation="prune_candidate", overlap_test_ids=["cycle-a"],
                ),
            ],
            [
                {
                    "group_id": "g-a",
                    "member_test_ids": ["cycle-a", "cycle-b"],
                    "retained_test_ids": ["cycle-b"],
                    "preservation_evidence": "cycle-b claims to preserve cycle-a's assertion",
                },
                {
                    "group_id": "g-b",
                    "member_test_ids": ["cycle-a", "cycle-b"],
                    "retained_test_ids": ["cycle-a"],
                    "preservation_evidence": "cycle-a claims to preserve cycle-b's assertion",
                },
            ],
            receipts=[
                {
                    "test_id": tid,
                    "second_reviewer": f"independent-{tid}",
                    "verdict": "agree",
                    "findings": "Independently confirmed the overlap claim.",
                    "final_review": None,
                }
                for tid in ("cycle-a", "cycle-b")
            ],
        )
    execute("circular-sole-survivor", build_cycle, CHECK_GROUPS, ["cycle-a", "cycle-b"])

    # 8. passing fixture with comma, quote, newline, and non-ASCII cells
    tricky_behavior = 'Comma, double quote " and a newline\nin one cell; emoji ✅ / café.'
    tricky_failure = 'A wrong branch would emit "café,✅" instead of the two-line state.'
    pass_state = {}

    def build_pass(root):
        missing_cost = fixture_review("pass-missing-cost")
        missing_cost.pop("cost_drivers", None)
        consolidate_review = fixture_review(
            "pass-consolidate", criticality=1, signal=2, uniqueness=1, fidelity=2,
            recommendation="consolidate", overlap_test_ids=["pass-keeper"],
            cost_drivers=["two feeds", "setup() per case ✅"],
        )
        adjudicated = dict(consolidate_review)
        adjudicated["reviewer"] = "independent-reviewer"
        write_fixture(
            root,
            [
                fixture_inventory_row("pass-main"),
                fixture_inventory_row("pass-consolidate"),
                fixture_inventory_row("pass-keeper"),
                fixture_inventory_row("pass-empty-cost"),
                fixture_inventory_row("pass-blank-cost"),
                fixture_inventory_row("pass-null-cost"),
                fixture_inventory_row("pass-missing-cost"),
            ],
            [
                fixture_review("pass-main", behavior=tricky_behavior, failure_caught=tricky_failure),
                consolidate_review,
                fixture_review(
                    "pass-keeper", criticality=1, signal=2, uniqueness=1, fidelity=2,
                ),
                fixture_review("pass-empty-cost", cost_drivers=[]),
                fixture_review("pass-blank-cost", cost_drivers=""),
                fixture_review("pass-null-cost", cost_drivers=None),
                missing_cost,
            ],
            [{
                "group_id": "g-ok",
                "member_test_ids": ["pass-consolidate", "pass-keeper"],
                "retained_test_ids": ["pass-keeper"],
                "preservation_evidence": "pass-keeper asserts the same boundary and fixture",
            }],
            receipts=[{
                "test_id": "pass-consolidate",
                "second_reviewer": "independent-reviewer",
                "verdict": "resolved",
                "findings": "Checked the source and confirmed the retained protection.",
                "final_review": adjudicated,
            }],
        )

    def verify_pass(checks, rows, root):
        ok = True
        csv_path = root / "tests.csv"
        write_atomic(csv_path, render_csv(rows))
        file_check = check_file_roundtrip(csv_path, rows, root)
        pass_state["file_check"] = file_check
        if file_check["status"] != "passed":
            ok = False
        main_row = next(row for row in rows if row["test_id"] == "pass-main")
        if main_row["behavior"] != tricky_behavior:
            ok = False
        if main_row["failure_caught"] != tricky_failure:
            ok = False
        if 'emoji ✅ / café' not in main_row["behavior"]:
            ok = False
        if main_row["cost_drivers"] != "in-process fixture; no fixed sleeps":
            ok = False
        consolidate_row = next(row for row in rows if row["test_id"] == "pass-consolidate")
        expected_list_cell = json.dumps(["two feeds", "setup() per case ✅"], ensure_ascii=False)
        if consolidate_row["cost_drivers"] != expected_list_cell:
            ok = False
        if consolidate_row["reviewer"] != "fixture-worker":
            ok = False
        if consolidate_row["second_reviewer"] != "independent-reviewer":
            ok = False
        pass_state["provenance_cells"] = (
            consolidate_row["reviewer"], consolidate_row["second_reviewer"]
        )
        pass_state["cost_driver_cells"] = (
            main_row["cost_drivers"], consolidate_row["cost_drivers"]
        )
        empty_cells = {}
        for test_id in ("pass-empty-cost", "pass-blank-cost", "pass-null-cost", "pass-missing-cost"):
            cell = next(row for row in rows if row["test_id"] == test_id)["cost_drivers"]
            empty_cells[test_id] = cell
            if cell != "[]":
                ok = False
        pass_state["empty_cost_cells"] = empty_cells
        pass_state["rows"] = len(rows)
        return ok

    execute("pass-tricky-cells", build_pass, expect_pass=True, extra_checks=verify_pass)

    # 9-10. hosted attempt-ledger gates: pass path and a manifest-claim mismatch.
    def write_hosted_matrix(root, gate_override=None):
        write_fixture(
            root,
            [fixture_inventory_row("hosted-main"), fixture_inventory_row("hosted-second")],
            [
                fixture_review("hosted-main", criticality=2, signal=2, uniqueness=2, fidelity=2),
                fixture_review("hosted-second", criticality=2, signal=2, uniqueness=2, fidelity=2),
            ],
            [],
        )
        attempt_rows = []
        singles = [2.0] * 2817 + [242.491]
        for index, value in enumerate(singles):
            attempt_rows.append([
                "Run macOS checks", f"bin-{index % EXPECTED_BINARIES}",
                f"ovrcr::suite::case_{index:04d}", "1", "PASS", f"{value:.3f}",
                f"TRY 1 PASS [{value:.3f}s] case_{index}",
            ])
        for name, first, second in (
            (RETRY_GATES[0][0], 4.855, 4.372),
            (RETRY_GATES[1][0], 24.507, 21.743),
        ):
            attempt_rows.append(
                ["Run macOS checks", "bin-0", name, "1", "FAIL", f"{first:.3f}", f"TRY 1 FAIL {name}"]
            )
            attempt_rows.append(
                ["Run macOS checks", "bin-0", name, "2", "PASS", f"{second:.3f}", f"TRY 2 PASS {name}"]
            )
        buffer = io.StringIO()
        writer = csv.writer(buffer, lineterminator="\n")
        writer.writerow(ATTEMPTS_HEADER)
        writer.writerows(attempt_rows)
        attempts_path = root / "evidence" / "attempts.csv"
        attempts_path.write_text(buffer.getvalue(), encoding="utf-8")
        gates = {
            "attempt_records": EXPECTED_ATTEMPT_RECORDS,
            "unique_tests": EXPECTED_UNIQUE_TESTS,
            "binaries": EXPECTED_BINARIES,
            "cumulative_seconds": EXPECTED_CUMULATIVE_SECONDS,
        }
        gates.update(gate_override or {})
        manifest = {
            "pinned_revision": FIXTURE_REVISION,
            "gates": gates,
            "files": {
                "evidence/attempts.csv": {"sha256": sha256_file(attempts_path)},
            },
        }
        (root / "evidence" / "manifest.json").write_text(
            json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
        )

    def verify_hosted_pass(checks, rows, root):
        hosted = next(entry for entry in checks if entry["check"] == CHECK_HOSTED)
        if hosted["status"] != "passed" or aggregate_status(checks) != "complete":
            return False
        model = load_model(root)
        payload = build_validation(
            model, checks, rows, "complete",
            {"path": "tests.csv", "rows": len(rows), "sha256": "fixture", "bytes": 0},
        )
        required = {
            "revision", "status", "command_line", "input_hashes",
            "row_counts", "checks", "csv",
        }
        return (
            required <= set(payload)
            and payload["status"] == "complete"
            and len(payload["input_hashes"]) >= 4
            and len(payload["checks"]) == 9
        )

    execute(
        "hosted-gates-pass", write_hosted_matrix, expect_pass=True,
        extra_checks=verify_hosted_pass,
        expected_label="PASS checks 1-9 incl. hosted gates; validation payload complete",
    )

    def build_hosted_mismatch(root):
        write_hosted_matrix(root, {"cumulative_seconds": 5900.0})

    execute("hosted-gates-mismatch", build_hosted_mismatch, CHECK_HOSTED, [])

    # 11. compatibility import of the inventory workstream's intermediate.
    def build_compat(root):
        (root / "evidence").mkdir(parents=True, exist_ok=True)
        (root / "reviews").mkdir(parents=True, exist_ok=True)
        row = {
            "test_id": "compat-row",
            "input_test_name": "compat::row",
            "invocation": "Run macOS checks",
            "runner": "nextest",
            "suite": "ovrcr::bin/ovrcr",
            "target_kind": "integration",
            "test_level": "integration",
            "test_name": "compat::row",
            "variant": "default",
            "source_file": "Cargo.toml",
            "source_start": "1",
            "source_end": "3",
            "first_attempt_seconds": "0.010",
            "total_attempt_seconds": "0.010",
            "attempt_count": "1",
            "failed_attempt_count": "0",
            "timing_basis": "hosted-single-run",
            "timing_source": "fixture ledger line 1",
        }
        buffer = io.StringIO()
        writer = csv.writer(buffer, lineterminator="\n")
        writer.writerow(INVENTORY_RUST_HEADER)
        writer.writerow([row[column] for column in INVENTORY_RUST_HEADER])
        (root / "evidence" / "inventory_rust.csv").write_text(buffer.getvalue(), encoding="utf-8")
        (root / "reviews" / "shard-a.jsonl").write_text(
            json.dumps(fixture_review("compat-row"), ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        (root / "evidence" / "overlap-groups.json").write_text("[]\n", encoding="utf-8")

    def verify_compat(checks, rows, root):
        model = load_model(root)
        return (
            model["compatibility"] is True
            and model["inventory_source"] == "evidence/inventory_rust.csv"
            and all(row["ci_step"] == "Run macOS checks" for row in rows)
            and bool(revision_of(model))
            and aggregate_status(checks) == "complete-with-evidence-gaps"
        )

    execute(
        "compat-inventory-rust", build_compat, expect_pass=True,
        extra_checks=verify_compat,
        expected_label="PASS when only inventory_rust.csv exists (invocation->ci_step, derived revision)",
    )

    # 12. Two assemblies from the same evidence are byte-identical.
    hosted_root = selftest_root / "hosted-gates-pass"
    first_model = load_model(hosted_root)
    first_checks, first_rows, _ = evaluate(first_model)
    first_text = render_csv(first_rows)
    second_model = load_model(hosted_root)
    second_checks, second_rows, _ = evaluate(second_model)
    second_text = render_csv(second_rows)
    first_shas = {
        "csv": sha256_bytes(first_text.encode("utf-8")),
        "rows": len(first_rows),
    }
    second_shas = {
        "csv": sha256_bytes(second_text.encode("utf-8")),
        "rows": len(second_rows),
    }
    first_validation = build_validation(
        first_model, first_checks, first_rows, aggregate_status(first_checks),
        {"path": "tests.csv", "rows": len(first_rows),
         "sha256": first_shas["csv"], "bytes": len(first_text.encode("utf-8"))},
    )
    second_validation = build_validation(
        second_model, second_checks, second_rows, aggregate_status(second_checks),
        {"path": "tests.csv", "rows": len(second_rows),
         "sha256": second_shas["csv"], "bytes": len(second_text.encode("utf-8"))},
    )
    deterministic = (
        first_text == second_text
        and first_validation == second_validation
        and len(first_rows) == len(second_rows)
    )
    results.append((
        "determinism",
        "PASS identical CSV bytes and validation payload on repeated assembly",
        (
            f"csv sha256 {first_shas['csv'][:16]}… rows={first_shas['rows']}"
            if deterministic
            else "CSV or validation payload differed between runs"
        ),
        "PASS" if deterministic else "FAIL",
    ))

    # 11. --check is read-only and its exit code tracks failed checks.
    check_mode_ok = True
    check_mode_notes = []
    for case, expected_code in (("hosted-gates-pass", 0), ("circular-sole-survivor", 1)):
        case_root = selftest_root / case
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer), contextlib.redirect_stderr(buffer):
            code = run_assemble(check_only=True, root=case_root)
        if code != expected_code:
            check_mode_ok = False
            check_mode_notes.append(f"{case}:exit={code} expected {expected_code}")
        for artifact in (case_root / "tests.csv", case_root / "evidence" / "validation.json"):
            if artifact.exists():
                check_mode_ok = False
                check_mode_notes.append(f"{case}:wrote {artifact.name}")
    results.append((
        "check-mode-read-only",
        "PASS exit 0 on valid, exit 1 on failed, writes nothing",
        "; ".join(check_mode_notes) if check_mode_notes else "exit codes matched; no files written",
        "PASS" if check_mode_ok else "FAIL",
    ))

    after = fixture_snapshot(BASE, selftest_root)

    header = ("fixture", "expected", "observed", "result")
    table = [header] + [tuple(result) for result in results]
    widths = [max(len(str(cell)) for cell in column) for column in zip(*table)]
    for index, row in enumerate(table):
        line = " | ".join(str(cell).ljust(widths[col]) for col, cell in enumerate(row))
        print(line)
        if index == 0:
            print(" | ".join("-" * width for width in widths))

    confinement = "PASS" if before == after else "FAIL"
    if before != after:
        changed = sorted(set(before) ^ set(after))
        print(f"selftest confinement: FAIL; changed paths: {ids_summary(changed)}")
    else:
        print("selftest confinement: PASS (all fixture writes stayed under evidence/selftest/)")
    if "file_check" in pass_state:
        print(
            "round-trip detail: " + pass_state["file_check"]["detail"]
        )
    if "cost_driver_cells" in pass_state:
        string_cell, array_cell = pass_state["cost_driver_cells"]
        print(f"cost_drivers cells: string={string_cell!r}; array={array_cell!r}")
    if "empty_cost_cells" in pass_state:
        print(f"empty cost_drivers cells: {pass_state['empty_cost_cells']}")
    if "provenance_cells" in pass_state:
        author, checker = pass_state["provenance_cells"]
        print(f"provenance cells: reviewer={author!r}; second_reviewer={checker!r}")
    failed = [result for result in results if result[3] != "PASS"]
    print(f"selftest: {len(results) - len(failed)}/{len(results)} fixtures behaved as expected")
    return 0 if (not failed and confinement == "PASS") else 1


def expected_description(expected_check, expected_ids, expect_pass):
    if expect_pass:
        return "PASS structural checks + exact round-trip (comma, quote, newline, non-ASCII)"
    if not expected_ids:
        return f"FAIL {expected_check.split(' ', 1)[0]} {expected_check.split(' ', 1)[1]}"
    return f"FAIL {expected_check.split(' ', 1)[0]} naming {', '.join(expected_ids)}"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Assemble and validate the macOS test-value review CSV."
    )
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--check", action="store_true", help="validate only; write nothing")
    group.add_argument("--selftest", action="store_true", help="run disposable fixture proofs")
    group.add_argument(
        "--audit", action="store_true",
        help="diagnostic: write evidence/audit-details.json offender lists (tolerates a missing "
             "overlap-groups.json and merges segmented receipts in memory)",
    )
    args = parser.parse_args(argv)
    if args.selftest:
        return run_selftest()
    if args.audit:
        return run_audit()
    return run_assemble(check_only=args.check)


if __name__ == "__main__":
    sys.exit(main())
