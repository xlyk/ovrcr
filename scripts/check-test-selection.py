#!/usr/bin/env python3
"""Fail when a test target the workspace declares never appears in the selection.

The sharded `macOS checks` job hashes a list of built test binaries. If a target
fails to build, the list is silently short: the shard counts still add up, every
job is green, and the missing target's tests simply never run. That happened
with `ovrcr::operator_auth`, which vanished for two runs and came back for two
more at identical source.

This compares the suites nextest actually built against the test-capable targets
`cargo metadata` says exist, so a dropped target fails the build instead of
quietly shrinking coverage.

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


def declared_suites():
    """Test-capable targets in this workspace, in nextest's suite-id spelling."""
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    )
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

    expected = declared_suites()
    shards = load_manifests(manifests)

    failed = False
    union = set().union(*shards.values())
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
