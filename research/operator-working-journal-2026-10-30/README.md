# Operator Working journal — native acceptance, 2026-10-30

Tested product source: `271a1d6b497344484108f7b559dd40317e6c352e` on `codex/operator-layout-design`. Matching GUI/CLI and existing native holder were freshly built with `gui,acceptance-diagnostics`, copied before launch, and left unchanged until both original owners exited. [State and exact hashes](state.json), [launcher](launch.py), [provider inference counts](provider-finished.json), [durable conversations/turns/messages](durable-journal.json), and [cleanup](cleanup.json) bind this evidence to that revision. Later evidence-only commits have identical product source.

This is a private controlled local-provider run through normal synthetic OAuth and credential storage. Assistant prose is static fixture output (`A/B before tool`, `partial waiting`, `final`, `QUICK final`); actual compiled workers execute Bash and the actual CLI. These captures do not establish real-provider response quality or Linux rendering. No personal account, credentials, proxy, installed app replacement, or shared live instance was used. Existing installed owner PID 55305 was left alone.

## Pixels and full accessibility

Each JPEG has a full paired `.ax.txt.gz` and readable `.json` with source revision and original/stored AX hashes. Native captures are original JPEG bytes. Eight successful states plus two retained driver/size-limit attempts are listed below; all were inspected as actual pixels and full AX.

| Capture | Observed scope |
| --- | --- |
| [01 wide concurrent busy](01-wide-concurrent-busy.jpg) | 140×40. A and B both busy; B journal user cue, quiet header, fine guide, adjacent independently collapsed bash/success output rows, target, composer rules and real cursor. Colors remain readable without card fills. |
| [02 keyboard call and unread](02-keyboard-call-unread.jpg) | A completed while B remains busy; global one-unread and sidebar teal check. Keyboard Tab/Down/Enter expands call with full `call-fixture-b` ID and arguments while output stays collapsed. |
| [03 mouse output details](03-mouse-output-details.jpg) | Mouse separately expands output; call remains expanded. Full ID, `E2E_B_TOOL`, `CLI_BEGIN`, actual CLI JSON visible. Home restores top; long JSON continues below viewport through ordinary scrolling rather than being dropped. |
| [04 compact quick and terminal marker](04-quick-compact-terminal-marker.jpg) | Actual local terminal mode selected before input. Separate `OPERATOR_JOURNAL_INPUT_OK` output proves input beyond command echo. Ctrl+O opens seven-row, 80-column quick form over that terminal; ordinary title fill remains. |
| [05 multiline quick cursor](05-quick-multiline-cursor.jpg) | Four pasted lines, CJK/emoji and real cursor moved before the final period; content grows to nine rows. The footer changed to `No settings findings` because the disposable defaults file was intentionally reloaded while this draft was open; these two captures alone do not prove every underlying state is unchanged. Esc dismissal preceded normal Dashboard restart. |
| [06 native 80×24 attempt](06-native-80x24-journal.jpg) | Hidden sidebar preserves clipped chat identity, transcript guide and composer. Driver clicked B then incorrectly pressed Return after mouse already opened the composer; the existing busy/draft-retained error is visible and retained. No new durable turn resulted. This is not a clean error-free narrow frame. |
| [07 native minimum 72×24](07-native-60x24-quick-complete.jpg) | Attempted 60-column resize stops at actual native minimum 72 columns; filename preserves the attempted target. Fresh Quick shows one complete turn with two input lines and `QUICK final`, correct target and cursor. This is not 60-column proof. |
| [08 nested 60×24](08-nested-60x24-draft.jpg) | Kernel `stty size` receipt is 24 60. Separate owned nested Dashboard shows quiet journal identity, two-line Unicode draft, rules and cursor without clipping. |
| [09 nested 60×16](09-nested-60x16-floor.jpg) | Kernel receipt is 16 60. Six-line draft scrolls its four visible editor rows to the real caret; transcript floor remains above the target. Source/unit tests establish the nonempty three-row transcript case; this empty nested transcript does not independently prove that content case. |
| [10 nested floor quick](10-nested-60x16-quick.jpg) | Forwarded native mouse click on inner title-bar quick control (Ctrl+O belongs to outer Dashboard). Three-line Unicode quick input/cursor stays inside eight-row dialog; underlying draft remains visible while Quick is open. Cleanup sent Escape/Escape/q as one batch, with no separately captured post-dismissal state. Existing automated Quick cancel/restore tests provide separate dismissal coverage. |

The native 80×24 frame is exactly 80 cells wide and 24 rows; native minimum is 72×24. The nested frames use matching frozen bundled CLI inside the real native terminal, a separate `OVRCR_HOME`/socket and recorded kernel dimensions. An initial nested 60×24 attempt was invalidated by resizing the outer window, which reset its PTY size; original zero Dashboard/shutdown waits are retained under `nested-attempt-01-*`. The corrected runs set dimensions only after the outer window was stable. [Nested 24-row launcher](nested.sh) and [16-row launcher](nested16.sh) retain the original Dashboard and shutdown results.

## Lifecycle and limits

