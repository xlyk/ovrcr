#!/usr/bin/env python3
"""Map every executed nextest test to its Rust definition.

Builds a module tree per Cargo target (lib/bin/integration), following `mod`
declarations including `#[path]`, and matches the authoritative executed
(binary_id, test_name) set from attempts.csv against function definitions.
"""

import csv
import re
import sys
import tomllib
from pathlib import Path

EVIDENCE = Path(__file__).resolve().parent
REPO = EVIDENCE.parents[2]
ATTEMPTS = EVIDENCE / "attempts.csv"
LOG = EVIDENCE / "macos-job.log"
OUT = EVIDENCE / "inventory_rust.csv"


# --------------------------------------------------------------------------
# Minimal Rust tokenizer (comments and string contents skipped, positions kept)
# --------------------------------------------------------------------------

class Tok:
    __slots__ = ("kind", "value", "line")

    def __init__(self, kind, value, line):
        self.kind = kind
        self.value = value
        self.line = line

    def __repr__(self):
        return f"Tok({self.kind},{self.value!r},L{self.line})"


IDENT_START = re.compile(r"[A-Za-z_]")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def tokenize(text):
    toks = []
    i = 0
    n = len(text)
    line = 1
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
            continue
        if c in " \t\r":
            i += 1
            continue
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if text.startswith("/*", i):
            depth = 1
            i += 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    if text[i] == "\n":
                        line += 1
                    i += 1
            continue
        # raw strings: r"..." r#"..."#
        m = re.match(r'b?r(#*)"', text[i:i + 8])
        if m and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            hashes = m.group(1)
            start_line = line
            i += m.end()
            closing = '"' + hashes
            j = text.find(closing, i)
            j = n if j < 0 else j
            value = text[i:j]
            line += text.count("\n", i, j)
            toks.append(Tok("str", value, start_line))
            i = j + len(closing)
            continue
        if c == '"':
            start_line = line
            i += 1
            buf = []
            while i < n:
                ch = text[i]
                if ch == "\\":
                    buf.append(text[i:i + 2])
                    if text[i + 1:i + 2] == "\n":
                        line += 1
                    i += 2
                    continue
                if ch == '"':
                    i += 1
                    break
                if ch == "\n":
                    line += 1
                buf.append(ch)
                i += 1
            toks.append(Tok("str", "".join(buf), start_line))
            continue
        if c == "'":
            # char literal vs lifetime
            m = re.match(r"'(?:\\.|[^'\\\n])'", text[i:i + 8])
            if m:
                toks.append(Tok("char", m.group(0), line))
                i += m.end()
                continue
            i += 1
            continue
        if IDENT_START.match(c):
            m = IDENT.match(text, i)
            toks.append(Tok("ident", m.group(0), line))
            i = m.end()
            continue
        if c.isdigit():
            m = re.match(r"[0-9][0-9A-Za-z_.]*", text[i:])
            toks.append(Tok("num", m.group(0), line))
            i += len(m.group(0))
            continue
        toks.append(Tok("punct", c, line))
        i += 1
    return toks


def P(tok, value):
    """True when tok is that punctuation character (string contents excluded)."""
    return tok.kind == "punct" and tok.value == value


def match_close(toks, i):
    """Given index of an opening bracket/paren/brace, return matching close."""
    pairs = {"{": "}", "(": ")", "[": "]"}
    open_ch = toks[i].value
    close_ch = pairs[open_ch]
    depth = 0
    j = i
    while j < len(toks):
        if P(toks[j], open_ch):
            depth += 1
        elif P(toks[j], close_ch):
            depth -= 1
            if depth == 0:
                return j
        j += 1
    raise ValueError(f"unbalanced {open_ch} at line {toks[i].line}")


# --------------------------------------------------------------------------
# Target discovery
# --------------------------------------------------------------------------

def workspace_members():
    root = tomllib.loads((REPO / "Cargo.toml").read_text())
    members = root["workspace"]["members"]
    dirs = []
    for m in members:
        d = (REPO / m).resolve()
        dirs.append(d)
    return dirs


def discover_targets():
    """Return {binary_id: (package, kind, name, root_path)}."""
    targets = {}
    for d in workspace_members():
        manifest_path = d / "Cargo.toml"
        if not manifest_path.exists():
            continue
        m = tomllib.loads(manifest_path.read_text())
        pkg = m["package"]["name"]
        pkg_dir = manifest_path.parent
        # lib
        lib = m.get("lib")
        lib_path = None
        if lib and "path" in lib:
            lib_path = pkg_dir / lib["path"]
        elif (pkg_dir / "src/lib.rs").exists():
            lib_path = pkg_dir / "src/lib.rs"
        if lib_path:
            targets[pkg] = (pkg, "lib", pkg, lib_path)
        # bins
        bins = {}
        for b in m.get("bin", []):
            bins[b["name"]] = pkg_dir / b["path"]
        if (pkg_dir / "src/main.rs").exists() and pkg not in bins:
            bins.setdefault(pkg, pkg_dir / "src/main.rs")
        bin_dir = pkg_dir / "src/bin"
        if bin_dir.is_dir():
            for f in sorted(bin_dir.glob("*.rs")):
                bins.setdefault(f.stem, f)
        for name, path in bins.items():
            targets[f"{pkg}::bin/{name}"] = (pkg, "bin", name, path)
        # integration tests
        tests = {}
        for t in m.get("test", []):
            tests[t["name"]] = pkg_dir / t["path"]
        tdir = pkg_dir / "tests"
        if tdir.is_dir():
            for f in sorted(tdir.glob("*.rs")):
                tests.setdefault(f.stem, f)
            for f in sorted(tdir.iterdir()):
                if f.is_dir() and (f / "main.rs").exists():
                    tests.setdefault(f.name, f / "main.rs")
        for name, path in tests.items():
            targets[f"{pkg}::{name}"] = (pkg, "integration", name, path)
    return targets


