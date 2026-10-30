# Operator layout and quick-message acceptance, 2026-10-30

This is an acceptance candidate. Focused Operator/native checks pass, while full
workspace, TUI Bridge and terminal-startup gates retain unresolved failures.
Feature acceptance and hosted-platform delivery remain open.

The tests exercise the production Dashboard, Server framing, compiled worker,
normal synthetic OAuth storage, Bash children, real PTYs and retained journal.
The native runs use the reviewed checkout's frozen GUI/CLI bundle. All accounts,
messages, repositories, HOME/config/socket paths and model endpoints are disposable
fixtures. No personal account, paid inference or installed application was used.

## Revisions and implementation

- Product baseline: `dc5e8cb01f84dd2cb68089af2497830078e46b41`.
- Acceptance tests: `299c2890` (`test(operator): verify layout and quick message flows`).
- Native findings produced the independently reviewed product correction
  `479ceb2715259cb24dc7bd8368518383b81c694e`: explicit CRUST behind the quick
  header control and BLUE for the Operator section heading. Task 5 reported the
  pixel/AX mismatch before that separate product edit.
- Product executable source: `abd4793993c1b3d38951309c5e93ece078b0a42e`.
  This adds B's actual Bash completion after A closes.
- Final acceptance-test source: `dc9b49d` adds navigation to a second real terminal
  while Create is held, with an independently expected complete captured context.
  Product source and frozen native bytes are unchanged from abd4793.
- The final evidence/docs commit changes no executable source. Hosted checks
  must qualify that final PR revision independently.

`tests/operator_e2e.rs` reuses `DashboardClient`, `Live` and `Provider`.
The fixture's `hold` method writes the actual request and retains its real Server
receipt before Dashboard delivery. It continues to dispatch other Server events.
The ordering assertions use these receipt boundaries rather than sleep timing.
`tests/support/operator_provider.rs` adds a tool-free, gated one-inference model
and two controlled Bash scenarios. Existing A/B, timeout and failure scenarios
keep their original assertions and deadlines. `tests/terminal_acceptance.rs`
reuses `OuterDashboard`; no alternate product constructor or authentication bypass
was added. Existing event-loop coverage remains intact.

## Requirement and assertion map

| Requirement | Executed assertion or evidence |
| --- | --- |
| One captured first message | `operator_quick_message_records_one_captured_first_message`: exactly one durable User entry, exact multiline Unicode text and complete frozen context; independently expected project, workspace label/identity, cwd and terminal/session/run; provider quick request count 1 and total count 3 including B's two tool requests |
| Late Create/Submit, dismissal, concurrent completion | Same test holds each real mutation receipt, dismisses the popup, navigates to a second live terminal, completes B while Create is held, delivers Create, holds Submit through worker completion, and asserts terminal selection remains unchanged |
| Pending Enter, paste and failure | Same test rejects repeated Enter while pending, checks paste produces no request/PTY bytes, delivers a real invalid-model Create rejection after dismissal, and checks no extra inference. Task 4's existing failure/retry and exact-authority unit tests remain in the full TUI suite |
| Close only A | `operator_close_archive_restores_history_without_replay`: both Bash child PIDs are recorded alive in their distinct owned worker groups; after Close, A's controlled-stop proof, group absence and child ESRCH are asserted while B's child/group remain alive |
| B and terminal continue | Same test receives a separate `CLOSE_TERMINAL_OK` output line, releases B's real Bash gate, asserts durable successful `CLOSE_BASH_OK` output and Complete, then proves B's group/child absence |
| Restore without replay | Same test compares A's preserved history across Server restart, calls normal `UnarchiveSession`, asserts no additional inference or Bash execution, explicitly submits one new turn on the same session with an advanced retained run, and checks exactly one new user message/request |
| Generic lifecycle refusal | `final_review_public_terminal_lifecycle_refuses_operator_then_native_stop_allows_next_turn` remains unchanged and executes in the compiled-worker suite |
| Real PTY input | `operator_quick_outer_pty_modifiers_paste_escape_restore_terminal`: literal Ctrl+O, CSI-u Shift+Enter, bracketed multiline paste; first/second/third on distinct visible editor lines; Escape restores Terminal mode; separate target-child acknowledgement; zero durable conversations and no quick text in terminal |
| Production dispatch | Existing `operator_quick_production_dispatch_keeps_key_paste_mouse_resize_off_pty` and `operator_real_socket_reader_keeps_streams_and_terminal_input_independent` run with the full TUI suite |
| Sidebar/status/hover/collapse | Native run 1 captures `native-02`, `native-05`, `native-06`, `native-07`, `native-12`; current run captures `native2-01`, `native2-06` |
| Native close/cancel/restore | `native-08`–`native-13`; current `native2-05`–`native2-08`, plus kernel group and durable-journal receipts |
| Native Ctrl+O and multiline creation | `native-03`, `native-09`, `native-durable-journal.json`; current `native2-02`, `native2-03`, `native2-05`, `native2-durable-journal.json`; both holders finish with quick_requests=1 |
| Narrow floors | Current `native2-02` is 80×24; `native2-03` is native 60×24 using standard Cmd+plus zoom; `native2-04` uses the documented isolated nested PTY at 60×16, with `native2-nested.kernel-size` recording `16 60` |
| Truthful Unread | Native run 1 holds B final while A is visible, shows teal unread checks for B and quick, then presents B's durable tail and captures its check cleared while quick remains unread (`native-06`, `native-10`–`native-12`). The real journal pagination E2E preserves positive/intermediate/negative assertions using the selected checkmark glyph |

