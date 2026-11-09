#!/usr/bin/env python3
"""Enumerate the Python check cases of the GitHub Actions job "macOS checks".

Two runners:
  * python-unittest - the 39 `unittest` methods of native/bridge/check-installer.py
    and tests/cursor_bridge_install.py. Enumerated with `ast` only: the check
    scripts mutate the filesystem when imported, so they are parsed, never run.
  * python-check    - the 200 sequential script checks of the four
    native/bridge/check-*.py scripts, split by invocation profile.

Writes inventory_python.csv and python-checks.md next to this script and
reconciles every suite/mode against the hosted per-script counts printed in
macos-job.log.

Stdlib only. Run with: python3 -I research/macos-test-review/evidence/enum_python.py
"""

import ast
import csv
import re
import urllib.parse
from pathlib import Path

EVIDENCE = Path(__file__).resolve().parent
REPO = EVIDENCE.parents[2]
LOG = EVIDENCE / "macos-job.log"
REVISION = "992a1da5c59a865ebb48051e4d9d8ff7184c8dc5"
RUN = "42429838269"
JOB = "128576086691"

STARTUP_STEP = "Relocated macOS startup assets"
CURSOR_STEP = "Relocated pinned Cursor dependency and passive CLI diagnostics"

HEADER = [
    "test_id", "parent_test_id", "revision", "ci_step", "runner", "suite",
    "target_kind", "test_name", "variant", "source_file", "source_start",
    "source_end", "first_attempt_seconds", "total_attempt_seconds",
    "attempt_count", "failed_attempt_count", "timing_basis", "timing_source",
]

UNITTEST_TARGETS = [
    (STARTUP_STEP, "native/bridge/check-installer.py", "InstallerTests"),
    (CURSOR_STEP, "tests/cursor_bridge_install.py", "Installation"),
]

PRODUCTION = "production"
LOCAL = "local-development"
DEFAULT = "default"