# --------------------------------------------------------------------------
# Module tree
# --------------------------------------------------------------------------

class Def:
    __slots__ = ("module", "name", "file", "start", "end", "attrs", "body")

    def __init__(self, module, name, file, start, end, attrs, body):
        self.module = module
        self.name = name
        self.file = file
        self.start = start
        self.end = end
        self.attrs = attrs
        self.body = body


def read_text(path):
    return path.read_text(encoding="utf-8", errors="replace")


def is_root_file(path):
    return path.stem in ("lib", "main", "mod")


def resolve_module_file(current_file, name, path_attr):
    if path_attr is not None:
        cand = (current_file.parent / path_attr).resolve()
        if cand.exists():
            return cand
    if is_root_file(current_file):
        base = current_file.parent
    else:
        base = current_file.parent / current_file.stem
    for cand in (base / f"{name}.rs", base / name / "mod.rs"):
        if cand.exists():
            return cand.resolve()
    return None


def attr_text(attr_toks):
    return " ".join(t.value for t in attr_toks)


def path_attr_of(attr_toks):
    txt = attr_text(attr_toks)
    if "path" not in txt:
        return None
    for i, t in enumerate(attr_toks):
        if t.value == "path":
            for u in attr_toks[i:]:
                if u.kind == "str":
                    return u.value
    return None


def is_item_ident(tok):
    return tok.kind == "ident"


def parse_fn(toks, i, end, text):
    """Parse a `fn` item at index i; return (name, start, end, body, next_index)."""
    if i + 1 >= end or toks[i + 1].kind != "ident":
        return None
    name = toks[i + 1].value
    j = i + 2
    while j < end and not P(toks[j], "(") and not P(toks[j], ";"):
        j += 1
    if j >= end or not P(toks[j], "("):
        return None
    pclose = match_close(toks, j)
    k = pclose + 1
    while k < end and not P(toks[k], "{") and not P(toks[k], ";"):
        k += 1
    if k >= end or not P(toks[k], "{"):
        return None
    close = match_close(toks, k)
    start_line = toks[i].line
    end_line = toks[close].line
    body = "\n".join(text.splitlines()[start_line - 1:end_line])
    return name, start_line, end_line, body, close + 1


class TargetTree:
    def __init__(self, root):
        self.root = root
        self.defs = []
        self.helper_fns = {}
        self.helper_types = {}
        self._texts = {}
        self.depth_guard = 0
        self._walk_file(root, (), [])

    def _text_of(self, file):
        if file not in self._texts:
            self._texts[file] = read_text(file)
        return self._texts[file]

    def _walk_file(self, path, module, attrs):
        self.depth_guard += 1
        if self.depth_guard > 200:
            raise RuntimeError(f"module recursion too deep at {path}")
        try:
            toks = tokenize(read_text(path))
            self._walk(toks, 0, len(toks), module, path, attrs)
        finally:
            self.depth_guard -= 1

    def _walk(self, toks, i, end, module, file, inherited_attrs):
        pending = list(inherited_attrs)
        while i < end:
            t = toks[i]
            v = t.value
            if P(t, "#"):
                j = i + 1
                is_inner = False
                if j < end and P(toks[j], "!"):
                    is_inner = True
                    j += 1
                if j < end and P(toks[j], "["):
                    close = match_close(toks, j)
                    attr = toks[i:close + 1]
                    if not is_inner:
                        pending.append(attr)
                    i = close + 1
                    continue
                i += 1
                continue
            if t.kind == "ident" and v == "mod":
                name_tok = toks[i + 1] if i + 1 < end else None
                if name_tok is None or name_tok.kind != "ident":
                    i += 1
                    continue
                name = name_tok.value
                j = i + 2
                while j < end and not (P(toks[j], ";") or P(toks[j], "{")):
                    j += 1
                if j >= end:
                    i += 1
                    continue
                if P(toks[j], ";"):
                    p = None
                    for a in pending:
                        p = path_attr_of(a)
                        if p:
                            break
                    child = resolve_module_file(file, name, p)
                    if child is not None:
                        self._walk_file(child, module + (name,), [])
                    else:
                        print(f"UNRESOLVED MODULE {file}:{t.line} mod {name}", file=sys.stderr)
                    i = j + 1
                else:
                    close = match_close(toks, j)
                    self._walk(toks, j + 1, close, module + (name,), file, [])
                    i = close + 1
                pending = []
                continue
            if t.kind == "ident" and v == "macro_rules":
                j = i + 1
                while j < end and not P(toks[j], "{"):
                    j += 1
                if j < end:
                    close = match_close(toks, j)
                    i = close + 1
                else:
                    i += 1
                pending = []
                continue
            if t.kind == "ident" and v == "include":
                j = i + 1
                if j < end and P(toks[j], "!"):
                    j += 1
                if j < end and P(toks[j], "("):
                    close = match_close(toks, j)
                    for u in toks[j:close]:
                        if u.kind == "str":
                            child = (file.parent / u.value).resolve()
                            if child.exists():
                                self._walk_file(child, module, [])
                            else:
                                print(f"UNRESOLVED INCLUDE {file}:{t.line} {u.value}", file=sys.stderr)
                            break
                    i = close + 1
                    continue
                i += 1
                continue
            if t.kind == "ident" and v == "fn":
                parsed = parse_fn(toks, i, end, self._text_of(file))
                if parsed is not None:
                    name, start, stop, body, nxt = parsed
                    attrs = [attr_text(a) for a in pending]
                    self.defs.append(Def(module, name, file, start, stop, attrs, body))
                    if not any(is_test_attr(a) for a in attrs):
                        self.helper_fns.setdefault(name, []).append(body)
                    i = nxt
                    pending = []
                    continue
                i += 1
                pending = []
                continue
            if t.kind == "ident" and v == "impl":
                j = i + 1
                if j < end and P(toks[j], "<"):
                    depth = 0
                    while j < end:
                        if P(toks[j], "<"):
                            depth += 1
                        elif P(toks[j], ">"):
                            depth -= 1
                        j += 1
                        if depth == 0:
                            break
                type_name = None
                first_ident = None
                after_for = False
                while j < end and not P(toks[j], "{"):
                    if toks[j].kind == "ident":
                        if toks[j].value == "for":
                            after_for = True
                        elif after_for and type_name is None:
                            type_name = toks[j].value
                        elif first_ident is None:
                            first_ident = toks[j].value
                    j += 1
                if j < end:
                    close = match_close(toks, j)
                    type_name = type_name or first_ident
                    k = j + 1
                    while k < close:
                        if toks[k].kind == "ident" and toks[k].value == "fn":
                            parsed = parse_fn(toks, k, close, self._text_of(file))
                            if parsed is not None:
                                name, _s, _e, body, nxt = parsed
                                self.helper_fns.setdefault(name, []).append(body)
                                if type_name:
                                    self.helper_types.setdefault(type_name, []).append(body)
                                k = nxt
                                continue
                        k += 1
                    i = close + 1
                else:
                    i += 1
                pending = []
                continue
            if t.kind == "ident" and i + 1 < end and P(toks[i + 1], "!"):
                # macro invocation: skip its delimiter group
                j = i + 2
                if j < end and (P(toks[j], "{") or P(toks[j], "(") or P(toks[j], "[")):
                    close = match_close(toks, j)
                    i = close + 1
                else:
                    i = j
                pending = []
                continue
            if P(t, "{"):
                # impl/trait/enum body at this scope: skip
                close = match_close(toks, i)
                i = close + 1
                pending = []
                continue
            i += 1
        return

    def lookup(self, test_name):
        parts = test_name.split("::")
        fn = parts[-1]
        module = tuple(parts[:-1])
        hits = [d for d in self.defs if d.module == module and d.name == fn]
        return hits


