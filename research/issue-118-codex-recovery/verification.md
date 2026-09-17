# Issue #118 development verification

Base: `e241f505efec53217d7229d70aea3f388c39e028`.
Branch: `implement-task-118`. The delivery commit/PR pins the tested source.
Native acceptance belongs to [#128](https://github.com/xlyk/ovrcr/issues/128), not this record.

## Behavior

- Existing managed Codex 0.153.0 root startup/prompt hooks retain their exact identity and history reference. The bounded `session_meta` header must match. No newest-file lookup, resume-last, prompt persistence or trust bypass.
- Shared recovery validates prerequisites and launches managed `codex resume UUID`. The same row, title and durable reference survive two restarts without another callback or prompt.
- Missing prerequisites and native exit retain an actionable row. Retry uses the same identity. Existing ownership, capacity, duplicate-request and run fencing remain authoritative.
- Initial resume reporting stays unavailable for the entire invocation. CLI and Dashboard expose it; `attached` stays false. Old activity and Unread are absent.
- Recovery names the last authoritative hook identity, not continuous selected history. A silent native backtrack cannot update the record before another supported hook. Native history/configuration must remain stable during reopen. See [supported configuration and limits](../../docs/codex-reporting-setup.md#retained-conversation-recovery).
- Wire protocol 19 appends Codex as reference tag 3. SQLite schema remains 5.

Pinned native grammar was inspected using `codex --version` and `codex resume --help` (0.153.0). The exact-ID branch of [Codex rust-v0.153.0 startup](https://github.com/openai/codex/blob/rust-v0.153.0/codex-rs/tui/src/lib.rs#L1370-L1398) exits on a missing session; unlike `--last`, it does not select StartFresh. No native conversation was launched for this development verification.

## Automated results (macOS, 2026-09-17)

| Command | Result |
| --- | --- |
| `SHELL=/bin/sh cargo test --workspace --all-targets --all-features --no-fail-fast` | 968 passed, 0 failed, 20 intentional ignores across 31 suites |
| `cargo check --workspace --all-targets --all-features` | Passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |
| `cargo test --workspace --doc` | Successful; zero runnable doc tests |
| `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs` | 43 passed |

Raw local attempts are retained in `/tmp/ovrcr-118-checks/`; final full run is `32-workspace-isolated-shell.log`. These results do not establish hosted Linux, capacity/high-water jobs, native GUI/provider continuity, or reboot acceptance. Ignored acceptance/load tests remain separate gates.

### Failed attempts retained

- Original integration regression failed because the conversation was null and Codex recovery unsupported. Startup and visible-resume-limit regressions also failed before their respective changes.
- The first controlled helper invoked the hook from the provider PID rather than a direct child. The production authentication correctly ignored it. The fixture now invokes the real hook CLI as a direct child; a separate negative case proves wrong-parent rejection.
- Early recovery validation incorrectly closed reporting for non-UUID reporting IDs. A focused regression and existing Ready/Unread tests caught this. Reporting retains its original ID contract; only recovery execution requires a canonical UUID. Non-resumable identities supersede old references without advertising resume.
- Recovery diagnostics initially hid live Ready text. The shared status renderer now preserves live provider activity alongside recovery diagnostics; both facts are asserted.
- The existing backpressure test failed on a `SessionChanged` event from the fixture's other shell, not an input response. Its wait now drains events within the original 300 ms budget and still rejects every response.
- Another full attempt hit the existing one-second Codex version-probe limit. A later GUI attempt inherited interactive zsh configuration, emitted compinit errors and missed palette input. No product timeout or assertion was relaxed. The final complete run used `/bin/sh` to avoid user shell startup.
- A serial full attempt exceeded the tool's 300-second deadline and is not a pass. Ordinary fixture cleanup and final-run cleanup tests passed; the interrupted run's individual descendants were not independently audited. No checkout test binary remained in the process check. Do not broadly kill processes to compensate for uncertain ownership.

## Reproduce the development seam

```sh
cargo test -p ovrcr --test retained_sessions codex_reopen_uses_exact_identity_without_prompt_across_two_restarts -- --exact --nocapture
cargo test -p ovrcr --lib report::codex
cargo test -p ovrcr-runtime --lib codex_recovery
cargo test -p ovrcr-tui --lib dashboard::status
```

The integration test runs the compiled application, private binary server, managed launcher, real PTY, exact root-child hook CLI and SQLite storage. It checks startup capture, foreign-parent rejection, old Ready/Unread reset, a newer decoy file, exact argv/cwd, same-row title, missing history/home/executable/workspace, changed CODEX_HOME, duplicate reopen, two restarts, native exit 23 and explicit retry. It checks that prompts, output and neighboring history content never enter the metadata database. Its native executable is controlled: this proves orchestration, not real Codex continuity.

## Review

Two sequential review passes covered the working diff against the base above; this harness had no subagent tool, so these were not independent reviews.

- **Standards:** shared Reporter/recovery/spawn boundaries, metadata-only persistence, appended protocol tag, unchanged readiness authority, isolated real-PTY tests and documentation checked. No remaining code blocker found.
- **Spec:** exact identity/no prompt, repeated restart, retained failure/Retry, unavailable reporting and common recovery capability covered. Native acceptance and hosted checks are not claimed.

## Acceptance-agent handoff

Use the PR's exact head SHA and build `cargo build --features gui`. Follow `docs/testing-computer-use.md` and the disposable GUI skill in this checkout; isolate OVRCR_CONFIG, OVRCR_SOCKET, workspace and CODEX_HOME. Do not use a live server or alter the user's hook configuration.

Use native Codex CLI 0.153.0 and review/trust the generated synchronous hooks through native Codex. Launch a fresh managed Codex session, establish a recognizable fact, and check that `terminal list --json` contains its exact recovery UUID. Restart only the owned fixture, reopen the same row, and verify native continuity without submitting a prompt. Repeat the restart before another turn. Verify the visible reporting-unavailable message, `attached: false`, absence of old Ready/Unread, missing-history failure, native failure/Retry and unchanged trust/approval settings. Record native versions, exact SHA, screenshots/accessibility, platform results and cleanup in #128. Development does not sign off those checks or close parent #112.