## Native source and executable identity

[Current wide Original layout](native2-01-wide-original.jpg) has paired
[full accessibility text](native2-01-wide-original.ax.txt). It shows B's completed
real Bash/tool turn beside A's independently running turn. Every JPEG in this
folder has matching `.ax.txt` from the same capture; bytes have JPEG signatures.
Screenshots were inspected as pixels, including header contrast, one-line rows,
selection, Unicode spacing, clipping and cursor position.

`native-state.json` records the 299c289 bundle hashes; `native2-state.json` records
the abd4793 bundle hashes and the frozen holder hash. GUI and CLI were built with
`gui,acceptance-diagnostics`, assembled with the published `scripts/gui.sh` bundle
steps, and launched directly from `Contents/MacOS` with a sanitized environment.
Ordinary `just gui` would rebuild without diagnostics, so it was not represented
as loopback-provider proof. Original build commands/exits are in `native*-build.json`.
The native launcher and holder commands/PIDs and their original wait results are
in `native*-owner.json` and `native*-exit.json`.

Both native holders finish with A=2, B=2 and quick=1 requests. A/B each include a
real Bash call and its continuation; they are not labeled one-inference turns.
Normal fixture-owned OAuth used the production AuthManager and protected storage.
Only a literal loopback inference endpoint was available. The second run copied
the holder executable before starting broad Cargo gates, keeping all active
native executable bytes frozen while the build directory changed.

## Attempts and limits

- `close-e2e-attempt1` reached restored submission but observed one inference
  where two were expected. `close-e2e-attempt2` did not obtain a Bash child PID.
  Serial attempt 3 and later strengthened completion coverage passed. Concurrent
  feature builds occurred during the initial attempts; no causal claim is made
  without the missing failed-turn/executable evidence.
- `quick-pty-attempt1` used the Agent picker for a Terminal-kind fixture, which
  correctly found no agent. Attempt 2 reached the input outcome but used an
  ordinary connection for an Operator list, which the Server correctly refused.
  The corrected case uses the existing Agent fixture and a read-only count of the
  private retained store. Attempt 4 strengthens separate-line assertions.
- `compiled-worker-attempt1` found an obsolete solid-dot Unread assertion.
  The selected layout intentionally uses a teal checkmark. All three positive,
  intermediate-page and negative-tail assertions changed together; attempt 2
  passed with the existing pagination and lifecycle assertions intact.
- `native-04-focus-attempt` did not establish delayed native receipts: successful
  Create/Submit arrived before Escape, selected the new chat, and later typing
  opened an owned form. The form was cancelled. Native late-receipt focus remains
  unverified; the deterministic real-socket Dashboard test proves that ordering.
- The first nested shell command exposed RTK's splitting of an executable path
  containing spaces. The corrected published route changes into the bundle's
  `Contents/MacOS` and runs `rtk proxy ./ovrcr`.
- `native-15` is an unqualified nested quick attempt: Ctrl+O was correctly
  intercepted by the outer Dashboard. `native-16` and current `native2-04` click
  the inner title-bar control through forwarded native mouse input and show the
  inner global-context quick editor. The current nested run additionally records
  its original Dashboard and shutdown exits as 0.
- Native run 1 exposed invisible title-bar text and a mismatched section color.
  Its images are retained. The independently reviewed correction is visible in
  the current run. Source/behavior evidence from 299c289 is distinguished from
  current abd4793 captures; the interleaved product diff changed color tokens only.
- The first required full workspace gate failed on the startup-copy
  case `captured_build_survives_atomic_executable_replacement_and_decline`: the fixture did not establish a successful UnixStream connection within its budget,
  and its original Live shutdown timed out. A socket path existed by cleanup;
  path absence is not claimed. A focused rerun passed 1/1. The first failure remains a failed gate;
  `workspace-failure-cleanup.json` preserves that unqualified wait and the later
  kernel/process-root absence before removal of its owned root.

## Command results

`command-results.json` contains each exact command, original exit, revision,
private environment keys/root and executed Rust suite counts. Each command has
its own full `.log` and `.json`; failures are retained beside later attempts.
Node 22.23.2 and Rust 1.98.0 meet the project prerequisites.