RUN = "42429838269"
JOB = "128576086691"
REVISION = "992a1da5c59a865ebb48051e4d9d8ff7184c8dc5"
JOB_URL = "https://github.com/xlyk/ovrcr/actions/runs/42429838269/job/128576086691"
INVOCATION = "Test"
TIMING_BASIS = "hosted-single-run"
TIMING_SUFFIX = f"run {RUN} job {JOB}"
MERGED_FIELDS = [
    "test_id", "parent_test_id", "revision", "ci_step", "runner", "suite",
    "target_kind", "test_name", "variant", "source_file", "source_start",
    "source_end", "first_attempt_seconds", "total_attempt_seconds",
    "attempt_count", "failed_attempt_count", "timing_basis", "timing_source",
]
INVENTORY_FIELDS = [
    "test_id", "input_test_name", "invocation", "runner", "suite", "target_kind",
    "test_level", "test_name", "variant", "source_file", "source_start", "source_end",
    "first_attempt_seconds", "total_attempt_seconds", "attempt_count",
    "failed_attempt_count", "timing_basis", "timing_source",
]


def strip_comments(text):
    """Drop // and /* */ comments while leaving string/char literals intact."""
    out = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            i += 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            continue
        if c == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(text[i:j])
            i = j
            continue
        out.append(c)
        i += 1
    return "".join(out)


def is_test_attr(attr):
    txt = re.sub(r"\s+", " ", attr).strip()
    m = re.match(r"#\s*\[\s*((?:[A-Za-z_][\w]*\s*::\s*)*[A-Za-z_][\w]*)", txt)
    if not m:
        return False
    path = re.sub(r"\s+", "", m.group(1))
    return path == "test" or path.endswith("::test") or path in ("rstest", "proptest")


def is_ignore_attr(attr):
    return re.search(r"\[\s*ignore\b", re.sub(r"\s+", " ", attr)) is not None


def is_test_def(d):
    return any(is_test_attr(a) for a in d.attrs)


def ignore_reason(d):
    for a in d.attrs:
        if is_ignore_attr(a):
            txt = re.sub(r"\s+", " ", a).strip()
            m = re.search(r"ignore\s*=\s*(.+?)\s*\]$", txt)
            if m:
                return m.group(1)
            return "ignored in source (#[ignore])"
    return ""