# Sequential script checks, in execution order.
# (variant, label, anchor line, source_start, source_end, anchor kind)
#   kind "for"       -> the anchor line opens a `for` loop over a literal list;
#                       one row per element, element text in the label.
#   kind "call"      -> one check()/validate()/reject_validation()/check_cache() call.
#   kind "block"     -> a standalone block that ends in a counter increment.
# Shared checks run by both invocations of a script; mode-specific checks run
# only in their own branch. The local-development invocation of a script runs
# every shared check again, so it carries its own copy of those rows: the
# hosted totals (200) equal the sum of shared + mode-specific rows.
GUARDS_COMMON = [
    ("native/bridge/check-guards.py", "entitlement-pin", 35, 28, 35, "block"),
    ("native/bridge/check-guards.py", "contract-available", 50, 50, 50, "call"),
    ("native/bridge/check-guards.py", "contract-schema-newer", 51, 51, 51, "call"),
    ("native/bridge/check-guards.py", "contract-wire-newer", 52, 52, 52, "call"),
    ("native/bridge/check-guards.py", "contract-argv", 53, 53, 55, "for"),
    ("native/bridge/check-guards.py", "client-extra-argv", 56, 56, 56, "call"),
    ("native/bridge/check-guards.py", "client-input", 57, 57, 58, "for"),
    ("native/bridge/check-guards.py", "client-metadata", 59, 59, 64, "for"),
    ("native/bridge/check-guards.py", "client-op", 65, 65, 70, "for"),
    ("native/bridge/check-guards.py", "client-sound", 71, 71, 75, "for"),
    ("native/bridge/check-guards.py", "open-stdin-timeout", 87, 78, 94, "block"),
    ("native/bridge/check-guards.py", "opposite-profile-metadata", 97, 97, 97, "call"),
    ("native/bridge/check-guards.py", "opposite-profile-extra-argv", 98, 98, 98, "call"),
    ("native/bridge/check-guards.py", "wrong-profile-open-stdin-exits", 109, 100, 116, "block"),
]
GUARDS_PRODUCTION = [
    ("native/bridge/check-guards.py", "guard-marker", 138, 138, 141, "for"),
    ("native/bridge/check-guards.py", "bundle-id-local", 144, 142, 144, "call"),
    ("native/bridge/check-guards.py", "bundle-id-private-unmarked", 147, 145, 147, "call"),
]
GUARDS_LOCAL = [
    ("native/bridge/check-guards.py", "local-metadata-mutation", 124, 124, 136, "for"),
]
PACKAGING_COMMON = [
    ("native/bridge/check-packaging.py", "entitlement-pin", 32, 25, 32, "block"),
    ("native/bridge/check-packaging.py", "source-accepted", 44, 44, 44, "call"),
    ("native/bridge/check-packaging.py", "bundle-copy-rejected", 47, 47, 99, "for"),
    ("native/bridge/check-packaging.py", "source-symlink-rejected", 102, 100, 102, "call"),
    ("native/bridge/check-packaging.py", "fixture-identity-rejected", 109, 109, 109, "call"),
    ("native/bridge/check-packaging.py", "fixture-identity-accepted-production", 114, 114, 114, "call"),
]
PACKAGING_LOCAL = [
    ("native/bridge/check-packaging.py", "local-source-production-profile-rejected", 116, 116, 116, "call"),
    ("native/bridge/check-packaging.py", "local-source-production-identity-rejected", 117, 117, 117, "call"),
    ("native/bridge/check-packaging.py", "local-source-identity-argument-rejected", 118, 118, 118, "call"),
    ("native/bridge/check-packaging.py", "local-marker-rejected", 119, 119, 129, "for"),
    ("native/bridge/check-packaging.py", "production-profile-from-local-rejected", 137, 137, 137, "call"),
    ("native/bridge/check-packaging.py", "production-profile-accepted", 138, 138, 138, "call"),
]
STARTUP_COMMON = [
    ("native/bridge/check-startup-package.py", "relocated-validator-accepts", 32, 32, 33, "block"),
    ("native/bridge/check-startup-package.py", "changed-helper-rejected", 41, 41, 42, "block"),
    ("native/bridge/check-startup-package.py", "entitlement-pin", 51, 45, 53, "block"),
    ("native/bridge/check-startup-package.py", "missing-entitlements-rejected", 56, 54, 57, "block"),
    ("native/bridge/check-startup-package.py", "entitlement-layout-rejected", 59, 59, 75, "for"),
    ("native/bridge/check-startup-package.py", "changed-purpose-rejected", 85, 82, 86, "block"),
    ("native/bridge/check-startup-package.py", "changed-sound-rejected", 91, 88, 92, "block"),
    ("native/bridge/check-startup-package.py", "install-refuses-ad-hoc", 93, 93, 94, "block"),
]
STARTUP_LOCAL = [
    ("native/bridge/check-startup-package.py", "local-production-validation-rejected", 108, 108, 109, "call"),
    ("native/bridge/check-startup-package.py", "local-identity-arguments-rejected", 110, 110, 111, "call"),
    ("native/bridge/check-startup-package.py", "local-contract-profile-pin-rejected", 116, 116, 117, "call"),
    ("native/bridge/check-startup-package.py", "local-production-metadata-rejected", 118, 118, 118, "call"),
    ("native/bridge/check-startup-package.py", "contract-field-rejected", 120, 120, 124, "for"),
    ("native/bridge/check-startup-package.py", "local-marker-rejected", 126, 126, 133, "for"),
    ("native/bridge/check-startup-package.py", "cache-hit-local-profile", 221, 221, 221, "call"),
    ("native/bridge/check-startup-package.py", "converted-production-metadata-valid", 233, 225, 233, "block"),
    ("native/bridge/check-startup-package.py", "cache-miss-production-profile", 234, 234, 234, "call"),
    ("native/bridge/check-startup-package.py", "cache-miss-relocated-profile-pin", 239, 239, 239, "call"),
]
# Invoked bare (`python3 -I native/bridge/check-local-startup.py`): no profile
# flag, so variant=default. The script drives both the production and the
# local-development just recipes inside its own private fixture.
LOCAL_STARTUP_SITES = [
    (DEFAULT, "native/bridge/check-local-startup.py", "run-recipe-production", 120, 120, 131, "block"),
    (DEFAULT, "native/bridge/check-local-startup.py", "run-recipe-local-marker", 134, 134, 144, "block"),
    (DEFAULT, "native/bridge/check-local-startup.py", "run-recipe-explicit-opt-out", 147, 147, 152, "for"),
    (DEFAULT, "native/bridge/check-local-startup.py", "run-recipe-invalid-opt-in", 156, 156, 161, "for"),
    (DEFAULT, "native/bridge/check-local-startup.py", "unmanaged-local-assets-preserved", 167, 163, 171, "block"),
    (DEFAULT, "native/bridge/check-local-startup.py", "symlinked-local-assets-refused", 177, 173, 181, "block"),
    (DEFAULT, "native/bridge/check-local-startup.py", "linux-kernel-build-path", 184, 183, 188, "block"),
    (DEFAULT, "native/bridge/check-local-startup.py", "install-assets-production", 197, 190, 206, "block"),
    (DEFAULT, "native/bridge/check-local-startup.py", "install-assets-local", 212, 212, 219, "block"),
    (DEFAULT, "native/bridge/check-local-startup.py", "install-assets-refusal", 221, 221, 225, "for"),
    (DEFAULT, "native/bridge/check-local-startup.py", "unmanaged-release-assets-preserved", 230, 227, 233, "block"),
]


