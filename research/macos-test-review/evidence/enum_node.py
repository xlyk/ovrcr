#!/usr/bin/env python3
"""Enumerate the Node test cases of the GitHub Actions job "macOS checks".

Reads research/macos-test-review/evidence/macos-job.log only (nothing is imported
or executed from the repository), attributes every runner-reported TAP case to
its .mjs source and writes inventory_node.csv, node-tap.txt and node-notes.md
next to this script.

Stdlib only. Run with: python3 -I research/macos-test-review/evidence/enum_node.py
"""

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

BATCH_STEP = "Extension tests"
BATCH_FILES = [
    "tests/pi_package_bootstrap.mjs",
    "tests/pi_reporting_extension.mjs",
    "tests/omp_reporting_extension.mjs",
    "tests/pi_delegation_extension.mjs",
    "tests/pi_interrupt_extension.mjs",
    "tests/pi_question_extension.mjs",
    "tests/pi_continuation_records.mjs",
    "tests/pi_observation_extension.mjs",
    "tests/pi_workflow_tracker.mjs",
]

# (workflow step name, .mjs file, Rust test filter passed to cargo, expected cases)
NESTED = [
    ("Installed Pi package through production materialization",
     "tests/pi_package_installed.mjs",
     "production_materializer_loads_local_package_with_one_injected_owner", 2),
    ("Installed Pi effective child tools through production assignment argv",
     "tests/pi_child_tools_installed.mjs",
     "coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools", 2),
]

HEADER = [
    "test_id", "parent_test_id", "revision", "ci_step", "runner", "suite",
    "target_kind", "test_name", "variant", "source_file", "source_start",
    "source_end", "first_attempt_seconds", "total_attempt_seconds",
    "attempt_count", "failed_attempt_count", "timing_basis", "timing_source",
]

PREFIX = re.compile(r"^[^\t]*\t[^\t]*\t\d{4}-\d\d-\d\dT[\d:.]+Z ?")


def strip(line):
    return PREFIX.sub("", line)


def load_log():
    with LOG.open(encoding="utf-8-sig", errors="replace") as handle:
        return [strip(line.rstrip("\n")) for line in handle]


def parse_tap(lines, first, last):
    """Parse one TAP stream from lines[first-1:last]."""
    cases = []
    summary = {}
    markers = {}
    index = first - 1
    while index < last:
        line = lines[index]
        match = re.match(r"^(ok|not ok) (\d+) - (.*)$", line)
        if match:
            duration = None
            duration_line = None
            for probe in range(index + 1, min(index + 14, last)):
                found = re.match(r"^\s+duration_ms: ([0-9.]+)$", lines[probe])
                if found:
                    duration = float(found.group(1))
                    duration_line = probe + 1
                    break
            cases.append({
                "number": int(match.group(2)),
                "name": match.group(3),
                "ok": match.group(1) == "ok",
                "duration_ms": duration,
                "duration_line": duration_line,
                "log_line": index + 1,
            })
        if line.startswith("1.."):
            summary["plan"] = int(line[3:])
            markers["plan"] = index + 1
        elif line.startswith("# tests "):
            summary["tests"] = int(line.split()[2])
            markers["tests"] = index + 1
        elif line.startswith("# pass "):
            summary["pass"] = int(line.split()[2])
        elif line.startswith("# fail "):
            summary["fail"] = int(line.split()[2])
        elif line.startswith("# duration_ms "):
            summary["duration_ms"] = float(line.split()[2])
            markers["duration_ms"] = index + 1
        index += 1
    return cases, summary, markers


# ---------------------------------------------------------------- JS scanning

REGEX_PREV = set("=([{,:;!&|?+-*%~^<>")


def scan_template(text, index):
    """Return the index just past the template literal that starts at index."""
    length = len(text)
    while index < length:
        char = text[index]
        if char == "\\":
            index += 2
            continue
        if char == "`":
            return index + 1
        if char == "$" and index + 1 < length and text[index + 1] == "{":
            depth = 1
            index += 2
            while index < length and depth:
                inner = text[index]
                if inner == "\\":
                    index += 2
                    continue
                if inner == "{":
                    depth += 1
                elif inner == "}":
                    depth -= 1
                    if not depth:
                        index += 1
                        break
                elif inner == "`":
                    index = scan_template(text, index + 1)
                    continue
                elif inner in "\"'":
                    quote = inner
                    index += 1
                    while index < length and text[index] != quote:
                        index += 2 if text[index] == "\\" else 1
                    index += 1
                    continue
                index += 1
            continue
        index += 1
    raise ValueError("unterminated template")