def classify(d, tree=None):
    body = strip_comments(d.body)
    # Composition is a property of the test body itself; helper bodies may
    # surface external boundaries but must not create component breadth.
    own_comps = set(
        re.findall(r"(?:crate|super(?:\s*::\s*super)*)\s*::\s*([a-z_][a-z0-9_]*)", body)
    )
    if tree is not None:
        extra = []
        types = set(re.findall(r"\b([A-Z][A-Za-z0-9_]*)\s*(?:::|\s*\{)", body))
        for name in types:
            bodies = tree.helper_types.get(name, [])
            if bodies and len(bodies) <= 8:
                extra.extend(bodies)
        calls = set(re.findall(r"(?<![\w:])([a-z_][a-z0-9_]*)\s*\(", body))
        for name in calls:
            bodies = tree.helper_fns.get(name, [])
            if len(bodies) == 1:
                extra.extend(bodies)
        if extra:
            body = body + "\n" + "\n".join(extra)[:400000]
    if "CARGO_BIN_EXE_" in body:
        return "acceptance"
    if re.search(r"portable_pty|PtyPair|openpty", body):
        return "acceptance"
    if re.search(r"\.mjs\b|node --test|npm run|npx ", body):
        return "acceptance"
    if re.search(r"\bLive\s*::|\blive\.(?:command|root|socket|server)\b", body):
        return "acceptance"
    if re.search(r"tempdir|TempDir|tempfile|std::fs|\bfs::|rusqlite|Connection::open|\.sqlite\b", body):
        return "integration"
    if re.search(r"Command::new|std::process|UnixStream|TcpStream|TcpListener", body):
        return "integration"
    if re.search(r"\blive::", body):
        return "integration"
    if len(own_comps) >= 2:
        return "integration"
    if re.search(r"DashboardProxy|ControlFixture|GitFixture", body):
        return "integration"
    return "unit"


def test_id(runner, invocation, suite, test_name, variant):
    import urllib.parse
    parts = (runner, invocation, suite, test_name, variant)
    return "|".join(urllib.parse.quote(x, safe="") for x in parts)


def log_line_index():
    idx = {}
    for n, line in enumerate(LOG.read_text(encoding="utf-8-sig", errors="replace").splitlines(), 1):
        idx.setdefault(line, n)
    return idx




