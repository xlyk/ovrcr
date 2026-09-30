# Issue #161: Claude interrupted-history and doctor repairs

This is a scoped repair for [#161](https://github.com/xlyk/ovrcr/issues/161), the
oldest open issue by creation time (2026-09-23T17:44:04Z). It does not complete
Claude/Codex notification parity or accept permanent product limits.

The task began on main `46178d8d9753b226bf704811c2645a9a00aeaf87` and was reconciled
without conflicts onto `248e2e5f48681ea90c7516e09b149772dd92ae4b` after #233 merged.
The starting checkout's branch, index and unrelated CONTEXT/ADR edits were left
intact. Tests use separate configuration, sockets, workspaces and owned processes.

## Behavior and source evidence

The root transcript reader previously treated Claude's `<synthetic>` assistant
records as metered API responses. Their missing `requestId` poisoned the retained
usage prefix with `invalid_usage_record`; shared source health then suppressed
otherwise valid Ready delivery after resume. The accumulator now excludes that
explicit provider category before requiring API identity/usage. The reader still
validates the conversation and root/sidechain metadata first, and unknown malformed
API records remain failures. Counting remains Partial, with existing bounds,
duplicate/conflict rules and watermark behavior.

Read-only inspection of installed Claude Code 2.1.286 confirms the explicit model
constant, synthetic assistant constructor with absent request ID, and native usage
calculation excluding that model. [Artifact hash and reviewed facts](checks/source-review.json)
are separate from the native interruption observation reported in the
[issue's blocker record](https://github.com/xlyk/ovrcr/issues/161#issuecomment-5884397420).
No new real-provider turn, trust approval or provider-configuration change was made.

Doctor now reports `session_status=reporting_unavailable` while retaining binding
details, exposes only known diagnostic codes in `source_health.reason`, and gives
specific guidance for `identity_transition_unavailable`. Configuration repair cannot
recover that frozen lifetime; the native process remains usable and a new supervised
invocation is an explicit later user action. Arbitrary/private reason text stays
redacted. Setup does not rewrite settings or approve native trust.

## Regression evidence

- [Collector RED](checks/02-collector-red.log): real helper returned `invalid_usage_record` for a synthetic assistant between two real requests.
- [Doctor RED](checks/05-doctor-red.log): real managed launch/server/socket/CLI returned `bound` after a fork identity freeze.
- [Managed resume RED](checks/06-resume-red.log): reporting became Unavailable after interrupted history instead of Connected.
- [Accumulator GREEN](checks/07-focused-green.log): 12 owning tests, including no synthetic usage identity/counting.
- [Collector GREEN](checks/08-collector-green.log): 16 real-helper tests, including later real usage and rejection of a foreign synthetic row.
- [Doctor GREEN](checks/09-doctor-green.log): real managed fixture freeze, retained binding, exact reason, specific advice and subsequent native input.
- [Resume GREEN](checks/10-resume-green.log): real managed explicit resume, PTY/private transport/server/socket and shipped Dashboard. The new Ready reaches an external host capture after synthetic history without historical replay.
- [Privacy RED](checks/16-doctor-privacy-red.log), [setup/doctor GREEN](checks/17-doctor-privacy-green.log) and [safe-code unit GREEN](checks/18-doctor-unit.log): unknown/private reason remains null; original private-value assertion is preserved. The unavailable status expectation intentionally changed.

The [initial workspace run](checks/11-workspace.log) found the older unavailable-status expectation and the
raw reason disclosure; neither is hidden by weakening assertions. An earlier test
fixture call failed to compile after adding an interrupted-history parameter; the
[failed attempt](checks/03-doctor-red.log) is retained separately from behavioral RED.
Each retained check has a sibling JSON command, result, source revision and diff hash.

## Current-main verification

The first [workspace run on current main](checks/19-workspace-current-main.log)
failed two retained-Claude tests before reporting attachment:
`automatic_recovery_failure_requires_explicit_retry_even_after_reconnect` and
`claude_reopen_retains_exact_conversation_before_another_callback`. Both reported
the launcher's native fallback and lacked `RETAINED_CLAUDE_ATTACHED`. The
[serial retained-suite rerun](checks/21-retained-serial.log) passed all 20 tests
(two controlled helpers ignored). The failure is consistent with the existing
bounded-probe contention described in #62; its low-level cause was not separately
instrumented here. The original failure remains evidence, not a passed result.
The [complete serial workspace rerun](checks/22-workspace-serial.log) passed:
1,251 tests, zero failures, 21 ignored tests and zero filtered tests across 34
executables. Ignored controlled helpers, installed-provider and dedicated acceptance
fixtures are not passed gates. No assertions or timeouts were weakened.
[Current-main Clippy](checks/20-clippy-current-main.log) passed with all targets/features
and warnings denied; [formatting](checks/31-format-current-main.json) passed.
[Extension tests](checks/30-extensions-current-main.log) passed all 47 tests with
Node 22.23.2. [Doctests](checks/29-doctests-current-main.log) exited successfully but
executed zero tests, so they provide no behavioral coverage. Hosted current-commit
acceptance still needs its own results.

Native macOS CUA ran the actual reviewed checkout's GUI bundle, with its own server,
socket, configuration, disposable repositories and synthetic supervised Claude
process. Entering `ready` showed [Unread response ready and partial usage 11](native/26-native-ready-final.jpg)
with [matching accessibility text](native/26-native-ready-final.ax.txt), including
a separate `FIXTURE_READY_HANDLED` acknowledgement. Entering `freeze`, `doctor`
and `ping` showed [Unavailable, the exact doctor diagnosis and a separate input acknowledgement](native/27-native-doctor-input.jpg)
with [matching accessibility text](native/27-native-doctor-input.ax.txt).
The native fixture remained usable after reporting froze. This proves the native
terminal/dashboard/doctor application path with owned synthetic reporting data.

The first native doctor fixture attempt failed because it assumed the private
`OVRCR_SESSION_ID` would reach the native process. The launcher deliberately
sanitizes it. The [failed accessibility capture](native/25b-native-fixture-failed.ax.txt)
is retained; the fixture was corrected to receive the public session ID explicitly.
No product behavior changed for that fixture repair. [Fixture/provenance](native/provenance.json)
and [owned-resource inventory](native/native-owned.json) record the boundary.

Desktop notifications and sound stayed disabled. No OS permission, provider account,
global hook setting or live session was touched. Closing the GUI returned exit 0;
[cleanup verification](native/28-native-cleanup.json) found the recorded directory,
socket, GUI/server processes and all session groups gone. The worktree is retained
for review and follow-on stacked work.

## Remaining parent acceptance

- Claude AskUserQuestion/MCP elicitation and Codex questions still lack verified
  native root human-wait/open/close evidence. The current [Claude hook reference](https://code.claude.com/docs/en/hooks#elicitation)
  makes elicitation IDs optional and allows programmatic responses without a human
  dialog. This does not discharge the issue's independent-request/source gate.
- Ambiguous Claude background fork and Codex fork/backtrack/startup transitions
  remain fail-closed. No screen/silence/tool-lifetime identity heuristic was added.
- Codex clear-path coverage and history-picker timing still need native evidence.
- Genuine provider macOS Input/Reopen/switch and native desktop/sound acceptance,
  plus hosted current-commit Linux/capacity/memory acceptance, are not established
  by these synthetic fixtures. #161 remains open.