def scan_regex(text, index):
    length = len(text)
    index += 1
    in_class = False
    while index < length:
        char = text[index]
        if char == "\\":
            index += 2
            continue
        if char == "[":
            in_class = True
        elif char == "]":
            in_class = False
        elif char == "/" and not in_class:
            index += 1
            while index < length and text[index].isalpha():
                index += 1
            return index
        elif char == "\n":
            return index
        index += 1
    return index


def match_paren(text, open_index):
    """Return the index of the ')' matching text[open_index] == '('."""
    stack = [")"]
    index = open_index + 1
    prev = "("
    length = len(text)
    while index < length:
        char = text[index]
        if char == "/" and index + 1 < length and text[index + 1] == "/":
            newline = text.find("\n", index)
            index = length if newline < 0 else newline
            continue
        if char == "/" and index + 1 < length and text[index + 1] == "*":
            end = text.find("*/", index + 2)
            index = length if end < 0 else end + 2
            continue
        if char in "\"'":
            quote = char
            index += 1
            while index < length:
                if text[index] == "\\":
                    index += 2
                    continue
                if text[index] == quote:
                    index += 1
                    break
                if text[index] == "\n":
                    break
                index += 1
            prev = quote
            continue
        if char == "`":
            index = scan_template(text, index + 1)
            prev = "`"
            continue
        if char == "/" and (prev in REGEX_PREV or prev == ""):
            index = scan_regex(text, index)
            prev = "/"
            continue
        if char in "([{":
            stack.append({"(": ")", "[": "]", "{": "}"}[char])
            prev = char
            index += 1
            continue
        if char in ")]}":
            closer = stack.pop()
            if char != closer:
                raise ValueError("unbalanced %r at %d" % (char, index))
            if not stack:
                return index
            prev = char
            index += 1
            continue
        if not char.isspace():
            prev = char
        index += 1
    raise ValueError("unterminated call at %d" % open_index)


SITE = re.compile(r"^(?P<indent>[ \t]*)(?P<loop>for\s*\(.*\)\s*)?test\(")


def split_elements(text):
    """Split the inside of a JS array/tuple literal into its top-level elements."""
    parts = []
    depth = 0
    current = []
    index = 0
    length = len(text)
    while index < length:
        char = text[index]
        if char in "\"'":
            quote = char
            current.append(char)
            index += 1
            while index < length:
                current.append(text[index])
                if text[index] == "\\":
                    if index + 1 < length:
                        current.append(text[index + 1])
                        index += 2
                        continue
                if text[index] == quote:
                    index += 1
                    break
                index += 1
            continue
        if char == "`":
            end = scan_template(text, index + 1)
            current.append(text[index:end])
            index = end
            continue
        if char in "([{":
            depth += 1
            current.append(char)
            index += 1
            continue
        if char in ")]}":
            depth -= 1
            current.append(char)
            index += 1
            continue
        if char == "," and depth == 0:
            parts.append("".join(current).strip())
            current = []
            index += 1
            continue
        current.append(char)
        index += 1
    tail = "".join(current).strip()
    if tail:
        parts.append(tail)
    return parts


def js_string_at(text, index):
    while index < len(text) and text[index] in " \t\n":
        index += 1
    quote = text[index]
    if quote not in "\"'`":
        raise ValueError("expected a string literal at %d" % index)
    start = index
    index += 1
    value = []
    while index < len(text):
        char = text[index]
        if char == "\\":
            value.append(text[index + 1:index + 2])
            index += 2
            continue
        if char == quote:
            index += 1
            break
        value.append(char)
        index += 1
    return "".join(value), text[start:index], quote