def build(common, per_mode):
    rows = []
    for variant in (PRODUCTION, LOCAL):
        for entry in list(common) + list(per_mode.get(variant, [])):
            rows.append((variant,) + entry)
    return rows


SITES = (build(GUARDS_COMMON, {PRODUCTION: GUARDS_PRODUCTION, LOCAL: GUARDS_LOCAL})
         + build(PACKAGING_COMMON, {LOCAL: PACKAGING_LOCAL})
         + build(STARTUP_COMMON, {LOCAL: STARTUP_LOCAL})
         + LOCAL_STARTUP_SITES)

CHECK_SUITES = [
    "native/bridge/check-local-startup.py",
    "native/bridge/check-packaging.py",
    "native/bridge/check-startup-package.py",
    "native/bridge/check-guards.py",
]

# Hosted per-script counts, in workflow command order, as printed by the scripts.
# (suite, mode, log regex, workflow command fragment)
HOSTED = [
    ("native/bridge/check-local-startup.py", DEFAULT,
     r"^(\d+) actual startup recipe checks passed; fake tools only",
     "python3 -I native/bridge/check-local-startup.py"),
    ("native/bridge/check-startup-package.py", PRODUCTION,
     r"^(\d+) relocated startup payload checks passed;",
     "python3 -I native/bridge/check-startup-package.py target/startup-assets"),
    ("native/bridge/check-packaging.py", PRODUCTION,
     r"^(\d+) packaging checks passed under PYTHONOPTIMIZE=1;",
     "python3 -I native/bridge/check-packaging.py 'target/startup-assets/OVRCR Bridge.app'"),
    ("native/bridge/check-guards.py", PRODUCTION,
     r"^(\d+) compiled-entry and bundle-layout guard checks passed;",
     "python3 -I native/bridge/check-guards.py 'target/startup-assets/OVRCR Bridge.app'"),
    ("native/bridge/check-startup-package.py", LOCAL,
     r"^(\d+) relocated startup payload checks passed;",
     "python3 -I native/bridge/check-startup-package.py --local-development target/startup-assets-local-development"),
    ("native/bridge/check-packaging.py", LOCAL,
     r"^(\d+) packaging checks passed under PYTHONOPTIMIZE=1;",
     "python3 -I native/bridge/check-packaging.py --local-development 'target/startup-assets-local-development/OVRCR Bridge local.app'"),
    ("native/bridge/check-guards.py", LOCAL,
     r"^(\d+) compiled-entry and bundle-layout guard checks passed;",
     "python3 -I native/bridge/check-guards.py --local-development 'target/startup-assets-local-development/OVRCR Bridge Local.app'"),
]

