# Issue 59: desktop notification acceptance

Code revision: `422bc2d8a02042dfa5462920341944113d3eb7ee`.
Implementation base: `3574e9272e6464edfb484a973b33fd4c3f05384d`, containing
reviewed PR #56 head `86ef46b8eff3128566f76022c2cfef49714031ac` as an ancestor.
The base relationship and merged PR state were verified on 2026-09-11.

The implementation and native macOS acceptance are complete. Delivery requires
all checks on the final PR revision to pass. See [PR #69](https://github.com/xlyk/ovrcr/pull/69)
for that exact revision and its current checks; the earlier failed CI attempt
below remains part of this record.

## Acceptance matrix

| Requirement | Evidence |
| --- | --- |
| Reviewed hooks base | Exact ancestor recorded above; no runtime, protocol or provider changes |
| Off by default; existing settings and controls; identity only | Config/action tests; real disabled/re-enable test; native `N` toggle and identity-only macOS notifications |
| Accepted root deduplication; child, stale, duplicate, interruption and exit suppression | Real managed completion integration covers duplicate/stale/child/interruption events with later accepted completions as dispatch barriers; `desktop_ready_requires_new_accepted_root_identity_and_only_exports_identity` covers exited-session eligibility |
| Visible-pane suppression without acknowledgement | Geometry unit tests; real visible → hidden queue cancellation; native retained Ready with no alert |
| Attach/reconnect baseline and no disconnected delivery/replay | Pre-Hello event-order unit regression; real detach/reattach test; native enabled reconnect plus a fresh delivered completion |
| Bounded nonblocking host handling | Missing executable, error and blocked subprocess integrations; input/detach continue; owned host process group terminates |
| Full managed CLI/PTY/socket/dashboard route | Five deterministic host-recording integrations and actual macOS Notification Center observations |
| Synchronous ownership and 50 sessions | Runtime unchanged; local full regression passes; hosted Linux capacity and memory jobs pass at code revision |
| Isolation and cleanup | Native configuration/socket/child owned by fixture; launcher exit 0; directory/socket and all 17 PIDs / 15 process groups absent |
| Exact revisions, docs, diary, worker and independent review | This record, support/setup docs, Personal work diary and independent review below; coordinator owns staging and current-head CI |
| No merge, release or live trust/config changes | PR delivery only; none performed |

## Automated results

| Gate | Actual result at `422bc2d8` |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo test --workspace --all-targets --all-features --no-fail-fast` | 687 passed, 0 failed, 14 intentional ignores, 25 test binaries; 135.793 seconds |
| `cargo test --workspace --doc` | Command passed; zero doctests across five crates |
| Notification integration coverage within full suite | All five tests passed |
| Hosted Linux suite | 661 passed, 0 failed, 14 intentional ignores; formatting, Clippy and zero-doctest command passed |
| Hosted Linux capacity | One isolated 50-session acceptance test executed and passed |
| Hosted Linux memory high-water | One isolated acceptance test executed and passed |
| Hosted macOS attempt | Failed in unchanged runtime fixture; see retained failure below |

Coordinator logs and metadata are [format](automated/final-fmt-02.json),
[Clippy](automated/final-clippy-02.json), [workspace](automated/final-workspace-02.json)
and [doctests](automated/final-doctests-02.json), each with matching `.log`.
`--no-fail-fast` runs all binaries without relaxing assertions or concurrency.
Hosted [metadata](automated/hosted-34658269165.json) and
[complete log](automated/hosted-34658269165.log) retain the exact code revision.

## Native macOS results and platform limits

The [native report](native/review.md) records actual macOS desktop delivery before
and after reconnect, input and toggle outcomes, duplicates, visible-pane and
disabled suppression, no replay, screenshots, accessibility excerpts and cleanup.
Only terminal/project/workspace identity appeared in the two observed alerts.

The [deterministic native child](native/codex-fixture.py) uses the real managed
launcher, PTY, reporter, socket and dashboard built from the reviewed checkout.
It does not recertify the installed Codex CLI. PR #56 covers the unchanged actual
Codex 0.153.0 hook semantics. No credentials or native trust settings were changed.

Native desktop delivery is verified on macOS only. Linux has automated adapter
and application-path coverage; visible Linux desktop delivery remains unverified.
A successful host command does not prove display or permission: macOS may silently
suppress `osascript` notifications. OVRCR uses bounded best-effort submission and
does not change OS notification policy. Negative native observations complement,
but do not replace, deterministic dispatch counts and host-failure tests.

Feature GUI screenshots and AX are in `native/`. Full Notification Center captures
contain unrelated user notifications and remain in a private local evidence
directory; only task-specific excerpts are committed. Hashes, process ownership,
and completed cleanup are in [the cleanup record](native/22-cleanup.json).

## Independent review

A scoped worker implemented the change; an independent read-only reviewer traced
the committed code, callers and assertions. The coordinator alone staged and
committed. Two review findings were fixed:

- `1475f0ade7854773ab78db82418caa085fd57895` suppresses historical `SessionChanged`
  events that can precede the first hierarchy. This revision also gates Connected
  health and uses one bounded pending queue with one cancellable host operation
  and the existing dashboard wake mechanism.
- `422bc2d8a02042dfa5462920341944113d3eb7ee` makes queued cancellation durable:
  visible → hidden and unavailable → connected changes cannot revive an alert
  while another host operation is blocked. Unit and real integration assertions
  reproduced the defect before the correction.

Final independent code/assertion review at `422bc2d8` found no blocking issues;
both findings were resolved. A separate final documentation/evidence review checked
counts, links, platform claims and privacy; its coverage-attribution correction is
included. Native and automated acceptance are separate evidence.

## Retained failures and limits

[Development attempts](automated/development-attempts.json) classify every retained
log by its result, not its filename. The initial managed-path RED observed zero
host calls instead of one. A later failed GREEN exposed missing `SessionChanged`
ingestion. Health, pre-Hello and durable queue regressions also failed before
correction. Compile errors are not behavioral RED. The first host-wake probe
passed a weak descriptor-HUP assertion; no RED claim is made for it. The final
assertion requires a real wake byte.

The [first full local attempt](automated/final-workspace-01.json), at `1475f0a`,
exited 101 after 102.746 seconds. Its lifecycle binary passed 78 tests, failed two
and intentionally ignored nine: `codex_managed_lost_bind_receipt_recovers_before_busy`
reported `version probe unsupported or unavailable`, then
`split_server_two_streams_resize_resync_and_detach` encountered the poisoned shared
lock. All five notification integrations passed. Later binaries and doctests did
not run. The successful `422bc2d8` run does not establish a cause for this failure.

[Hosted run 34658269165](https://github.com/xlyk/ovrcr/actions/runs/34658269165)
at `422bc2d8` passed Linux, capacity and memory but failed macOS: 611 passed,
one failed and 14 intentionally ignored before Cargo stopped. In unchanged
`server::tests::partial_resize_connection_delivers_error_before_owner_close`,
`set_read_timeout(Some(Duration::from_secs(3)))` at runtime `server/tests.rs:1881`
returned `Invalid argument` before the error-frame assertion. A separate native
[reproduction](automated/socket-timeout-race.rs) and [log](automated/socket-timeout-race.log)
confirm macOS returns EINVAL if the peer has already closed, while the buffered
response and EOF remain readable. A fixture timing race is the diagnosis; no
notification or runtime code was changed to bypass it. The [failed log](automated/hosted-34658269165-failed.log)
remains retained. Later current-head CI is recorded in the PR, not inferred here.

Earlier CUA preflight calls stalled despite requested timeouts (295 seconds for
Notification Center, then 3,278 seconds for Finder). The later native fixture run
completed all acceptance steps; its separate preflight observation records a
178-second Notification Center selection. Tool delays are not desktop proof.