def find_sites(path, unresolved_count=None):
    """Every node:test case site in a .mjs file with its defining line range.

    Loop-generated cases are expanded, one entry per iterable element.
    """
    text = (REPO / path).read_text(encoding="utf-8")
    lines = text.split("\n")
    offsets = [0]
    for line in lines:
        offsets.append(offsets[-1] + len(line) + 1)

    def line_of(offset):
        low, high = 0, len(lines) - 1
        while low < high:
            mid = (low + high + 1) // 2
            if offsets[mid] <= offset:
                low = mid
            else:
                high = mid - 1
        return low + 1

    sites = []
    for number, line in enumerate(lines, 1):
        match = SITE.match(line)
        if not match:
            continue
        open_index = offsets[number - 1] + line.index("test(") + len("test")
        name, literal, quote = js_string_at(text, open_index + 1)
        close = match_paren(text, open_index)
        indent = len(match.group("indent").expandtabs(8))
        loop_line = None
        if match.group("loop"):
            loop_line = number
        elif indent:
            probe = number - 1
            while probe >= 1:
                candidate = lines[probe - 1]
                if not candidate.strip():
                    probe -= 1
                    continue
                probe_indent = len(candidate) - len(candidate.lstrip())
                if probe_indent < indent:
                    if re.match(r"^\s*for\s*\(", candidate):
                        loop_line = probe
                    break
                probe -= 1
        base = {
            "line": number,
            "end_line": line_of(close),
            "loop_line": loop_line,
            "name": name,
            "literal": literal,
            "template": quote == "`",
            "element": None,
        }
        if not loop_line:
            sites.append(base)
            continue

        loop_line_text = lines[loop_line - 1]
        loop_open = offsets[loop_line - 1] + loop_line_text.index("(")
        loop_close = match_paren(text, loop_open)
        header = text[loop_open + 1:loop_close]
        keyword = re.search(r"\b(?:of|in)\b", header)
        if keyword is None:
            raise ValueError("cannot read the loop header at %s:%d" % (path, loop_line))
        iterable = text[loop_open + 1 + keyword.end():loop_close].strip()
        if iterable[0] in "[(" and iterable[-1] in "])":
            elements = split_elements(iterable[1:-1])
        elif unresolved_count:
            # The loop walks runtime data (e.g. the launches recorded by the
            # parent Rust test); the parent step's TAP stream fixes the count.
            elements = ["<runtime element %d of %s>" % (position + 1, iterable)
                        for position in range(unresolved_count)]
        else:
            raise ValueError("loop at %s:%d is not table-driven: %r" % (path, loop_line, iterable))
        after = loop_close + 1
        while after < len(text) and text[after] in " \t":
            after += 1
        if after < len(text) and text[after] == "{":
            loop_end = line_of(match_brace(text, after))
        elif text.startswith("test(", after):
            loop_end = line_of(close)
        else:
            raise ValueError("unreadable loop body at %s:%d" % (path, loop_line))
        for position, element in enumerate(elements):
            entry = dict(base)
            entry["loop_line"] = loop_line
            entry["end_line"] = loop_end
            entry["element"] = " ".join(element.split())
            entry["index"] = position
            sites.append(entry)
    return sites


def match_brace(text, open_index):
    """Return the index of the '}' matching text[open_index] == '{'."""
    # Reuse the paren scanner by walking with a fresh stack seeded on '{'.
    if text[open_index] != "{":
        raise ValueError("expected '{' at %d" % open_index)
    depth = 1
    position = open_index + 1
    length = len(text)
    prev = "{"
    while position < length:
        char = text[position]
        if char == "/" and position + 1 < length and text[position + 1] == "/":
            newline = text.find("\n", position)
            position = length if newline < 0 else newline
            continue
        if char == "/" and position + 1 < length and text[position + 1] == "*":
            end = text.find("*/", position + 2)
            position = length if end < 0 else end + 2
            continue
        if char in "\"'":
            quote = char
            position += 1
            while position < length:
                if text[position] == "\\":
                    position += 2
                    continue
                if text[position] == quote:
                    position += 1
                    break
                if text[position] == "\n":
                    break
                position += 1
            prev = quote
            continue
        if char == "`":
            position = scan_template(text, position + 1)
            prev = "`"
            continue
        if char == "/" and (prev in REGEX_PREV or prev == ""):
            position = scan_regex(text, position)
            prev = "/"
            continue
        if char in "([{":
            depth += 1
            prev = char
            position += 1
            continue
        if char in ")]}":
            depth -= 1
            if not depth:
                return position
            prev = char
            position += 1
            continue
        if not char.isspace():
            prev = char
        position += 1
    raise ValueError("unterminated brace at %d" % open_index)


