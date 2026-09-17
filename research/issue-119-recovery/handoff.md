# Pi and Oh My Pi recovery: development handoff

Issue #119. Base: `65939b2feeea33185b6b94e4de129507a40180f0` (merged #116).
The delivered commit is the commit containing this handoff; pin its full SHA
with `git rev-parse HEAD` before acceptance. This is a development checkpoint,
not a review-ready feature or native acceptance. Independent spec review found
an unresolved contract conflict: concurrent deletion between validation and native
loading can trigger a native fresh-session fallback. Decide whether to require
an additional native attachment guard/upstream strict-open support or explicitly
exclude concurrent external history mutation before continuing this work.

## Independent native contracts

- Pi 0.85.1: installed package `@earendil-works/pi-coding-agent` documents
  `--session <path|id>` in README and exposes `getSessionId()` /
  `getSessionFile()` in SessionManager. The adapter always passes the absolute
  file, never an ID prefix or `--continue`. Its session-format documentation
  identifies the first JSONL record as `type: session` with the exact `id`.
- OMP 18.2.2: `omp --version` and `omp --help` independently identify
  `--resume <path|id>` (not Pi's flag). Source at
  [`60c9a115b2e8decc0f75825459362d14188a8bc0`](https://github.com/can1357/oh-my-pi/tree/60c9a115b2e8decc0f75825459362d14188a8bc0):
  `packages/coding-agent/src/main.ts:1097` passes a path to SessionManager.open;
  `session/session-manager.ts:2466` exposes the exact ID and file; its load path
  restores identity from the session header. `packages/utils/src/dirs.ts`
  defines agent-directory and profile/XDG resolution. This adapter refuses
  profile/XDG overrides rather than guessing their effective configuration.
- Both native loaders can create a new conversation for a missing explicit
  file. OVRCR therefore checks the exact file and bounded identity header before
  constructing resume argv. Concurrent external deletion/replacement after
  validation is not protected by a provider-side atomic attach API.

Native continuity on these versions must still be established separately.
Existing reporting evidence for OMP 18.1.19 is not evidence for 18.2.2 recovery.
No credentials, prompts, environment snapshots or transcript bodies are stored.

## Automated reproduction

Run in this checkout, with Node available on PATH:

```sh
SHELL=/bin/sh cargo test -p ovrcr --test retained_sessions
cargo test -p ovrcr-runtime extension_recovery
cargo test -p ovrcr-protocol wire_encoding_matches_snapshot
node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs
cargo check --workspace --all-targets --all-features
SHELL=/bin/sh cargo test --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

### Local results on the checkpoint contents

All commands above ran on macOS. The full suite was rerun after review corrections;
the earlier `/tmp/ovrcr-119-workspace.log` pass is not used for final verification.

| Gate | Result | Evidence |
| --- | --- | --- |
| Full workspace, all targets/features | exit 0; 961 passed, 19 ignored, 0 failed across 31 test binaries | `/tmp/ovrcr-119-workspace-final.log` |
| Pi + OMP Node extensions | exit 0; 43 passed, 0 skipped | `/tmp/ovrcr-119-node-final.log` |
| Workspace check | exit 0 | `/tmp/ovrcr-119-check-final.log` |
| Workspace clippy, warnings denied | exit 0 | `/tmp/ovrcr-119-clippy-final.log` |
| Formatting and diff whitespace | both exit 0 | terminal command output |
| Focused public-path recovery test | exit 0; 1 passed (both providers), 7 filtered | `/tmp/ovrcr-119-stale-run-2.log` |
| Recovery adapter units | exit 0; 2 passed | `/tmp/ovrcr-119-unit.log` (also in final workspace run) |
| Protocol snapshot | exit 0; 1 passed | `/tmp/ovrcr-119-wire-green.log` (also in final workspace run) |

The 19 ignored tests include opt-in native/load tests and controlled fixture
entry points; they are not counted as acceptance passes. Hosted Linux/macOS CI
and #129/#130 native acceptance were not run. Independent standards/spec reviews
used read-only Codex agents (`/tmp/ovrcr-119-{standards,spec}-review.md`). The
activity-suppression finding has a failing regression in
`/tmp/ovrcr-119-activity-red.log` and passes after the correction. The initial
stale-run fixture attempted to acquire an already-active lease and failed in
`/tmp/ovrcr-119-stale-run.log`; the corrected fixture first releases that lease,
then proves a valid old-run binding cannot commit after restart. The no-fallback
race remains a blocking spec finding, not a waived check.

`extension_conversations_switch_and_survive_repeated_restart` creates a private
config/socket/workspace for each provider. Its executable loads the real managed
extension into the existing Node host. It checks A-to-B-to-A changes, a retired
producer's late change, a real old-run supervisor's delayed retention request
against two replacement runs, two process restarts before any callback, exact native
arguments, stable row/title, no repeated prompt, missing resources, mismatched
history identity, activity delivery during a rejected SQLite retention write,
and a durable ephemeral replacement that cannot reopen A.
Shared server tests continue to exercise binding generations, retired leases,
run fencing, capacity and ownership. The existing lifecycle suites exercise
provider-specific readiness, input requests and pause/recovery.

The new transport test failed on missing `session_file` before the change.
The public restart test failed on absent `recovery.conversation` before the
adapter/receiver change. Temporary local check logs are `/tmp/ovrcr-119-*.log`.
All test resources use the existing Live fixture cleanup; no user's sessions
were stopped and no native GUI/provider acceptance was run.

## Separate native acceptance: #129 and #130

Each agent must pin the development SHA, build the actual GUI from that checkout,
and follow `docs/testing-computer-use.md`. Use a task-owned HOME, config, socket
and workspace. If setting `PI_CODING_AGENT_DIR`, use a separate private directory
for each provider: both providers honor that same variable. Never point it at
live provider data. Obtain authorization before credential copies or paid turns.

1. Record the actual provider version. Launch through the Dashboard's managed
   Pi or OMP entry, with no custom flags initially.
2. Establish a conversation with a recognizable, non-secret fact. Record its
   exact native ID and file using the provider's own session UI.
3. Switch A to B to A. Verify the public `terminal list --json` recovery identity
   follows each accepted switch; preserve the row and pinned title.
4. Stop only the owned fixture server/processes. Restart with the same private
   OVRCR config. Reopen the retained row, acknowledging old-process termination
   when required. Confirm launch and attachment are distinct states.
5. Restart immediately again, without another prompt. Confirm the same native
   history and identity are present. Then ask a continuity question.
6. Independently verify Busy, Ready quality (Pi Confirmed; OMP Observed), Unread,
   supported input requests and `/ovrcr-reattach` after recovery.
7. Make only fixture history/configuration/directory unavailable and verify a
   visible failure without a fresh conversation or most-recent fallback. Restore
   resources and Retry. Record screenshots, accessibility evidence and cleanup.

Profiles, extra configuration/extension flags and unlisted launch options are
unavailable for recovery. Pi and OMP reporting itself remains supported when
recovery configuration is unsupported. Native Linux continuity and platform GUI
prerequisites remain separate gates. Return failures with the pinned SHA and
reproduction; do not close #112 or treat one provider's pass as the other's.
