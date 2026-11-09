#!/usr/bin/env python3
"""Fail when a test target the workspace declares never appears in the selection.

The sharded `macOS checks` job hashes a list of built test binaries. If a target
is not built, the list is silently short: the per-shard counts still add up,
every job stays green, and that target's tests never run. Nothing in the shard
logs distinguishes that from a target a branch or a merge legitimately dropped.

Expectations come from two independent sources on purpose:

* `cargo metadata` — every test-capable target the manifest machinery knows.
* the test sources themselves — `tests/*.rs` beside each package manifest, which
  is what cargo auto-discovers when no `[[test]]` section overrides it.

The second source exists because the first is not a reliable floor: on a Linux
runner `cargo metadata` once reported one fewer target than the macOS shards
were expected to build. A target is required if either source knows about it.

Why the check exists: `ovrcr::operator_auth` appeared in some runs and not
others at unchanged source, which looked like an intermittent build failure.
It turned out that `main` had deleted the test and the pull-request merge ref
applied the deletion, while a stale local checkout still had the file. Both
readings produce a smaller selection and a green run. This tool plus the
recorded manifests are what tell them apart.

Usage: python3 scripts/check-test-selection.py <manifest-dir> [...]
Every *.json in the directory is treated as one shard's `cargo nextest list
--message-format json` output.
"""

import glob
import json
import os
import subprocess
import sys

LIB_KINDS = {"lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"}


def cargo_metadata():
    return json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    )


def metadata_suites(meta):
    """Test-capable targets the manifest machinery reports, in suite-id spelling."""
    suites = {}
    for package in meta["packages"]:
        for target in package["targets"]:
            if not target.get("test"):
                continue
            kinds = set(target["kind"])
            name = target["name"]
            if kinds & LIB_KINDS:
                suite = package["name"]
            elif "test" in kinds:
                suite = f"{package['name']}::{name}"
            elif "bin" in kinds:
                suite = f"{package['name']}::bin/{name}"
            elif "example" in kinds:
                suite = f"{package['name']}::example/{name}"
            elif "bench" in kinds:
                suite = f"{package['name']}::bench/{name}"
            else:
                continue
            suites[suite] = f"{package['name']} ({'/'.join(sorted(kinds))}:{name})"
    return suites


def source_suites(meta):
    """Integration-test targets visible on disk for each workspace package."""
    suites = {}
    for package in meta["packages"]:
        package_dir = os.path.dirname(package["manifest_path"])
        tests_dir = os.path.join(package_dir, "tests")
        if not os.path.isdir(tests_dir):
            continue
        # Only top-level tests/*.rs are targets; tests/support/* is a module tree.
        for path in sorted(glob.glob(os.path.join(tests_dir, "*.rs"))):
            name = os.path.basename(path)[: -len(".rs")]
            suites[f"{package['name']}::{name}"] = os.path.relpath(path)
    return suites


def load_manifests(paths):
    shards = {}
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            document = json.load(handle)
        shards[os.path.basename(os.path.dirname(path)) or path] = set(
            document.get("rust-suites", {})
        )
    return shards


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)

    manifests = sorted(
        path
        for argument in argv[1:]
        for path in (
            glob.glob(os.path.join(argument, "**", "*.json"), recursive=True)
            if os.path.isdir(argument)
            else [argument]
        )
    )
    if not manifests:
        sys.exit(f"no selection manifests found in {argv[1:]}")

    meta = cargo_metadata()
    from_metadata = metadata_suites(meta)
    from_sources = source_suites(meta)
    expected = dict(from_sources)
    expected.update(from_metadata)

    script_only = sorted(set(from_sources) - set(from_metadata))
    print(
        f"declared targets: {len(expected)} "
        f"({len(from_metadata)} from cargo metadata, {len(from_sources)} from test sources)"
    )
    if script_only:
        print(
            "note: cargo metadata did not report these test sources, requiring them "
            f"from disk anyway: {script_only}"
        )

    shards = load_manifests(manifests)

    failed = False
    for shard, suites in sorted(shards.items()):
        missing = sorted(set(expected) - suites)
        if missing:
            failed = True
            print(f"FAIL {shard}: {len(suites)} suites, missing {len(missing)} declared target(s)")
            for suite in missing:
                print(f"     missing {suite}  <- {expected[suite]}")
        else:
            print(f"ok   {shard}: {len(suites)} suites, every declared target present")

    if len(set(map(frozenset, shards.values()))) > 1:
        failed = True
        union = set().union(*shards.values())
        print(f"FAIL shards disagree: union {len(union)} suites")
        for shard, suites in sorted(shards.items()):
            print(f"     {shard} lacks {sorted(union - suites)}")

    if failed:
        sys.exit(
            "\ntest selection is incomplete: the partition would silently drop "
            "these tests instead of running them"
        )
    print(f"\nall {len(shards)} shard(s) built every declared target ({len(expected)} suites)")


if __name__ == "__main__":
    main(sys.argv)