# ------------------------------------------------------------------ inventory

def quote_part(part):
    return urllib.parse.quote(part, safe="")


def test_id(*parts):
    return "|".join(quote_part(part) for part in parts)


def seconds(milliseconds):
    """Measured milliseconds as seconds text, 3 decimals.

    A case under 0.5 ms would render as `0.000`, which reads as the placeholder
    the canonical schema forbids; those keep the same measurement at 6 decimals.
    """
    value = milliseconds / 1000.0
    text = "%.3f" % value
    if text == "0.000" and value > 0:
        text = ("%.9f" % value).rstrip("0")
        if text.endswith("."):
            text += "0"
    return text


def template_regex(literal):
    """Regex matching the runtime name produced by a template literal."""
    inner = literal[1:-1]
    pieces = re.split(r"\$\{[^}]*\}", inner)
    return re.compile("^" + ".*".join(re.escape(piece) for piece in pieces) + "$")


def main():
    lines = load_log()

    start = None
    for index, line in enumerate(lines):
        if line.startswith("##[group]Run node --test "):
            start = index + 1
            break
    if start is None:
        raise SystemExit("Extension tests step not found")
    end = start
    while end < len(lines) and not lines[end].startswith("# duration_ms "):
        end += 1
    cases, summary, markers = parse_tap(lines, start, end + 1)
    if len(cases) != 106 or summary.get("tests") != 106 or summary.get("plan") != 106:
        raise SystemExit("batch is not the expected 106 cases: %r" % (summary,))
    if [case["number"] for case in cases] != list(range(1, 107)):
        raise SystemExit("batch numbering is not 1..106")
    if not all(case["ok"] for case in cases) or summary.get("fail") != 0:
        raise SystemExit("a batch case did not pass")
    if any(case["duration_ms"] is None for case in cases):
        raise SystemExit("a batch case has no duration_ms")

    sites = {path: find_sites(path) for path in BATCH_FILES}

    def matches(site, name):
        if site["name"] == name:
            return True
        return bool(site["template"] and template_regex(site["literal"]).match(name))

    # The stream emits one contiguous block per file, in the file's declaration
    # order, but two files declare identically named cases. Solve the partition
    # of the 106 names into nine consecutive segments, one per file.
    solutions = []

    def solve(position, remaining, chosen):
        if not remaining:
            if position == len(cases):
                solutions.append(list(chosen))
            return
        for path in remaining:
            block = sites[path]
            if position + len(block) > len(cases):
                continue
            if all(matches(site, cases[position + offset]["name"])
                   for offset, site in enumerate(block)):
                chosen.append(path)
                solve(position + len(block), [item for item in remaining if item != path], chosen)
                chosen.pop()

    solve(0, list(BATCH_FILES), [])
    if len(solutions) != 1:
        raise SystemExit("ambiguous or missing file segmentation: %d solutions" % len(solutions))

    attribution = {}
    cursor = 0
    for path in solutions[0]:
        for site in sites[path]:
            attribution[cases[cursor]["number"]] = (path, site)
            cursor += 1
    if cursor != len(cases):
        raise SystemExit("segmentation did not cover every case")

    seen = {}
    for case in cases:
        path, site = attribution[case["number"]]
        seen.setdefault(path, []).append(site["line"])
    for path, numbers in seen.items():
        expected = [site["line"] for site in sites[path]]
        if numbers != expected:
            raise SystemExit("source order mismatch in %s: %r vs %r" % (path, numbers, expected))

    rows = []
    loop_notes = []
    per_file = {}
    for case in cases:
        path, site = attribution[case["number"]]
        per_file[path] = per_file.get(path, 0) + 1
        start_line = site["loop_line"] or site["line"]
        rows.append([
            test_id("node-test", BATCH_STEP, path, case["name"], "default"),
            "", REVISION, BATCH_STEP, "node-test", path, "node",
            case["name"], "default", path, start_line, site["end_line"],
            seconds(case["duration_ms"]), seconds(case["duration_ms"]), "1", "0",
            "hosted-single-run",
            "TAP duration_ms %s at macOS-job.log:%d; run %s job %s step '%s'"
            % (case["duration_ms"], case["duration_line"], RUN, JOB, BATCH_STEP),
        ])
        if site["loop_line"]:
            loop_notes.append(
                "| %d | `%s` | %d | `%s` | `%s` |"
                % (case["number"], case["name"], site["loop_line"], path, site["literal"]))

    parents = {}
    nested_rows = []
    for step, path, rust_filter, expected in NESTED:
        group = None
        for index, line in enumerate(lines):
            if "cargo test" in line and rust_filter in line and "--ignored" in line:
                group = index + 1
                break
        if group is None:
            raise SystemExit("nested step %r not found" % step)

        parent_name = None
        for index in range(group, min(group + 400, len(lines))):
            found = re.match(r"^test (\S+) \.\.\. ", lines[index])
            if found:
                parent_name = found.group(1)
                parent_line = index + 1
                break
        if parent_name is None:
            raise SystemExit("parent test name for %r not found" % step)
        if parent_name != rust_filter:
            raise SystemExit("parent name %r differs from the workflow filter %r"
                             % (parent_name, rust_filter))

        tap_start = None
        for index in range(group, min(group + 400, len(lines))):
            if lines[index] == "TAP version 13":
                tap_start = index + 1
                break
        if tap_start is None:
            raise SystemExit("no nested TAP for %r" % step)
        tap_end = tap_start
        while tap_end < len(lines) and not lines[tap_end].startswith("# duration_ms "):
            tap_end += 1
        nested_cases, nested_summary, nested_markers = parse_tap(lines, tap_start, tap_end + 1)
        if len(nested_cases) != expected or nested_summary.get("tests") != expected:
            raise SystemExit("nested case count for %r is %d" % (step, len(nested_cases)))
        if any(case["duration_ms"] is None for case in nested_cases):
            raise SystemExit("a nested case has no duration_ms")

        cargo_result = None
        for index in range(tap_end, min(group + 400, len(lines))):
            if lines[index].startswith("test result: "):
                cargo_result = (index + 1, lines[index])
                break
        if cargo_result is None:
            raise SystemExit("no cargo test result for %r" % step)
        parent_id = test_id("cargo-test", step, "ovrcr::lib", parent_name, "production")
        parents[step] = {
            "parent_test_name": parent_name,
            "parent_test_id": parent_id,
            "parent_log_line": parent_line,
            "children": nested_cases,
            "summary": nested_summary,
            "markers": nested_markers,
            "tap_range": (tap_start, tap_end + 1),
            "source": path,
            "cargo_result": cargo_result,
        }
        found_sites = find_sites(path, unresolved_count=expected)
        if len(found_sites) != len(nested_cases):
            raise SystemExit("%s has %d source slots for %d nested cases"
                             % (path, len(found_sites), len(nested_cases)))
        for case, site in zip(nested_cases, found_sites):
            if not matches(site, case["name"]):
                raise SystemExit("nested case %r does not belong to %s:%d"
                                 % (case["name"], path, site["line"]))
            start_line = site["loop_line"] or site["line"]
            nested_rows.append([
                test_id("node-test", step, path, case["name"], "production"),
                parent_id, REVISION, step, "node-test", path, "node",
                case["name"], "production", path, start_line, site["end_line"],
                seconds(case["duration_ms"]), seconds(case["duration_ms"]), "1", "0",
                "hosted-single-run",
                "nested TAP duration_ms %s at macOS-job.log:%d inside parent '%s' "
                "(run %s job %s step '%s')"
                % (case["duration_ms"], case["duration_line"], parent_name, RUN, JOB, step),
            ])
        if len(found_sites) != expected:
            raise SystemExit("%s has %d test sites, expected %d"
                             % (path, len(found_sites), expected))
        parents[step]["slots"] = found_sites

    with (EVIDENCE / "inventory_node.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(HEADER)
        writer.writerows(rows + nested_rows)

    with (EVIDENCE / "node-tap.txt").open("w", encoding="utf-8") as handle:
        handle.write("# Raw TAP extracted from macos-job.log\n")
        handle.write("# run %s job %s revision %s\n" % (RUN, JOB, REVISION))
        handle.write("# batch step '%s': log lines %d-%d, %d cases, duration_ms %s\n"
                     % (BATCH_STEP, start, end + 1, len(cases), summary.get("duration_ms")))
        for case in cases:
            handle.write("%s\t%s\n" % (seconds(case["duration_ms"]), case["name"]))
        for step, path, rust_filter, expected in NESTED:
            info = parents[step]
            handle.write("\n# nested step '%s' (%s): log lines %d-%d, parent '%s', %d cases\n"
                         % (step, path, info["tap_range"][0], info["tap_range"][1],
                            info["parent_test_name"], len(info["children"])))
            for case in info["children"]:
                handle.write("%s\t%s\n" % (seconds(case["duration_ms"]), case["name"]))

    total_ms = sum(case["duration_ms"] for case in cases)
    subs = sum(1 for case in cases if "%.3f" % (case["duration_ms"] / 1000.0) == "0.000")
    write_notes(rows, nested_rows, parents, per_file, summary, markers, total_ms,
                loop_notes, sites, subs)

    print("batch cases=%d sum_duration=%.3fs reported_tests=%s"
          % (len(cases), total_ms / 1000.0, summary["tests"]))
    print("per-file: %s" % (per_file,))
    for step, info in parents.items():
        print("parent step '%s'" % step)
        print("  name: %s" % info["parent_test_name"])
        print("  id:   %s" % info["parent_test_id"])
        print("  children: %s" % ([case["name"] for case in info["children"]],))
    print("rows=%d nested=%d total=%d" % (len(rows), len(nested_rows), len(rows) + len(nested_rows)))


def write_notes(rows, nested_rows, parents, per_file, summary, markers, total_ms,
                loop_notes, sites, subs):
    with (EVIDENCE / "node-notes.md").open("w", encoding="utf-8") as handle:
        handle.write("# Node inventory notes\n\n")
        handle.write("Run %s, job %s, revision `%s`.\n\n" % (RUN, JOB, REVISION))

        handle.write("## The 106-case batch\n\n")
        handle.write("`.github/workflows/ci.yml` step `%s` runs nine files in one\n"
                     "`node --test` invocation. The log emits one TAP stream:\n\n" % BATCH_STEP)
        handle.write("| fact | value | log line |\n|---|---|---|\n")
        handle.write("| TAP plan | `1..%s` | %s |\n" % (summary.get("plan"), markers.get("plan")))
        handle.write("| reported tests | %s | %s |\n" % (summary.get("tests"), markers.get("tests")))
        handle.write("| passed | %s | - |\n" % summary.get("pass"))
        handle.write("| failed | %s | - |\n" % summary.get("fail"))
        handle.write("| reported duration_ms | %s | %s |\n"
                     % (summary.get("duration_ms"), markers.get("duration_ms")))
        handle.write("| sum of per-case duration_ms | %.3f | - |\n\n" % total_ms)
        handle.write("Seconds are the TAP `duration_ms` at 3 decimals. %d cases run under\n"
                     "0.5 ms and would round to `0.000`, which the canonical inventory schema\n"
                     "forbids as a timing cell; those rows keep the identical measurement at full\n"
                     "printed precision (e.g. `duration_ms 0.161417` -> `0.000161417`). No value is\n"
                     "rounded up and none is inferred.\n\n" % subs)
        handle.write("The sum of the per-case durations exceeds the stream's own duration\n"
                     "because `node --test` runs the nine files concurrently, so case durations\n"
                     "overlap; the batch row timings must never be added up or compared against\n"
                     "the stream total.\n\n")
        handle.write("`# suites 0`: the reporter emits flat names, so `test_name` is the\n"
                     "exact TAP name with no describe prefix joined by ` > `.\n\n")
        handle.write("Rows: 106 (one per runner-reported case) + 4 nested rows = %d.\n\n"
                     % (len(rows) + len(nested_rows)))
        handle.write("| file | cases |\n|---|---|\n")
        for path in BATCH_FILES:
            handle.write("| `%s` | %d |\n" % (path, per_file.get(path, 0)))
        handle.write("| total | %d |\n\n" % sum(per_file.values()))
        handle.write("The TAP stream reports each file as one contiguous block in\n"
                     "declaration order; the block order in the log is completion order,\n"
                     "not the order of the files on the command line.\n\n")

        handle.write("## Loop-generated cases\n\n")
        handle.write("These cases come from a table/loop, so `source_start..source_end` is\n"
                     "the loop's line range (the runner still reports one case per element):\n\n")
        handle.write("| TAP # | test name | loop line | file | source template |\n"
                     "|---|---|---|---|---|\n")
        handle.write("\n".join(loop_notes) + "\n\n")

        handle.write("## Nested cases under Rust parents\n\n")
        handle.write("Two `cargo test` steps run one Rust test each; the Rust test spawns\n"
                     "`node --test` on a dedicated .mjs file and the nested TAP stream appears\n"
                     "under `--nocapture`. They are not part of the 106.\n\n")
        for step, info in parents.items():
            handle.write("- step `%s`\n" % step)
            handle.write("  - parent test name (macOS-job.log:%d): `%s`\n"
                         % (info["parent_log_line"], info["parent_test_name"]))
            handle.write("  - parent_test_id: `%s`\n" % info["parent_test_id"])
            handle.write("  - suite component is `ovrcr::lib` per the shared construction;\n"
                         "    the reporting cargo target is `%s`\n"
                         % ("tests/pi_package.rs" if "pi_package" in info["source"]
                            else "tests/pi_delegation.rs"))
            handle.write("  - nested TAP lines %d-%d, %d cases, duration_ms %s\n"
                         % (info["tap_range"][0], info["tap_range"][1],
                            len(info["children"]), info["summary"].get("duration_ms")))
            handle.write("  - source slots: %d (`%s`)\n"
                         % (len(info["slots"]), info["source"]))
            handle.write("  - cargo reported at macOS-job.log:%d: `%s`\n\n"
                         % (info["cargo_result"][0], info["cargo_result"][1]))
        handle.write("Timing deviation from the assignment brief: the brief stated per-child\n"
                     "timing is unmeasured because the parent's duration covers it. The parent\n"
                     "steps do print the nested TAP stream, and it reports `duration_ms` per\n"
                     "child, so those measured values are recorded (seconds, 3 decimals) with\n"
                     "`timing_basis=hosted-single-run`, `attempt_count=1` (each parent ran once\n"
                     "with `--test-threads=1`) and `failed_attempt_count=0`. No duration was\n"
                     "inferred from the parent step's wall time.\n\n")

        handle.write("## Excluded file\n\n")
        handle.write("1. `tests/pi_publish_contract_installed.mjs` — a `node:test` file with its\n"
                     "   own cases. It is NOT invoked by this job: it is absent from the nine-file\n"
                     "   batch, absent from both nested steps, and no other file in the job\n"
                     "   references it. It stays outside the scored inventory.\n"
                     "2. `tests/fixtures/pi/pi_publish_contract.mjs` — a helper module, not a test\n"
                     "   file: it exports `assertPublishContract` and declares no cases. The job\n"
                     "   does execute it indirectly, because both\n"
                     "   `tests/pi_child_tools_installed.mjs` (in this job) and\n"
                     "   `tests/pi_publish_contract_installed.mjs` (not in this job) import it.\n"
                     "   Being a helper, it contributes no row of its own.\n\n")

        handle.write("## Source facts\n\n")
        for step, info in parents.items():
            handle.write("- `%s`: %d source slots (%d after loop expansion)\n"
                         % (info["source"], len(info["slots"]), len(info["slots"])))
        handle.write("\nBoth nested files fail closed without `OVRCR_TEST_PI_PACKAGE_ROOT`; the\n"
                     "job provisions the pinned installed host in the preceding step, which is why\n"
                     "they are separated from the ordinary batch.\n")
        handle.write("\nThe only other Node-shaped output in this job is none; every other\n"
                     "check is Python, inventoried in `inventory_python.csv`.\n")


if __name__ == "__main__":
    main()
