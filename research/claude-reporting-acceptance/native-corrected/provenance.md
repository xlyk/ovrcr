# Corrected native run provenance

This is a retrospective provenance record created after the native fixture had
closed. There is no contemporaneous pre-build `git status` file, so the evidence
does not independently prove the checkout state at build time.

The coordinator's execution sequence records commit `7ec5634` before invoking
`just gui`. The current reflog places `7ec5634` at 2026-09-10 11:50:07 -0700.
The retained app executable and `build-7ec5634.log` both have an mtime of
11:51:36 -0700, and the split screenshot has an mtime of 11:53:51 -0700. The
current `git diff --name-only 7ec5634..HEAD` contains documentation, evidence,
plan, and CI files; it contains no later Rust product-source change.

These facts support the coordinator-recorded source attribution to `7ec5634`,
but they do not replace the missing contemporaneous status snapshot. The exact
command output and timestamps are retained in [raw-provenance.txt](raw-provenance.txt).