A and B each completed one durable turn (two provider inferences each: initial tool call plus continuation). Quick completed one durable turn (one inference). There are four outer conversations: one unused empty setup attempt with no turn, A, B and Quick. Cmd+A was ineffective in the terminal model field, appending `fixture-a` to the default in that empty attempt; the corrected new form used Home/Delete. No request was submitted from that empty conversation. The wrong setup key, busy Return, native minimum and invalidated nested resize are retained rather than erased. Nested conversations received unsent drafts only; no provider call or personal authentication was used there.

The existing busy error persisted on B after completion, matching the separately open stale-error scope. No product fix or cause claim is made. Long expanded JSON was inspected at the top; `CLI_END` was observed in AX after keyboard disclosure auto-scroll. No tail JPEG or exhaustive pagination was captured. A screenshot is one viewport, not an export of all output. The durable journal retains the actual complete output. Clipboard delivery, live subscriptions, Linux fonts, native delayed-success receipts and every possible disclosure/failure state are outside this bounded capture; existing compiled acceptance covers relevant ordering/failure paths.

Both native original wait results are **0**, including the unchanged holder's ignored manual entrypoint. The nested original Dashboard/shutdown waits are **0** for each recorded launch. Cmd+W did not close the window; the native close button did. The interim inventory is retained in `cleanup-pending.json`. After the actual close, all 22 recorded PIDs and 18 groups were absent, both sockets/demo were absent, and the task root was removed. No closed-app query was made. Frozen binary hashes still matched after close. Runtime logs and full AX are lossless deterministic gzip; metadata is readable JSON. Successful build commands/exits were observed in the tool transcript and summarized in state.json; complete build stdout was not separately archived.

## Local gates

Current-source assigned gates and their exact original waits/counts are recorded below after completion. Task1's 71 Operator/public 329/full serial library 332 passing results were not rerun just for confirmation. The original Task1 default-parallel library failure (331 pass/1 Bridge failure, unknown cause) remains a separate failed historical gate.

| Assigned command | Result | Original exit |
| --- | --- | --- |
| `cargo test -p ovrcr --test operator_e2e --features gui,acceptance-diagnostics compiled_worker -- --nocapture` | 12 passed, 0 failed, 2 ignored, 1 filtered. The manual native holder was separately explicitly invoked and passed. | 0 |
| Initial wrong target: `cargo test -p ovrcr --test terminal operator_quick -- --nocapture` | No tests executed: target `terminal` does not exist. Preserved as a command error. | 101 |
| Correct target: `cargo test -p ovrcr --test terminal_acceptance operator_quick -- --nocapture` | 1 passed, 0 failed, 18 filtered. One meaningful execution after broad/lint owners finished. | 0 |
| `cargo test --workspace --all-targets --all-features` | **FAILED**. Eleven completed binary summaries total 365 passed, 2 failed, 6 ignored; later binaries/packages did not run after Cargo stopped. `client_startup_server`: 18 passed, 2 failed, 2 ignored. | 101 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Finished without warnings/errors. | 0 |
| `cargo check --workspace --all-targets --all-features` | Finished successfully. | 0 |
| `cargo fmt --all -- --check` | No formatting differences. | 0 |
| `git diff --check` | No output at gate time; final intended full-range diff checked again after artifacts. | 0 |

[Exact commands, original waits, counts and raw/stored hashes](gate-summary.json); raw command output is in corresponding `.log.gz`. [Environment](gates-environment.json) records a private HOME/TMPDIR/XDG/instance and an allowlisted environment retaining only local build tool paths. No inherited API keys/provider profiles/proxy configuration were supplied. [Gate runner](gates.py) archives unmodified output and original waits; its current quick target corrects the recorded initial command typo. The successful corrected-quick root was removed. The failed broad root `/private/tmp/ovrcr-journal-gates-83a_s_6i` is preserved; [cleanup record](gates-cleanup.json).

The named workspace failures are `captured_build_survives_atomic_executable_replacement_and_decline` and `accepted_restart_runs_changed_binary_with_same_protocol_and_package_version`. Both fail at `tests/support/live.rs:520` in `copied_fixture()` → `start_binary()` socket admission, before their binary replacement or decline/restart assertions. Original logs say `fixture server socket did not appear`, shutdown response `None`, server stop `TimedOut`, cleanup incomplete. The fixture's later fallback calls kill/wait but does not log its result. Subsequent PID absence is inventory, **not an original successful cleanup wait**. No failure cause is established; no sandbox, contention, timing, pre-existing-flake or journal-exoneration claim, no assertion/timeout changes and no rerun.

The two preserved failed roots are `tmp/.tmphszYEy` (server 16934) and `tmp/.tmpnWT7Ns` (server 16946). [File inventory](workspace-failed-fixtures.json) and [subsequent process inventory](workspace-failed-process-inventory.json) retain their observed state. No further startup investigation or product changes were made. Final-head hosted macOS/Linux/capacity/memory outcomes and independent review remain controller-owned and pending here. These local results do not make the branch fully accepted.

## Corrected-source follow-up

The [bounded final-correction native record](final-correction-native-01/README.md)
tests product revision `9bbfb9d5`: busy/ready Send availability and selected-title
fallback for collapsed/offscreen sidebar rows. Its five pixel/AX pairs and original
owner waits are separate from the historical `271a1d6b` record above; the sources
are not equivalent across the product correction. Current hosted acceptance belongs
to Draft PR382 and the controller CI record referenced there.