def write_merged(inv_rows):
    """Merge Rust rows with the verified Node/Python fragments into inventory.csv."""
    node_rows = [
        {k: r[k] for k in MERGED_FIELDS}
        for r in csv.DictReader((EVIDENCE / "inventory_node.csv").open(encoding="utf-8"))
    ]
    py_rows = [
        {k: r[k] for k in MERGED_FIELDS}
        for r in csv.DictReader((EVIDENCE / "inventory_python.csv").open(encoding="utf-8"))
    ]
    rust_rows = []
    for r in inv_rows:
        rust_rows.append(
            {
                "test_id": r["test_id"],
                "parent_test_id": "",
                "revision": REVISION,
                "ci_step": r["invocation"],
                "runner": r["runner"],
                "suite": r["suite"],
                "target_kind": r["target_kind"],
                "test_name": r["test_name"],
                "variant": r["variant"],
                "source_file": r["source_file"],
                "source_start": r["source_start"],
                "source_end": r["source_end"],
                "first_attempt_seconds": r["first_attempt_seconds"],
                "total_attempt_seconds": r["total_attempt_seconds"],
                "attempt_count": r["attempt_count"],
                "failed_attempt_count": r["failed_attempt_count"],
                "timing_basis": r["timing_basis"],
                "timing_source": r["timing_source"],
            }
        )
    merged = rust_rows + node_rows + py_rows
    with (EVIDENCE / "inventory.csv").open("w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=MERGED_FIELDS, quoting=csv.QUOTE_MINIMAL)
        w.writeheader()
        w.writerows(merged)

    with (EVIDENCE / "inventory_rust_test_level.csv").open("w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh)
        w.writerow(["test_id", "test_level"])
        for r in inv_rows:
            w.writerow([r["test_id"], r["test_level"]])

    # ---- merge gates ----
    import collections

    line_cache = {}

    def line_count(rel):
        if rel not in line_cache:
            p = REPO / rel
            line_cache[rel] = len(p.read_text(encoding="utf-8", errors="replace").splitlines()) if p.exists() else -1
        return line_cache[rel]

    runner_counts = collections.Counter(r["runner"] for r in merged)
    ids = collections.Counter(r["test_id"] for r in merged)
    id_set = set(ids)
    parent_rows = [r for r in merged if r["parent_test_id"]]
    unresolved_parents = sorted({r["parent_test_id"] for r in parent_rows if r["parent_test_id"] not in id_set})
    bad_sources = []
    for r in merged:
        n = line_count(r["source_file"])
        try:
            s0, s1 = int(r["source_start"]), int(r["source_end"])
        except ValueError:
            bad_sources.append((r["test_id"], r["source_file"], r["source_start"], r["source_end"]))
            continue
        if n < 1 or not (1 <= s0 <= s1 <= n):
            bad_sources.append((r["test_id"], r["source_file"], s0, s1, n))
    bad_revisions = [r["test_id"] for r in merged if r["revision"] != REVISION]
    expected_cargo = {
        "cargo-test|Installed%20Pi%20package%20through%20production%20materialization|ovrcr%3A%3Alib|production_materializer_loads_local_package_with_one_injected_owner|production",
        "cargo-test|Installed%20Pi%20effective%20child%20tools%20through%20production%20assignment%20argv|ovrcr%3A%3Alib|coding%3A%3Aproduction_assignment_argv_preserves_installed_sdk_child_publication_tools|production",
    }
    cargo_ids = {r["test_id"] for r in merged if r["runner"] == "cargo-test"}
    merge_gates = {
        "total_rows": {"observed": len(merged), "expected": 3171, "pass": len(merged) == 3171},
        "nextest_rows": {"observed": runner_counts["nextest"], "expected": 2820, "pass": runner_counts["nextest"] == 2820},
        "cargo_test_rows": {"observed": runner_counts["cargo-test"], "expected": 2, "pass": runner_counts["cargo-test"] == 2},
        "node_test_rows": {"observed": runner_counts["node-test"], "expected": 110, "pass": runner_counts["node-test"] == 110},
        "python_unittest_rows": {"observed": runner_counts["python-unittest"], "expected": 39, "pass": runner_counts["python-unittest"] == 39},
        "python_check_rows": {"observed": runner_counts["python-check"], "expected": 200, "pass": runner_counts["python-check"] == 200},
        "unique_test_ids": {"observed": len(id_set), "expected": 3171, "pass": len(id_set) == 3171},
        "parent_rows": {"observed": len(parent_rows), "expected": 4, "pass": len(parent_rows) == 4},
        "parents_resolve": {"observed": unresolved_parents, "expected": [], "pass": not unresolved_parents},
        "source_ranges": {"observed": bad_sources[:5], "expected": [], "pass": not bad_sources},
        "revisions": {"observed": len(bad_revisions), "expected": 0, "pass": not bad_revisions},
        "cargo_test_ids": {"observed": sorted(cargo_ids), "expected": sorted(expected_cargo),
                           "pass": cargo_ids == expected_cargo},
    }
    return merge_gates, bad_sources


def main():
    targets = discover_targets()
    rows = list(csv.DictReader(ATTEMPTS.open(encoding="utf-8")))
    attempts = {}
    for r in rows:
        attempts.setdefault((r["binary_id"], r["test_name"]), []).append(r)

    missing_targets = sorted({b for b, _ in attempts if b not in targets})
    if missing_targets:
        raise SystemExit(f"UNMAPPED BINARY IDS: {missing_targets}")

    trees = {}
    resolved_defs = {}
    unresolved = []
    ambiguous = []
    for (binary, name) in sorted(attempts):
        if binary not in trees:
            trees[binary] = TargetTree(targets[binary][3])
        hits = trees[binary].lookup(name)
        if len(hits) == 1:
            resolved_defs[(binary, name)] = hits[0]
        elif not hits:
            unresolved.append((binary, name))
        else:
            ambiguous.append((binary, name, hits))
    if unresolved or ambiguous:
        raise SystemExit(
            f"hard finding: unresolved={unresolved[:5]} ambiguous={ambiguous[:5]}"
        )

    lines = log_line_index()
    inv_rows = []
    for (binary, name) in sorted(attempts):
        recs = sorted(attempts[(binary, name)], key=lambda r: int(r["attempt"]))
        d = resolved_defs[(binary, name)]
        first = float(recs[0]["seconds"])
        total = sum(float(r["seconds"]) for r in recs)
        failed = sum(1 for r in recs if r["result"] == "FAIL")
        log_lines = sorted({lines[r["raw_line"]] for r in recs if r["raw_line"] in lines})
        src = (
            "evidence/macos-job.log:"
            + ",".join(str(n) for n in log_lines)
            + f" ({recs[0]['result']} attempt 1); {TIMING_SUFFIX}"
        )
        inv_rows.append(
            {
                "test_id": test_id("nextest", INVOCATION, binary, name, "default"),
                "input_test_name": name,
                "invocation": INVOCATION,
                "runner": "nextest",
                "suite": binary,
                "target_kind": targets[binary][1],
                "test_level": classify(d, trees[binary]),
                "test_name": name,
                "variant": "default",
                "source_file": d.file.relative_to(REPO).as_posix(),
                "source_start": d.start,
                "source_end": d.end,
                "first_attempt_seconds": f"{first:.3f}",
                "total_attempt_seconds": f"{total:.3f}",
                "attempt_count": len(recs),
                "failed_attempt_count": failed,
                "timing_basis": TIMING_BASIS,
                "timing_source": src,
            }
        )

    # Two ignored Rust parents re-entered by dedicated workflow steps.  Test
    # names are verbatim runner/printed names; suite/variant are the canonical
    # review identity agreed for child-row joins.
    parent_specs = [
        (
            "Installed Pi package through production materialization",
            "production_materializer_loads_local_package_with_one_injected_owner",
            "ovrcr::pi_package",
            "production_materializer_loads_local_package_with_one_injected_owner",
            "5.370",
            "evidence/macos-job.log:4364,4379,4428 (command, test line, 'finished in 5.37s'); "
            + TIMING_SUFFIX,
        ),
        (
            "Installed Pi effective child tools through production assignment argv",
            "coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools",
            "ovrcr::pi_delegation",
            "coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools",
            "7.250",
            "evidence/macos-job.log:4432,4446,4481 (command, test line, 'finished in 7.25s'); "
            + TIMING_SUFFIX,
        ),
    ]
    for invocation, canonical_name, tree_binary, defined_name, seconds, src in parent_specs:
        tree = trees.get(tree_binary) or TargetTree(targets[tree_binary][3])
        hits = tree.lookup(defined_name)
        if len(hits) != 1:
            raise SystemExit(f"hard finding: parent {defined_name} -> {hits}")
        d = hits[0]
        inv_rows.append(
            {
                "test_id": test_id("cargo-test", invocation, "ovrcr::lib", canonical_name, "production"),
                "input_test_name": canonical_name,
                "invocation": invocation,
                "runner": "cargo-test",
                "suite": "ovrcr::lib",
                "target_kind": "lib",
                "test_level": classify(d, trees[binary]),
                "test_name": canonical_name,
                "variant": "production",
                "source_file": d.file.relative_to(REPO).as_posix(),
                "source_start": d.start,
                "source_end": d.end,
                "first_attempt_seconds": seconds,
                "total_attempt_seconds": seconds,
                "attempt_count": 1,
                "failed_attempt_count": 0,
                "timing_basis": TIMING_BASIS,
                "timing_source": src,
            }
        )

    with OUT.open("w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=INVENTORY_FIELDS, quoting=csv.QUOTE_MINIMAL)
        w.writeheader()
        w.writerows(inv_rows)

    # ------------------------------------------------------------------
    # Exclusions
    # ------------------------------------------------------------------
    excl = []
    excl.append(
        {
            "kind": "nextest-filter",
            "subject": "ovrcr-runtime | server::tests::partial_resize_connection_delivers_error_before_owner_close",
            "reason": "Filtered out by -E 'not test(partial_resize_connection_delivers_error_before_owner_close)'",
            "evidence": "ci.yml:48 (macOS) and ci.yml:169 (linux); source crates/ovrcr-runtime/src/server/tests.rs:2021; "
            "count reconciliation: 2820 run + 32 ignored + 1 filtered = 2853 compiled tests = log line 505 "
            "'Starting 2820 tests across 56 binaries (33 tests skipped)'",
        }
    )
    ignored = []
    macos_off = []
    for binary in sorted(trees):
        tree = trees[binary]
        for d in tree.defs:
            if not is_test_def(d):
                continue
            name = "::".join(d.module + (d.name,))
            rel = d.file.relative_to(REPO).as_posix()
            if is_ignore_attr(" ".join(d.attrs)):
                ignored.append((binary, name, rel, d.start))
                excl.append(
                    {
                        "kind": "ignored-test",
                        "subject": f"{binary} | {name}",
                        "reason": ignore_reason(d),
                        "evidence": f"{rel}:{d.start} (#[ignore]); counted in the 33 skipped on log line 505",
                    }
                )
            for a in d.attrs:
                txt = re.sub(r"\s+", " ", a)
                if "cfg" in txt and "not" in txt and "target_os" in txt and "macos" in txt:
                    macos_off.append((binary, name, rel, d.start))
                    excl.append(
                        {
                            "kind": "cfg-platform",
                            "subject": f"{binary} | {name}",
                            "reason": "#[cfg(not(target_os = \"macos\"))]",
                            "evidence": f"{rel}:{d.start}; not compiled by the macOS job (absent from the 2853 compiled tests)",
                        }
                    )
                    break
    excl.append(
        {
            "kind": "feature-gate",
            "subject": "Cargo feature gates (--all-features)",
            "reason": "The macOS Test step passes --all-features, so feature-gated targets (gui, required-features ovrcr-gui, acceptance-diagnostics) were compiled and selected; no test was excluded by a feature gate in this run",
            "evidence": "ci.yml:48",
        }
    )
    excl.append(
        {
            "kind": "node-suite",
            "subject": "tests/pi_publish_contract_installed.mjs",
            "reason": "Not invoked by this job: the Extension tests node --test list covers 9 .mjs files and no step references this file",
            "evidence": "ci.yml:74; log line 3692 (exact node --test command)",
        }
    )
    excl.append(
        {
            "kind": "doctest",
            "subject": "cargo test --workspace --doc",
            "reason": "Five per-crate doctest sections (ovrcr, ovrcr_protocol, ovrcr_runtime, ovrcr_terminal, ovrcr_tui) each ran 0 tests",
            "evidence": "evidence/macos-job.log:3416,3422,3428,3434,3440 ('running 0 tests'); no doctest rows created",
        }
    )
    with (EVIDENCE / "exclusions.csv").open("w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=["kind", "subject", "reason", "evidence"],
                           quoting=csv.QUOTE_MINIMAL)
        w.writeheader()
        w.writerows(excl)

    # ------------------------------------------------------------------
    # Job steps
    # ------------------------------------------------------------------
    import datetime
    import hashlib
    import json

    run_json = json.loads((EVIDENCE / "run.json").read_text())
    job = next(j for j in run_json["jobs"] if j["databaseId"] == 128576086691)

    step_meta = {
        "Set up job": ("setup", "runner provisioning", ""),
        "Run actions/checkout@v4": ("setup", "actions/checkout@v4", ""),
        "Run dtolnay/rust-toolchain@stable": ("setup", "rust stable + rustfmt, clippy", ""),
        "Run taiki-e/install-action@nextest": (
            "setup", "taiki-e/install-action@nextest",
            "installs cargo-nextest 0.9.151 (log 369-373)",),
        "Run Swatinem/rust-cache@v2": ("setup", "Swatinem/rust-cache@v2 (shared-key checks)", ""),
        "Install zsh for shell integration tests": (
            "setup", "apt-get install zsh (if runner.os == Linux)",
            "skipped on macOS (conclusion skipped)"),
        "Configure git identity": ("setup", "git config user.name/user.email", ""),
        "Format": ("format-lint", "cargo fmt --all -- --check", ""),
        "Lint": ("format-lint", "cargo clippy --workspace --all-targets --all-features -- -D warnings",
                 "clippy also compiles the workspace"),
        "Test": (
            "test",
            "cargo nextest run --workspace --all-targets --all-features --profile ci -E 'not test(partial_resize_connection_delivers_error_before_owner_close)'",
            "compilation 2m16s = 136s (log 502, 'Finished test profile ... in 2m 16s'); "
            "nextest execution 2014.918s (log 505 first attempt line to 3397 FLAKY summary); "
            "2820 tests / 56 binaries / 33 skipped (32 #[ignore] + 1 -E filter); 2 flaky tests retried once",
        ),
        "Doc tests": (
            "test", "cargo test --workspace --doc",
            "five per-crate doctest sections, 0 tests each (log 3416,3422,3428,3434,3440); no test rows created",
        ),
        "Relocated macOS startup assets": (
            "package-check",
            "cargo build -p ovrcr --bin ovrcr; sh scripts/package-startup.sh ...; python3 -I native/bridge/check-{local-startup,installer,startup-package,packaging,guards}.py (default + local-development)",
            "builds the CLI (log 3464: 8.31s) then validates startup assets, installer, packaging and guards",
        ),
        "Relocated pinned Cursor dependency and passive CLI diagnostics": (
            "package-check",
            "HOME=$tmp python3 -I scripts/install-cursor-bridge.py; OVRCR_CURSOR_BRIDGE_ARCHIVE=... OVRCR_TEST_EXECUTABLE=target/debug/ovrcr python3 -I tests/cursor_bridge_install.py",
            "exercises the compiled CLI through a relocated pinned Cursor dependency",
        ),
        "Run actions/setup-node@v4": ("setup", "setup-node node-version 22", ""),
        "Extension tests": (
            "test",
            "node --test tests/pi_package_bootstrap.mjs tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs tests/pi_delegation_extension.mjs tests/pi_interrupt_extension.mjs tests/pi_question_extension.mjs tests/pi_continuation_records.mjs tests/pi_observation_extension.mjs tests/pi_workflow_tracker.mjs",
            "9 .mjs files; tests/pi_publish_contract_installed.mjs is not invoked (ci.yml:74)",
        ),
        "Provision pinned Pi loader test host": (
            "provision",
            "npm install --prefix $RUNNER_TEMP/ovrcr-pi-package-host @earendil-works/pi-coding-agent@0.87.1; copy lockfile/host package.json",
            "",
        ),
        "Installed Pi package through production materialization": (
            "test",
            "cargo test -p ovrcr --test pi_package --features acceptance-diagnostics production_materializer_loads_local_package_with_one_injected_owner -- --ignored --exact --nocapture --test-threads=1",
            "1 test; test runtime 5.37s (log 4428); env OVRCR_TEST_PI_PACKAGE_ROOT",
        ),
        "Installed Pi effective child tools through production assignment argv": (
            "test",
            "cargo test -p ovrcr --test pi_delegation --features acceptance-diagnostics coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools -- --ignored --exact --nocapture --test-threads=1",
            "1 test; test runtime 7.25s (log 4481); env OVRCR_TEST_PI_PACKAGE_ROOT",
        ),
        "Upload installed Pi loader evidence": (
            "upload", "actions/upload-artifact@v4 (pi-package-<os>-<run>-<attempt>)",
            "uploads $RUNNER_TEMP/ovrcr-pi-package-evidence",
        ),
        "Post Run actions/setup-node@v4": ("other", "post-action cleanup", ""),
        "Post Run Swatinem/rust-cache@v2": ("other", "post-action cache save", ""),
        "Post Run actions/checkout@v4": ("other", "post-action cleanup", ""),
        "Complete job": ("other", "runner cleanup", ""),
    }
    step_rows = []
    for s in job["steps"]:
        started = s["startedAt"]
        completed = s["completedAt"]
        t0 = datetime.datetime.fromisoformat(started.replace("Z", "+00:00"))
        t1 = datetime.datetime.fromisoformat(completed.replace("Z", "+00:00"))
        cls, cmd, note = step_meta.get(s["name"], ("other", "", ""))
        if s["conclusion"] == "skipped":
            note = (note + "; " if note else "") + "step conclusion: skipped"
        step_rows.append(
            {
                "step_name": s["name"],
                "started": started,
                "completed": completed,
                "duration_seconds": int((t1 - t0).total_seconds()),
                "classification": cls,
                "command_summary": cmd,
                "notes": note,
            }
        )
    with (EVIDENCE / "job-steps.csv").open("w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(
            fh,
            fieldnames=["step_name", "started", "completed", "duration_seconds",
                        "classification", "command_summary", "notes"],
            quoting=csv.QUOTE_MINIMAL,
        )
        w.writeheader()
        w.writerows(step_rows)

    # ------------------------------------------------------------------
    # Canonical merged inventory (Rust + verified Node/Python fragments)
    # ------------------------------------------------------------------
    merge_gates, bad_sources = write_merged(inv_rows)

    # ------------------------------------------------------------------
    # Reconciliation gates
    # ------------------------------------------------------------------
    unique = len(attempts)
    binaries = len({b for b, _ in attempts})
    failed = sum(1 for r in rows if r["result"] == "FAIL")
    seconds = round(sum(float(r["seconds"]) for r in rows), 3)

    def retry(name):
        recs = [r for r in rows if r["test_name"] == name]
        return (len(recs), float(recs[0]["seconds"]),
                round(sum(float(r["seconds"]) for r in recs), 3),
                sum(1 for r in recs if r["result"] == "FAIL"))

    codex = retry("codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path")
    pi = retry("parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects")
    gates = {
        "attempt_records": {"observed": len(rows), "expected": 2822, "pass": len(rows) == 2822},
        "unique_pairs": {"observed": unique, "expected": 2820, "pass": unique == 2820},
        "binary_ids": {"observed": binaries, "expected": 56, "pass": binaries == 56},
        "failed_attempts": {"observed": failed, "expected": 2, "pass": failed == 2},
        "cumulative_seconds": {"observed": seconds, "expected": 5931.968,
                               "pass": abs(seconds - 5931.968) <= 0.001},
        "codex_retry": {"observed": codex, "expected": (2, 4.855, 9.227, 1),
                        "pass": codex[0] == 2 and abs(codex[1] - 4.855) < 1e-9
                                and abs(codex[2] - 9.227) <= 0.001 and codex[3] == 1},
        "pi_retry": {"observed": pi, "expected": (2, 24.507, 46.25, 1),
                     "pass": pi[0] == 2 and abs(pi[1] - 24.507) < 1e-9
                             and abs(pi[2] - 46.25) <= 0.001 and pi[3] == 1},
    }
    if not all(g["pass"] for g in gates.values()):
        raise SystemExit(f"gate failure: {gates}")

    # ------------------------------------------------------------------
    # Manifest
    # ------------------------------------------------------------------
    import shutil
    evidence_files = [
        "macos-job.log", "run.json", "attempts.csv", "inventory_rust.csv",
        "inventory_rust_test_level.csv", "inventory_node.csv", "inventory_python.csv",
        "inventory.csv", "job-steps.csv", "exclusions.csv", "parse_log.py",
        "build_inventory.py",
    ]
    file_hashes = []
    for name in evidence_files:
        p = EVIDENCE / name
        file_hashes.append(
            {
                "path": f"evidence/{name}",
                "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
                "bytes": p.stat().st_size,
            }
        )
    nextest_version = None
    m = re.search(r"cargo-nextest (\d+\.\d+\.\d+)", LOG.read_text(errors="replace"))
    if m:
        nextest_version = m.group(1)
    manifest = {
        "pinned_revision": REVISION,
        "revision_verified": {
            "git_rev_parse_HEAD": REVISION,
            "branch": "chore/unit-test-review",
        },
        "workflow_run": {
            "run_id": int(RUN),
            "job_id": int(JOB),
            "job_name": "macOS checks",
            "url": JOB_URL,
            "conclusion": "success",
            "head_sha": run_json["headSha"],
            "started_at": job["startedAt"],
            "completed_at": job["completedAt"],
        },
        "retrieval": {
            "retrieval_times_utc": {
                "evidence/macos-job.log": datetime.datetime.fromtimestamp(
                    (EVIDENCE / "macos-job.log").stat().st_mtime, datetime.timezone.utc
                ).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "evidence/run.json": datetime.datetime.fromtimestamp(
                    (EVIDENCE / "run.json").stat().st_mtime, datetime.timezone.utc
                ).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "source": "filesystem mtime converted to UTC; both files were already present and complete at task start",
            },
            "commands": {
                "log": f"gh run view {RUN} --job {JOB} --log > research/macos-test-review/evidence/macos-job.log",
                "run": f"gh run view {RUN} --json conclusion,headSha,jobs > research/macos-test-review/evidence/run.json",
            },
            "refetch_status": "not refetched: macos-job.log ends with job cleanup and lists all steps; run.json lists 23 steps and the job conclusion",
        },
        "files": file_hashes,
        "reconciliation_gates": gates,
        "merge_gates": merge_gates,
        "tooling": {
            "nextest_version_in_log": nextest_version,
            "cargo_nextest_on_path_locally": bool(shutil.which("cargo-nextest")),
            "identities_source": "the log's own PASS/TRY lines; no nextest installation or cargo cache change",
            "python": sys.version.split()[0],
        },
        "test_level_rule": (
            "Body-based, first match wins: acceptance = compiled product boundary "
            "(CARGO_BIN_EXE_, support Live harness, PTY, node/.mjs); integration = controlled external "
            "dependency (tempdir/fs/sqlite, subprocess, sockets, live:: helpers) or references to >=2 "
            "distinct top-level crate modules in the test body itself; unit = otherwise. "
            "Helper/impl bodies are traced one hop for process/PTY/socket/storage boundary "
            "markers only; component breadth is never taken from helper bodies."
        ),
        "notes": [
            "The two appended ignored parents use the canonical review identity (suite ovrcr::lib, "
            "variant production) with the verbatim runner names: "
            "'production_materializer_loads_local_package_with_one_injected_owner' (tests/pi_package.rs) "
            "and 'coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools' "
            "(tests/pi_delegation/coding.rs); both are the workflow --exact arguments.",
            "Skipped count 33 reconciles as 32 source #[ignore] tests + 1 test removed by the -E filter; "
            "4 further tests are #[cfg(not(target_os = \"macos\"))] and are not compiled on this runner.",
            "test_level is a bounded heuristic, not a call-graph proof: helpers are expanded one hop "
            "(unique lowercase fn names, capitalized fixture types with <=8 impl bodies, 400KB cap) and "
            "deeper indirection can still hide a boundary or a second component; unit rows therefore "
            "mean 'no boundary or composition visible from the test body plus one helper hop'.",
        ],
    }
    (EVIDENCE / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")

    # ------------------------------------------------------------------
    # Report
    # ------------------------------------------------------------------
    from collections import Counter
    suite_counts = Counter(r["suite"] for r in inv_rows)
    kind_counts = Counter(r["target_kind"] for r in inv_rows)
    level_counts = Counter(r["test_level"] for r in inv_rows)
    print(f"inventory rows: {len(inv_rows)} (2820 nextest + 2 cargo-test parents)")
    print("target kinds:", dict(kind_counts))
    print("levels:", dict(level_counts))
    print("suites:", len(suite_counts))
    print("\nper-suite counts (sorted):")
    for s, c in sorted(suite_counts.items()):
        print(f"  {c:4d}  {s}")
    print("\ntop 20 by total_attempt_seconds:")
    for r in sorted(inv_rows, key=lambda x: float(x["total_attempt_seconds"]), reverse=True)[:20]:
        print(f"  {r['total_attempt_seconds']:>9}s  {r['suite']}  {r['test_name']}  [{r['test_level']}]")
    print("\nignored tests:", len(ignored), " cfg(not(macos)) tests:", len(macos_off))
    print("gates all pass:", all(g["pass"] for g in gates.values()))
    print("\nmerge gates:")
    print("| gate | observed | expected | result |")
    print("| --- | --- | --- | --- |")
    for label, g in merge_gates.items():
        print(f"| {label} | {g['observed']} | {g['expected']} | {'PASS' if g['pass'] else 'FAIL'} |")
    if bad_sources:
        print("FINDING bad source ranges:", bad_sources[:10])


if __name__ == "__main__":
    main()
