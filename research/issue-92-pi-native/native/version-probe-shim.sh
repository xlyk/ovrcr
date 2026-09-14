#!/bin/sh
# Native-gate shim for OVRCR #92 (see research/issue-92-pi-native/README.md, Defect 1).
# OVRCR bounds its `<exe> --version` admission probe at 1 second
# (src/report/admission.rs probe_command). Pi 0.85.1 on this host answers --version in
# 6-11 s because V8 must evaluate a ~153 MB ESM bundle, so managed reporting never engages.
# This shim answers ONLY the bare `--version` probe from the value the real binary printed,
# and exec's the real Pi for every other invocation, so the agent process, its PTY, its
# argv and its extension loading are the genuine Pi 0.85.1.
if [ "$#" -eq 1 ] && [ "$1" = "--version" ]; then
  echo 0.85.1
  exit 0
fi
exec /Users/xlyk/.local/bin/pi "$@"
