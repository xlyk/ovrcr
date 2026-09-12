# Issue 60: ready sound acceptance

Code revisions: feature `7861a97844f6760514522b191550bae836bd5179`, which-key
listing `6169e3bc4b7fea2b55a23e59bfb96269b6316d27`, review fixes `5b8637e`
(final code head; see [Review](#review)). Implementation base:
`12f6b9c21d6311e3f29eb27b9882d396b7fdc773` (`origin/main`), which contains the
squash-merged PR #69 desktop-alert commit `7d638c0` as an ancestor (the PR's
reviewed branch head `e29da18` was squashed, so it is not itself an ancestor).
Verified on 2026-09-11.

The change adds `ready_sound` to `dashboard.toml` and an uppercase `S`
browse-mode toggle, both independent of `desktop_notifications`. The existing
accepted-root observer, visibility suppression, attach/reconnect baseline,
bounded pending queue and single cancellable host worker are reused unchanged;
the worker now runs the enabled channels in order (notification, then sound)
per accepted response, reading the channel mask when each host command starts.
macOS plays `/System/Library/Sounds/Glass.aiff` with `afplay`; Linux plays the
freedesktop `complete.oga` with `paplay`. Playback has a five-second deadline
because measured `afplay` wall time is about 2.5 seconds. A failing or missing
player shows **Ready sound unavailable** and changes nothing else.

## Acceptance matrix

| Requirement | Evidence |
| --- | --- |
| Off by default; independent of desktop notifications through existing configuration and controls | `ready_sound_default_off_and_independent_of_desktop_notifications` (settings); `ready_sound_action_is_opt_in_and_discoverable` (hints, palette source); `question_mark_popup_lists_ready_sound_beside_desktop_notifications` (which-key); native `S`, `N`, config-file enable and popup dispatch |
| Reuse accepted selection, visibility suppression, reconnect/deduplication; no second watcher | One observer and one queue in `desktop.rs`; `sound_only_desktop_only_both_and_neither_share_one_accepted_candidate`; real-path `ready_sound_is_independent_of_desktop_notifications_on_the_managed_path` covers duplicate, child, interruption, visible-pane, disabled and reconnect cases; native duplicate Stop, visible pane and disconnected completion |
| Sound failure nonblocking; bounded supported host mechanism | Same worker, process group, deadline and cancellation as notifications; `sound_host_failure_reports_its_own_notice`; real-path failing player leaves Ready, input and detach intact; `afplay`/`paplay` are shipped system players |
| Sound-only, desktop-only, both, neither; one alert per accepted response; no replay/child/interruption | Unit matrix above; real-path test counts player and notification calls per phase; native table in [native/review.md](native/review.md) with process observations per turn |
| Real host audio acceptance per claimed platform; document unavailable delivery | macOS: real `afplay` spawned by the dashboard three times, running for the file duration (native report). Audibility itself is not machine-verified; Linux native playback is not claimed. Both limits are in `docs/dashboard.md` |
| Synchronous runtime, one server owner, one active dashboard, 50 sessions | No runtime, protocol or provider change; full workspace suite below |
| Isolated configuration, sockets, workspaces; preserve unrelated work; clean up only owned resources | Integration fixtures use their own config, socket and PATH; native runs used the disposable GUI demo with `OVRCR_DASHBOARD_CONFIG` unset; cleanup records in `native/` |
| Exact revisions and separate automated, native, platform results; docs and diary | This record, [native report](native/review.md), `docs/dashboard.md`, `README.md`, Codex setup and support docs, Personal work diary entry |
| No merge, release or live trust/configuration change | Branch commits only; none performed |

## Automated results

Every log has a matching `.json` with the command, exit code and duration.
Development logs are kept under their attempt names; classification is by
content in [development-attempts.json](automated/development-attempts.json).

| Gate | Revision | Result |
| --- | --- | --- |
| `cargo test -p ovrcr-tui --lib ready_sound` before implementation | working tree before `7861a97` | Compile failure on the new symbols ([log](automated/unit-red-01.log)); a compiler RED, not a behavioral one |
| Focused unit tests (desktop, settings, hints) | `7861a97` working tree | 27 passed ([log](automated/unit-green-01.log)) |
| Real managed-path sound integration | `7861a97` working tree | 1 passed ([log](automated/integration-green-01.log)); written after the implementation, so no RED exists for it |
| Existing desktop integrations with the shared fixture change | `7861a97` working tree | 6 passed ([log](automated/integration-desktop-green-01.log)) |
| Which-key popup listing before the fix | `7861a97` + test | Behavioral RED at `tests/tui/palette.rs:82` ([log](automated/whichkey-red-01.log)) |
| Which-key popup listing after the fix | `6169e3b` | 2 passed ([log](automated/whichkey-green-01.log)) |
| Sound and desktop integrations after the review fixes, including the hung player | `5b8637e` working tree | 6 passed ([log](automated/integration-green-02.log)) |
| `cargo fmt --all -- --check` | `5b8637e` | Passed ([json](automated/final-fmt-02.json)); also passed at `6169e3b` ([json](automated/final-fmt-01.json)) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | `5b8637e` | Passed ([json](automated/final-clippy-02.json)); also passed at `6169e3b` ([json](automated/final-clippy-01.json)) |
| `cargo test --workspace --all-targets --all-features --no-fail-fast` | `5b8637e` | 698 passed, 0 failed, 14 intentional ignores across 25 test binaries; 137.2 seconds ([json](automated/final-workspace-02.json), [log](automated/final-workspace-02.log)); earlier at `6169e3b`: 698 passed, 0 failed, 14 ignores ([json](automated/final-workspace-01.json)) |
| `cargo test --workspace --doc` | `5b8637e` | Passed; zero doctests across five crates ([json](automated/final-doctests-02.json)); also at `6169e3b` ([json](automated/final-doctests-01.json)) |

Hosted CI on the current head is required for delivery and is recorded in the
pull request, not inferred here.

## Native macOS results and platform limits

The [native report](native/review.md) records the real GUI helper built from
`7861a97`: the `S` and `N` toggles, one real `afplay` per accepted background
response in sound-only, both and configuration-enabled modes, `osascript` only
in desktop-only mode, nothing for neither, duplicate Stop, visible pane or a
completion while disconnected, and no replay after reconnect. Identity-only
notification arguments and the fixed player argument were captured from the
process table. The which-key popup at `6169e3b` lists and dispatches `S`.
Cleanup of both disposable fixtures is recorded.

Audibility is not machine-verified: the evidence is the real player process
running through the default output device for the file's duration at the
user's 6 % output volume. Native Linux playback is unverified; Linux has the
automated adapter and application-path coverage only. A successful player
command does not prove a sound was heard, and OVRCR does not change audio
settings.

## Review

Two independent read-only reviewers examined the committed diff
`origin/main...6169e3b` in parallel, one against the repository's documented
standards plus a code-smell baseline, one against the issue text. The
coordinator alone staged and committed.

Standards axis, four findings acted on and three accepted as-is:

- Hard: docs linked to an uncommitted evidence directory. Resolved by the
  evidence commit that includes this record.
- Hard: `docs/dashboard.md` still said disabling notifications cancels pending
  work. Now says only disabling the last enabled channel does.
- Judgement: `DesktopHost.enabled` duplicated `channels != 0`. The flag is
  removed; enablement derives from the mask.
- Smells: duplicated host literals in unit tests and duplicated wait loops in
  the lifecycle fixture were factored (`stub_host`, `wait_record`); the failure
  mask's second enumeration was removed by sending failures inline. The bare
  `u8` channel mask and the which-key key list stay as pre-existing design.

Spec axis, three findings acted on and two recorded:

- Missing: no test for a hung player. The real-path test now blocks the player
  and proves input and detach continue; detach ends the player's process group.
- Questionable: a worker-start failure in sound-only mode said "Desktop
  notifications unavailable". The notice now names the enabled channel.
- Questionable: two failures drained in one pass showed only the last. They
  now produce one combined notice.
- Recorded: turning one channel off does not cancel that channel's in-flight
  command (documented trade-off); a failure for a channel disabled after its
  command started is no longer reported.
- No scope creep found.

The fixes are commit `5b8637e`; every gate was rerun on that head.

## Retained failures and limits

- The unit RED is a compile failure and the integration test has no RED; the
  which-key RED is behavioral. All three logs are retained.
- Native evidence was gathered at `7861a97` and `6169e3b`. The review-fix
  commit `5b8637e` changed failure reporting, internal host state and test
  fixtures, not the observer, queue, channel order or host commands; it is
  covered by the rerun automated gates below, not by a third native run.
- Terminal-mode `S`, the failing-player notice, the hung-player deadline kill
  and the combined-failure notice are covered by unit and real-path integration
  tests only, not natively.
- Channels run one after another inside the single host operation, so a
  response with both channels holds the queue for the notification deadline
  plus playback (about 2.5 seconds). The queue stays bounded at 50; concurrent
  channel dispatch is the noted upgrade if that backlog matters.