PREFIX = re.compile(r"^[^\t]*\t[^\t]*\t\d{4}-\d\d-\d\dT[\d:.]+Z ?")


def strip(line):
    return PREFIX.sub("", line)


def load_log():
    with LOG.open(encoding="utf-8-sig", errors="replace") as handle:
        return [strip(line.rstrip("\n")) for line in handle]


def quote_part(part):
    return urllib.parse.quote(part, safe="")


def test_id(*parts):
    return "|".join(quote_part(part) for part in parts)


# ------------------------------------------------------------------ unittest

def unittest_rows():
    rows = []
    facts = []
    lines = load_log()
    for step, path, class_name in UNITTEST_TARGETS:
        source = (REPO / path).read_text(encoding="utf-8")
        tree = ast.parse(source, filename=str(REPO / path))
        classes = [node for node in tree.body
                   if isinstance(node, ast.ClassDef) and node.name == class_name]
        if len(classes) != 1:
            raise SystemExit("%s: class %s not found once" % (path, class_name))
        klass = classes[0]
        methods = [node for node in klass.body
                   if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
                   and node.name.startswith("test_")]
        executed = [name for name in re.findall(r"\(__main__\.%s\.(\w+)\) \.\.\. ok" % class_name,
                                               "\n".join(lines))]
        if executed:
            if sorted(executed) != sorted(method.name for method in methods):
                raise SystemExit("%s: executed names differ from source methods" % path)
        # unittest reports in sorted method order.
        for method in sorted(methods, key=lambda node: node.name):
            rows.append([
                test_id("python-unittest", step, "%s::%s" % (path, class_name),
                        method.name, DEFAULT),
                "", REVISION, step, "python-unittest", "%s::%s" % (path, class_name),
                "unittest", method.name, DEFAULT, path,
                method.lineno, method.end_lineno, "", "", "", "",
                "unmeasured", "",
            ])
        facts.append((path, class_name, len(methods), len(executed)))
    return rows, facts


# -------------------------------------------------------------- script checks

def classify(source, tree, anchor):
    line = source.split("\n")[anchor - 1].strip()
    if not line:
        raise SystemExit("anchor line %d is blank" % anchor)
    calls = [node for node in ast.walk(tree)
             if isinstance(node, ast.Call) and node.lineno == anchor]
    loops = [node for node in ast.walk(tree)
             if isinstance(node, (ast.For, ast.AsyncFor)) and node.lineno == anchor]
    increments = [node for node in ast.walk(tree)
                  if isinstance(node, ast.AugAssign) and node.lineno == anchor
                  and isinstance(node.target, ast.Name) and node.target.id == "checks"]
    return calls, loops, increments


