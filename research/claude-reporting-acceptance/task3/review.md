# Task3 mechanics independent review

Initial review: `32336e722f6c92843313301e91bb99ef6e6ad2b1` against `8108941`. One blocking finding: reporting-only private-channel creation failure aborted native execution. A bounded reviewer CLI probe hit sandbox socket denial, confirming the caller path; this was not described as an authorized PTY application failure.

Correction: `590b5d27a77a54a7392288cda735dcee20e2f977`. Targeted independent rereview found the P1 resolved and no introduced blocking defects. Channel failure invokes the lease-release callback before native spawn; release has a one-second deadline followed by watched-socket closure. The fallback strips all five inherited reporting variables. Real entry-point regressions prove native output/exit, one diagnostic, and another invocation reserving while the fallback remains running. See p1-channel-failopen.md and attempts18–22.

Task3 partial mechanics is ready. The reviewer changed no files and reran no tests during the correction review. Provider admission is still disabled; collector/final metrics, Linux, native GUI and full workspace acceptance remain open. This does not close all Task3 checkboxes.
