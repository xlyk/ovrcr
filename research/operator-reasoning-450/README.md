# Operator reasoning level — #450

Implementation: `7bbc7de4` plus reviewed corrections `62d6d023`, based on
`db89a90b`. Local branch: `codex/450-operator-reasoning`. No push, PR, merge,
installation or live provider call was performed.

## Result

- Added `operator.thinking_level` to the shared settings catalog, loader, editor,
  CLI help and settings matrix. Values: `default`, `off`, `minimal`, `low`,
  `medium`, `high`, `xhigh`, `max`; default `default`.
- Submit captures model and reasoning together. Each turn applies the captured
  reasoning after model selection; `default` sends no `set_thinking_level`.
- Pi's `get_state.thinkingLevel` and `get_available_thinking_levels` supply the
  actual level and capabilities. Invalid readback fails before prompting. A new
  model clears the previous capabilities before readback, including failure.
- The shared header hit rectangle and `Ctrl-t` open a supported-only picker.
  Enter sends `SetSetting` through the Outbox; Esc closes. Failed writes retain
  the saved value and display their error. The header distinguishes substitution
  from a next-turn change, including a pending return to `default` after Stop.
- Protocol 78 appends `ReasoningSelected` and the required fields. Schema 30 adds
  captured turn reasoning and optional conversation readback without rewriting
  journals, checkpoints or legacy records. Migration and reopen are covered.

## Verification

Commands ran in the isolated worktree's own `target/`. Runtime tests used a
private process HOME; binary fixtures supplied their own HOME, socket, workspace
and fake Pi profile. The final runs below exited zero. Compressed logs are beside
this report.

| Command (with `rtk proxy`) | Result |
| --- | --- |
| `cargo check --workspace --all-targets` | Pass |
| `just fmt-check` | Pass |
| `just lint` | Pass; all targets/features, warnings denied |
| `cargo test -p ovrcr-protocol` | 103 unit + 8 integration passed |
| `cargo test -p ovrcr-runtime -- --test-threads=1` | 495 unit + 12 cancellation + 23 fallback + 17 settings matrix + 3 theme tests passed; 1 internal subprocess helper ignored |
| `cargo test --test operator_worker --test operator_e2e --features acceptance-diagnostics -- --test-threads=4` | Worker 50 passed; E2E 18 passed, 4 opt-in cases ignored |
| `cargo test --test tui operator` | 54 passed; 411 unrelated tests filtered out |
| `cargo test -p ovrcr-tui -- --test-threads=4` | 442 unit + 14 integration passed |
| `cargo test --lib operator::worker` | 10 root worker unit tests passed; other workspace libraries had zero matches and supply no additional evidence |
| `OVRCR_TUI_UPDATE_SNAPSHOTS=1 cargo test -p ovrcr-tui frames_at_floor_and_wide_sizes_match_the_singleton_layout` | 1 snapshot generator test passed; final ordinary TUI suite verified the saved snapshots |

The final named suites account for 1,249 passing tests and 5 ignored cases.
No ignored case is claimed as passed. Three ignored E2E cases require explicit
installed-Pi opt-in; the fourth is the manual provider helper. The runtime helper
is selected only by its owning test.

Earlier attempts exposed sandbox denial of sockets, PTYs and Bridge fixture
clients. Repeating with execution access resolved those failures. Concurrent
worker runs hit three cold-start timeouts; four-thread rerun passed all 50.
One later concurrent runtime run timed out waiting for a fake Codex app-server
PID; the complete serial rerun passed. Assertions and deadlines were preserved.

## Intentional expectation changes

- Catalog path/type lists and the Settings editor row count include one new enum
  setting. The full settings-document fixture explicitly sets it.
- Protocol wire snapshots were regenerated with the version bump to 78.
- Three migration assertions now expect schema 30 instead of 29.
- Worker/E2E expectations for restoring startup thinking now require no thinking
  write under `default`, and assert Pi's model-specific readback. The stand-in Pi
  re-derives model thinking and silently clamps unsupported levels like Pi does.
- The removed `ModelStep::Thinking` refusal branch no longer has a test assertion;
  structured model refusal and transport-failure assertions remain.
- Five singleton frame snapshots include the new header control and the extra
  header row where required. Existing title, transcript, scrolling and minimum
  composer assertions remain.

## Computer use