def script_rows():
    rows = []
    per_suite_mode = {}
    detail = []
    for variant, path, label, anchor, start, end, kind in SITES:
        source = (REPO / path).read_text(encoding="utf-8")
        tree = ast.parse(source, filename=str(REPO / path))
        total_lines = len(source.split("\n"))
        if not (1 <= start <= anchor <= end <= total_lines):
            raise SystemExit("%s %s: bad range %d/%d/%d" % (path, label, start, anchor, end))
        calls, loops, increments = classify(source, tree, anchor)
        if kind == "for":
            if len(loops) != 1:
                raise SystemExit("%s:%d %s: expected one loop, found %d"
                                 % (path, anchor, label, len(loops)))
            iterable = loops[0].iter
            if not isinstance(iterable, (ast.List, ast.Tuple)):
                raise SystemExit("%s:%d %s: loop is not over a literal list"
                                 % (path, anchor, label))
            labels = []
            for element in iterable.elts:
                text = " ".join(ast.unparse(element).split())
                labels.append("%s[%s]:ln%d" % (label, text, anchor))
        elif kind == "call":
            if not calls:
                raise SystemExit("%s:%d %s: no call on the anchor line" % (path, anchor, label))
            names = {getattr(call.func, "id", getattr(call.func, "attr", None)) for call in calls}
            if not names & {"check", "validate", "reject_validation", "check_cache"}:
                raise SystemExit("%s:%d %s: anchor call is %r" % (path, anchor, label, names))
            labels = ["%s:ln%d" % (label, anchor)]
        elif kind == "block":
            if not (calls or increments or loops):
                raise SystemExit("%s:%d %s: block anchor has no check or counter increment"
                                 % (path, anchor, label))
            labels = ["%s:ln%d" % (label, anchor)]
        else:
            raise SystemExit("unknown kind %r" % kind)
        for name in labels:
            rows.append([
                test_id("python-check", STARTUP_STEP, path, name, variant),
                "", REVISION, STARTUP_STEP, "python-check", path, "script-check",
                name, variant, path, start, end, "", "", "", "",
                "unmeasured",
                "no per-check timing; %s aggregate only (see python-checks.md)" % path,
            ])
        key = (path, variant)
        per_suite_mode[key] = per_suite_mode.get(key, 0) + len(labels)
        detail.append((path, variant, label, start, end, kind, len(labels)))
    return rows, per_suite_mode, detail


def hosted_counts(lines):
    """Read the per-script totals from the log, in workflow command order."""
    step_start = None
    for index, line in enumerate(lines):
        if line.startswith("##[group]Run cargo build -p ovrcr --bin ovrcr"):
            step_start = index
            break
    if step_start is None:
        raise SystemExit("startup asset step not found")
    step_end = step_start
    while step_end < len(lines) and not (
            lines[step_end].startswith("##[group]Run bridge_home=")):
        step_end += 1
    patterns = {suite: pattern for suite, _, pattern, _ in HOSTED}
    printed = {}
    for suite in CHECK_SUITES:
        found = []
        for index in range(step_start, step_end):
            match = re.match(patterns[suite], lines[index])
            if match:
                found.append((int(match.group(1)), index + 1))
        if not found:
            raise SystemExit("no printed total for %s" % suite)
        printed[suite] = found
    hits = []
    for suite, mode, _, command in HOSTED:
        if not printed[suite]:
            raise SystemExit("%s printed fewer totals than the workflow invokes it" % suite)
        count, line = printed[suite].pop(0)
        hits.append((suite, mode, count, line))
    for suite, remaining in printed.items():
        if remaining:
            raise SystemExit("%s printed %d unused totals" % (suite, len(remaining)))
    positions = [line for _, _, _, line in hits]
    if positions != sorted(positions):
        raise SystemExit("printed order does not follow the workflow command order: %r"
                         % (hits,))
    return {(suite, mode): (count, line) for suite, mode, count, line in hits}


def unittest_facts(lines):
    facts = {}
    for index, line in enumerate(lines):
        match = re.match(r"^Ran (\d+) tests in ([0-9.]+)s$", line)
        if match:
            facts.setdefault("runs", []).append((int(match.group(1)), float(match.group(2)), index + 1))
    return facts["runs"]