| Gate | Revision | Actual result |
| --- | --- | --- |
| fmt, Clippy `-D warnings`, check, diff check | dc9b49d | all exit 0; original command logs/metadata have `-final` names |
| full workspace, all targets/features, attempt 1 | abd4793 | exit 101; 366 passed, 1 failed, 6 ignored before Cargo stopped at `client_startup_server`; later suites unrun |
| full workspace, all targets/features, attempt 2 | abd4793 | exit 101; same executed counts, different startup-copy test failed; no full-gate pass |
| first startup failure focused rerun | abd4793 | exit 0; 1 passed, 21 filtered; does not qualify the failed full gate |
| full affected protocol/runtime/TUI crates | abd4793 | exit 101; protocol 109 passed; runtime 488 passed/1 ignored; TUI 328 passed/2 failed |
| full TUI crate, serial rerun | abd4793 | exit 101; 328 passed/2 failed; original assertions/deadlines unchanged |
| full root TUI integration | abd4793 | exit 0; 329 passed |
| full terminal acceptance | abd4793 | exit 101; 11 passed/7 failed/1 ignored; Cargo did not reach root TUI in that invocation |
| focused real-PTY `operator_quick` | abd4793 | exit 0; 1 passed, 18 filtered |
| Operator auth/E2E/scope/worker suites | abd4793 | exit 0; 39 passed/3 ignored helpers |
| final compiled-worker suite | dc9b49d | exit 0; 12 passed/2 ignored helpers/1 filtered CLI case |
| independently frozen context after navigation | pre-commit dc9b49d change | exit 0; 1 passed/14 filtered |
| unchanged Node gate, Node 22.23.2 | abd4793 | exit 0; 106 passed, none skipped |
| unchanged offline Cursor Python gate | abd4793 | exit 0; 67 passed |

Both full workspace commands ran with `require_escalated`. `run-check.py` records
an environment containing only inherited PATH, fresh private HOME/TMPDIR,
CARGO_HOME/RUSTUP_HOME for the existing compiler cache, offline Cargo mode,
`SHELL=/bin/zsh`, loopback proxy exclusions and a synthetic private Git identity.
Every other inherited variable was absent. The copied Server fixture discards
child stdout/stderr in unchanged `Live::start_binary`, so neither failed copied
child has retained stderr or a qualified successful wait. The exact failures
remain unexplained; no timing/contention or permission cause is asserted.

Attempt 2 failed `accepted_restart_runs_changed_binary_with_same_protocol_and_package_version`.
Both startup failures mean no successful socket connection within the budget;
both later cleanup receipts record a stale socket path. The TUI failures exercise
direct fake Bridge clients, with statuses Failed versus expected Available/Denied,
a worker completion deadline, or a client that did not reach its gate. They do not
exercise the Operator loop. The seven terminal failures occurred before visible
Dashboard readiness, including the new quick case. The new initial Operator List
flush before TerminalGuard is not causally excluded. These are unresolved failed
local gates; focused passes do not erase them.


## Quick-only native completion

Native run 3 reuses the byte-identical abd4793 bundle after all process-heavy
checks ended. `native3-state.json` separates product and acceptance revisions.
It closes the earlier missing quick PID/PGID capture: session 13, PID/PGID 29583
was recorded alive while the tool-free provider waited; `native3-quick-retired.json`
independently proves that exact group absent after release. The holder records
A=0, B=0, quick=1. Its original GUI/holder waits are 0, and all 13 captured groups,
Server/Dashboard/GUI processes and Demo/socket/root paths are absent.

An optional rollback-only private SQLite gate held for 0.692 seconds with a
three-second ceiling. It caused an explicit `database is locked` Create rejection,
rather than a delayed successful Created receipt. The original failure is captured
in `native3-01`/`native3-02`; the gate performed no writes and its original wait
is 0. After that known rejection and gate release, an explicit native Enter retried
the preserved draft. Exactly one conversation/user entry and one inference result
were durable. No ambiguous mutation was automatically retried.

The terminal printed its own `NATIVE3_TERMINAL_OK` output while the quick worker
waited, and stayed selected as the quick result became unread
(`native3-03-terminal-while-completing`). This proves native background completion
keeps focus. Delayed successful native Create/Submit acknowledgement remains
unverified; the deterministic real-socket E2E supplies that ordering proof.

## Cleanup and remaining delivery gates

`focused-cleanup.json`, `native-cleanup.json`, `native-process-cleanup.json`,
`native2-cleanup.json` and `native3-cleanup.json` record owned roots/groups/processes. All three original GUI waits
and holder waits are 0. Recorded shell and worker groups, Demo Servers, Dashboards,
nested owners and sockets are absent. The Demo cleanup removed its own roots.
The coordinator removed each enclosing native root only after those checks.
No app-specific query ran after a native window closed.

Hosted macOS/Linux/headless, Linux 50-session capacity and memory acceptance,
installed Pi package checks, startup/Cursor packaging, independent final review,
the feature PR and the single work-diary closeout belong to the controller's
remaining delivery gates. No skipped/ignored helper, local macOS test, frame,
provider fixture or native screenshot is presented as another platform's pass.
No push or merge was performed by Task 5.