The user clarified that acceptance should use only the OVRCR GUI TUI.
`just gui` launched a disposable instance using this checkout and the deterministic
fake Pi peer. Its initial setting and fake Pi default were both `low`. Real
keyboard and mouse input went through the native GUI and compiled Dashboard.
The first walkthrough used `7bbc7de4`; a final rebuilt `62d6d023` fixture also
accepted a typed prompt and displayed the reasoning picker (`08-final-picker`).

| Evidence | Observed result |
| --- | --- |
| `01-native-low.jpg` | Typed prompt produced `REASONING_FIXTURE_READY`; actual level `low` |
| `02-native-picker.jpg` | `Ctrl-t` opened `default` + minimal/low/medium/high, ticked low, and named off/xhigh/max as not offered |
| `03-native-next-turn.jpg` | Down + Enter saved medium through the Server while the held turn remained low; header said `low → medium next turn` |
| `04-native-applied.jpg` | Stop then a new typed prompt used medium |
| `05-native-substituted.jpg` | CLI saved xhigh in the same disposable instance; a new turn reported `high (set: xhigh)` |
| `06-native-80x24.jpg` | Automatic sidebar hiding and two header rows preserved model, actual/stored reasoning and Stop |
| `07-native-72x24.jpg` | Same controls at the native helper's minimum width |
| `08-final-picker.jpg` | Reviewed `62d6d023` build: typed prompt, actual low, supported-only list and effective tick |

The header click also opened the same list; Esc dismissed it without leaving the
pane. Accessibility text is saved beside each screenshot. The helper's minimum
width is 72 columns; 60×24 and 60×16 are covered by the ordinary render and frame
tests. The picker adds one border, one checkmark for the level in use, and one
unsupported-level sentence; it adds no per-row status markers or nested border.

The initial GUI fixture exited zero and removed its instance directory. Three
earlier timed-out test roots were removed only after their exact owned server
PIDs were confirmed absent. The isolated worktree and its evidence remain for
review. Installed Pi, real credentials, live inference, Linux GUI, and hosted CI
were not exercised.

A final repeat initially met a mismatched disposable Server build; that fixture
was closed and a frozen bundled client/server was launched with inherited
instance/server overrides removed and a private HOME. CUA then timed out on the
`/private/tmp` app path. Its bundle-id lookup reported three other-checkout
registrations of `dev.ovrcr.gui`; `/tmp` allowed the final picker capture, but
further input did not reach that window and ended with `noWindowsAvailable`.
A temporary unique bundle id appeared in inventory but CUA returned `Invalid app`;
it was restored. The additional final-build pending-default GUI repetition was
not completed; its regression test passed. Cleanup used the exact task-owned
GUI and its verified direct-child bundled Server: scoped shutdown exited zero,
GUI termination returned 143 as expected, their PIDs were confirmed absent, and
the final fixture/image were removed. Earlier successful GUI observations are
kept separate from that incomplete repetition.

## Review

Independent Luna/max review checked `7bbc7de4` against `db89a90b`, then confirmed
the exact correction commit `62d6d023`. The three blocking findings were fixed:
the missing settings-matrix type expectation, pending default display after a
turn, and stale capabilities after failed readback on a new model. No remaining
blocking finding was reported. The reviewer did not run additional tests.

## Scope

At the user's explicit request, the live Operator's dedicated Pi
`settings.json` was separately set to `defaultThinkingLevel: low` and read back.
That is a profile setting for its next process; no live OVRCR instance was
started, stopped or upgraded. No provider-specific capability table, new
dependency, per-conversation override, or extra reasoning policy was added.

Issue: https://github.com/xlyk/ovrcr/issues/450

## 2026-11-11 CI follow-up

[macOS shard 1](https://github.com/xlyk/ovrcr/actions/runs/42982894977/job/130397212977)
failed in the root `registry_migration` integration suite: the schema-28 upgrade
case still waited for version 29 and asserted `30 == 29`. The aggregate `checks`
failure was a consequence of that shard failure.

The exact test reproduced locally before editing (one test failed; preserved in
`ci-migration-red.log.gz`). Its expected current version is now 30, and it also
asserts the new conversation `reasoning` and turn `thinking_level` columns exist.
The fixture's historical schema 28, legacy journal/provider/model bytes,
singleton lifecycle, foreign-key assertions and deadlines are unchanged.

`cargo test -p ovrcr --all-features --test registry_migration -- --test-threads=4`
then passed 13 tests with one internal helper ignored
(`ci-migration-green.log.gz`). `just fmt-check` and `just lint` passed. This fix
changes the integration test only; it changes no product behavior or workflow
selection.