def main():
    log = load_log()
    suites_in_sites = {path for _, path, _, _, _, _, _ in SITES}
    if suites_in_sites != set(CHECK_SUITES):
        raise SystemExit("SITES/CHECK_SUITES disagree: %r" % (suites_in_sites ^ set(CHECK_SUITES)))
    unit_rows, unit_facts = unittest_rows()
    script_rows_list, per_suite_mode, detail = script_rows()
    hosted = hosted_counts(log)

    # Attach the suite-level timing evidence now that the log line is known.
    unit_timing = {}
    for step, path, class_name in UNITTEST_TARGETS:
        if path == "native/bridge/check-installer.py":
            for index, line in enumerate(log):
                if re.match(r"^Ran 33 tests in [0-9.]+s$", line):
                    unit_timing[path] = "macOS-job.log:%d %r" % (index + 1, line)
                    break
        else:
            for index, line in enumerate(log):
                if re.match(r"^Ran 6 tests in [0-9.]+s$", line):
                    unit_timing[path] = "macOS-job.log:%d %r" % (index + 1, line)
                    break
        if path not in unit_timing:
            raise SystemExit("no unittest aggregate for %s" % path)
    for row in unit_rows:
        step, suite = row[3], row[5]
        path = suite.split("::")[0]
        row[17] = ("no per-method timing; unittest prints one aggregate: %s "
                   "(run %s job %s step '%s')" % (unit_timing[path], RUN, JOB, step))

    script_timing = {}
    for key, (count, line) in hosted.items():
        fraction = "(%d checks)" % count
        script_timing[key] = ("suite aggregate only: macOS-job.log:%d %s %r "
                              "(run %s job %s step '%s')"
                              % (line, fraction,
                                 log[line - 1], RUN, JOB, STARTUP_STEP))
    for row in script_rows_list:
        key = (row[5], row[8])
        row[17] = script_timing[key]

    # ------------------------------------------------------------ reconcile
    report = []
    ok = True
    enumerated_unittest = {}
    for row in unit_rows:
        path = row[5].split("::")[0]
        enumerated_unittest[path] = enumerated_unittest.get(path, 0) + 1
    runs = unittest_facts(log)
    if len(runs) != 2:
        raise SystemExit("expected two unittest aggregates, found %d" % len(runs))
    expected_runs = sorted(enumerated_unittest.values())
    if [count for count, _, _ in runs] != list(enumerated_unittest.values()):
        raise SystemExit("unittest aggregates %r do not match enumerated %r"
                         % ([c for c, _, _ in runs], enumerated_unittest))

    total_enumerated = 0
    for key in sorted(per_suite_mode):
        suite, mode = key
        hosted_count, hosted_line = hosted[key]
        got = per_suite_mode[key]
        total_enumerated += got
        if got != hosted_count:
            ok = False
        report.append((suite, mode, hosted_count, hosted_line, got))

    with (EVIDENCE / "inventory_python.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(HEADER)
        writer.writerows(unit_rows + script_rows_list)

    write_report(report, ok, unit_rows, script_rows_list, per_suite_mode, hosted,
                 detail, unit_facts, runs, unit_timing)

    print("python-unittest rows=%d %s" % (len(unit_rows), enumerated_unittest))
    print("python-check rows=%d" % len(script_rows_list))
    for suite, mode, hosted_count, hosted_line, got in report:
        print("%-45s %-18s hosted=%3d (line %4d) enumerated=%3d %s"
              % (suite, mode, hosted_count, hosted_line, got,
                 "ok" if got == hosted_count else "MISMATCH"))
    print("python-check total=%d hosted_total=%d"
          % (len(script_rows_list), sum(row[2] for row in report)))
    if not ok:
        raise SystemExit("reconciliation failed")
    if len(unit_rows) + len(script_rows_list) != 39 + 200:
        raise SystemExit("row total is not 239")


def write_report(report, ok, unit_rows, script_rows_list, per_suite_mode, hosted,
                 detail, unit_facts, runs, unit_timing):
    with (EVIDENCE / "python-checks.md").open("w", encoding="utf-8") as handle:
        handle.write("# Python inventory notes\n\n")
        handle.write("Run %s, job %s, revision `%s`.\n\n" % (RUN, JOB, REVISION))

        handle.write("## Runners and steps\n\n")
        handle.write("| runner | ci_step | suite | rows |\n|---|---|---|---|\n")
        handle.write("| python-unittest | `%s` | `native/bridge/check-installer.py::InstallerTests` | %d |\n"
                     % (STARTUP_STEP, sum(1 for row in unit_rows
                                          if row[5].startswith("native/bridge/check-installer.py"))))
        handle.write("| python-unittest | `%s` | `tests/cursor_bridge_install.py::Installation` | %d |\n"
                     % (CURSOR_STEP, sum(1 for row in unit_rows
                                         if row[5].startswith("tests/cursor_bridge_install.py"))))
        for suite in CHECK_SUITES:
            count = sum(1 for row in script_rows_list if row[5] == suite)
            handle.write("| python-check | `%s` | `%s` | %d |\n" % (STARTUP_STEP, suite, count))
        handle.write("| | | **total** | **%d** |\n\n" % (len(unit_rows) + len(script_rows_list)))

        handle.write("## python-unittest: 39 executed methods\n\n")
        handle.write("`unittest` sorts test methods by name, so rows are in execution order\n"
                     "(alphabetical), not source order. Source ranges are the `def` block.\n"
                     "No per-method timing is printed: the suite prints one `Ran N tests in T s`\n"
                     "aggregate, so every duration and attempt column is empty with\n"
                     "`timing_basis=unmeasured`.\n\n")
        for path, class_name, count, executed in unit_facts:
            handle.write("- `%s::%s`: %d `test_` methods, %d name lines printed by the runner\n"
                         % (path, class_name, count, executed))
        handle.write("\n| suite | aggregate |\n|---|---|\n")
        for path, _, _, _ in unit_facts:
            handle.write("| `%s` | %s |\n" % (path, unit_timing[path]))
        handle.write("\nThe two runner aggregates report %s and %s executed methods, matching\n"
                     "the enumerated %s per file.\n\n"
                     % (runs[0][0], runs[1][0],
                        [count for _, _, count, _ in unit_facts]))
        handle.write("Variant: `default` for both files. Neither invocation passes a profile\n"
                     "flag; `check-installer.py` derives its local fixtures from its own cases.\n\n")

        handle.write("## python-check: 200 sequential logical checks\n\n")
        handle.write("A row is one executed `check()`/`validate()`/`reject_validation()`/\n"
                     "`check_cache()` invocation, one element of a table-driven loop, or one\n"
                     "standalone block that ends in a check-counter increment. Assertions inside\n"
                     "such a block are not separate rows.\n\n")
        handle.write("### Reconciliation (hosted total vs enumerated)\n\n")
        handle.write("| script | variant | hosted | hosted log line | enumerated | match |\n"
                     "|---|---|---|---|---|---|\n")
        for suite, mode, hosted_count, hosted_line, got in report:
            handle.write("| `%s` | %s | %d | %d | %d | %s |\n"
                         % (suite, mode, hosted_count, hosted_line, got,
                            "yes" if got == hosted_count else "NO"))
        handle.write("| | | **%d** | | **%d** | %s |\n\n"
                     % (sum(row[2] for row in report), sum(row[4] for row in report),
                        "yes" if ok else "NO"))
        handle.write("Mode attribution follows the workflow command order: the step runs\n"
                     "`check-local-startup.py`, `check-installer.py`, then\n"
                     "`check-startup-package.py`, `check-packaging.py`, `check-guards.py`\n"
                     "without a flag, then the last three again with `--local-development`.\n"
                     "The script totals appear in the log in exactly that order, which the\n"
                     "enumeration asserts before pairing them.\n\n")

        handle.write("### Per-script site map (execution order)\n\n")
        handle.write("| script | variant | label | source | kind | rows |\n|---|---|---|---|---|---|\n")
        for path, variant, label, start, end, kind, count in detail:
            handle.write("| `%s` | %s | `%s` | %d-%d | %s | %d |\n"
                         % (path.split("/")[-1], variant, label, start, end, kind, count))
        handle.write("\n### Notes on specific counters\n\n")
        handle.write("**check-guards.py** keeps one `checks` counter and increments it for\n"
                     "every `check()` call plus the two standalone stdin-pipe blocks and the\n"
                     "entitlement-pin block: 34 increments before the metadata mutation, then\n"
                     "6 more in either branch (4 marker values + 2 bundle ids in production,\n"
                     "6 metadata keys in local development). 40 per mode is exact.\n\n")
        handle.write("**check-packaging.py** counts the entitlement-pin block, `validate()`\n"
                     "calls and each of the 22 mutated bundle copies: 27 in production, plus 9\n"
                     "local-profile checks (3 identity refusals, 4 marker values, 1 production\n"
                     "profile refusal, 1 explicit production profile acceptance) = 36.\n\n")
        handle.write("**check-startup-package.py** sets `checks = 6 + layout_checks` after the\n"
                     "relocated validations. The six base checks are the pristine acceptance,\n"
                     "the changed-helper refusal, the pinned-entitlement block, the changed\n"
                     "purpose, the changed sound and the `install-bridge.sh` non-ad-hoc refusal;\n"
                     "`layout_checks` starts at 1 (missing entitlements) and grows by the five\n"
                     "entitlement-layout cases, so 6 + 6 = 12. Local development adds 16: four\n"
                     "`reject_validation` calls, four changed contract fields, four invalid\n"
                     "markers, the converted production metadata acceptance and three\n"
                     "`check_cache` runs = 28. The pin block is counted once, in the base six;\n"
                     "the six `layout_checks` units are the missing-entitlement refusal plus the\n"
                     "five loop cases, so no block is counted twice.\n\n")
        handle.write("**check-local-startup.py** has no profile flag of its own: the workflow\n"
                     "invokes it bare, and the script drives both the production and the\n"
                     "local-development just recipes inside its private fixture, so the same 17\n"
                     "checks cover both profiles. Its rows therefore carry `variant=default` \n"
                     "(no profile argument on the command line) rather than `production` or\n"
                     "`local-development`, which would misdescribe half of what each row runs.\n"
                     "Each row's label names the recipe path it exercises\n"
                     "(`run-recipe-production`, `run-recipe-local-marker`, `install-assets-*`).\n\n")

        handle.write("### Scope\n\n")
        handle.write("Only commands of job `%s` (%s) are inventoried. The sibling job\n"
                     "`macOS offline Cursor preflight` runs\n"
                     "`python3 -I -m unittest discover -s experiments/cursor-cloud -p test_client.py -v`,\n"
                     "which is a different job and therefore outside these rows; so is the\n"
                     "`linux` job's copy of the same command.\n\n"
                     % (JOB, "macOS checks"))
        handle.write("Neither check script is imported by this enumeration: they mutate the\n"
                     "filesystem at import time (`__main__` bodies run immediately), so the\n"
                     "enumeration reads them with `ast.parse` and matches the printed totals.\n\n")
        handle.write("### Timing\n\n")
        handle.write("Every python row leaves `first_attempt_seconds`, `total_attempt_seconds`,\n"
                     "`attempt_count` and `failed_attempt_count` empty with\n"
                     "`timing_basis=unmeasured`. The scripts print only one summary line each,\n"
                     "and the buffered CI timestamps are not attributable to a single check, so\n"
                     "no per-check time is derived from a suite duration or from timestamp gaps.\n\n")
        handle.write("### Label construction\n\n")
        handle.write("Labels are frozen `script-local-qualified` names. Loop-generated checks\n"
                     "carry the iteration element and the loop's line as\n"
                     "`label[<unparsed element>]:ln<anchor>`, so the mapping back to source is\n"
                     "deterministic. Non-loop checks carry `label:ln<anchor>`. `source_start`..\n"
                     "`source_end` is the loop statement for loop-generated checks, the call\n"
                     "statement for single calls, and the enclosing block for standalone checks.\n")


if __name__ == "__main__":
    main()
