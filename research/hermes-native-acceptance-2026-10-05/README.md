# Hermes native acceptance, 2026-10-05

The installed Hermes command produced one distinct visible marker response from
each genuine OVRCR launch path on macOS. Both native Hermes processes exited 0.
Dashboard pause/resume was visible and controlled the foreground native process.
Normal GUI close failed with an epaint lock panic and required owned-process
recovery; later CLI-control GUI captures were stale. This is bounded acceptance,
not an all-passed native gate. [Issue #234](https://github.com/xlyk/ovrcr/issues/234)
remains open for the failures and unverified platform/configuration checks.

## Provenance and authorized budget

- Application source and native GUI build: `643e3122fbbcb0b3630d77ed4089e1d7a260e199`.
  The GUI bundle and CLI were built from the isolated acceptance checkout; no
  installed OVRCR binary or user configuration was replaced. The later changes
  in this package affect Hermes help/guidance and documentation only.
- Installed native command: `hermes`, a wrapper executing its existing Python
  virtual environment. Its clean source checkout was
  `5ef1409f50484dddc38c9665b32a837ff1b191af`, declaring `0.20.5` (2026.8.19).
  These identify this observation; OVRCR has no Hermes release allowlist or gate.
- Existing native default profile, configured provider `xai-oauth`, model
  `grok-4.6`; both native responses and owned session records name that model.
  No model/profile/authentication settings or native permissions were changed.
- Exactly **two sequential model turns**, one per launch path. Each prompt asked
  for its marker and prohibited tools, skills, file/memory writes, delegation and
  network fetches. The owned native session records contain one user and one
  assistant message apiece and **zero tool calls**. Actual spend is **unknown**:
  native `actual_cost_usd` is null and `cost_status` is `unknown`.
- Each session used the same task-owned empty Git repository/workspace, isolated
  OVRCR config, private socket and PTYs. OVRCR workspace isolation does not
  isolate the user's native Hermes profile. No existing session was adopted,
  interrupted or killed. Native startup retained its ordinary configured
  tool/skill/MCP discovery effects; these were not model tool calls.

## Observed results

The recorded [server/doctor/kernel observations](observations.json) retain the
captured values, including unavailable capabilities and the baseline doctor
guidance. Only the fixture's absolute path is normalized to `<empty fixture>`.

| Check | Result | Evidence |
| --- | --- | --- |
| Dashboard picker and managed launch | Agent `hermes`, session 14, supervisor PID/PGID 10332, native Python PID/PGID 10337 | `02-dashboard-session.json` in observations |
| Dashboard actual input and response | Submitted marker-only prompt; distinct `HERMES_DASHBOARD_OK` in the native Hermes response box | [AX excerpt](04-dashboard-response-ax-excerpt.txt), private original capture hashes in [manifest](capture-manifest.json) |
| Dashboard pause | GUI header gained `paused`; server phase `paused`; supervisor `Ts` and native foreground `T+` | [Paused AX](05-dashboard-paused-ax-excerpt.txt), `06-paused-session.json`, kernel states |
| Paused input refusal | Exit 1, `Conflict: session is paused; resume it before sending input` | `06-paused-input-rejected.json`; no additional model turn |
| Dashboard resume | Same supervisor PID, visible header no longer paused | [Resumed AX](07-dashboard-resumed-ax-excerpt.txt) |
| Dashboard native `/quit` | Native exit 0; visible `pid: closed`; truthful native-resume unavailable message | [Exit AX](08-dashboard-clean-exit-ax-excerpt.txt), `08-dashboard-exit.json` |
| CLI managed launch | Explicit `terminal create ... -- ovrcr agent run hermes -- hermes`; Agent session 15, supervisor PID/PGID 19404, native Python PID/PGID 19409 | `09-cli-created.json` |
| CLI actual input and response | `terminal send` returned ok; distinct `HERMES_CLI_OK` in the GUI native response box | [Response AX](10-cli-response-ax-excerpt.txt), `10-cli-input-result.json` |
| Truthful process-only capabilities | `activity: unknown`, `agent: null`, `unread: null`, context usage/title/recovery unavailable while running; doctor reports process lifecycle available and all reporting capabilities unavailable | Running inventories and `10-cli-doctor.json` |
| CLI pause/resume | Pause/resume commands returned success; pause inventory and kernel foreground states stopped | `11-cli-paused-session.json`, kernel observations; GUI reflection **failed/stale**, below |
| CLI native `/quit` | Server recorded exit 0 and no active PID | `12-cli-clean-exit.json`; GUI exit reflection **failed/stale** |
| Owned shell peer | Separate visible output markers after each native run; PTY geometry 126 columns × 57 rows | `13-peer-after-native-turns.json` |
| Existing host Hermes peers | All five recorded pre-existing PIDs still alive at cleanup | Final verification; this proves process continuity, not an untested conversation |
| Model/tool budget | Two sessions, two user messages, two assistant messages, zero tool calls/messages; both ended `cli_close` | `native-owned-session-counts.json` |
| Native process cleanup | All 23 recorded task-owned session groups gone; owned GUI/server absent; socket and disposable fixture roots removed | `verification.json`; GUI recovery exited 143 |

Pause proves the OVRCR supervisor and direct foreground native process stopped.
The recorded auxiliary native helper stayed `Ss`; this record does not claim all
descendant helpers paused. Live native interruption was not exercised within the
two-turn budget; the controlled PTY test covers interrupt forwarding separately.
Native Hermes printed its own resume suggestion at exit, while OVRCR correctly
kept retained native recovery unavailable; no resume was attempted.

## Failed and unverified gates

- **Stale CLI GUI/AX:** after the optional CLI pause, the GUI did not show pause,
  subsequent resume, or native exit. The later captures still show PID 19404,
  elapsed 2m, `agent unknown`. They are preserved as
  [post-pause failure AX](11-cli-paused-ax-excerpt.txt) and
  [post-exit failure AX](12-cli-clean-exit-ax-excerpt.txt), not labeled as visible
  lifecycle passes. Server inventory and kernel state establish the underlying
  control/exit results. Raising the window and bounded keyboard attempts did not
  restore the visible updates.
- **Normal GUI close failed:** the actual close button left the owned launcher
  alive and its output thread panicked with
  `DEBUG PANIC: Failed to acquire RwLock read after 10s. Deadlock?`.
  A bounded macOS sample showed the main thread in `App::close → Terminal::stop
  → Child::wait → __wait4`. Recovery shut down the exact owned server/socket and
  sent SIGTERM to the exact owned GUI PID. Launcher exit **143** is a recovery
  result. See [failure and hypothesis](gui-close-failure.txt). No GUI fix or
  definite root cause is claimed.
- **Unverified:** native Linux, other models/providers/auth modes/profiles,
  login/refresh flows, native hook delivery, native live interrupt, recovery and
  spend. Ready/Unread, Input requests, metrics and generated titles remain
  unsupported by this process-only adapter; native transcript values are not
  imported into those OVRCR capabilities.
- The full workspace regression is recorded separately from these bounded
  native results. A failed or unrun check cannot be converted into a native pass.

[Attempt notes](attempts-and-gaps.txt) preserve the transient CUA error and
corrected capture-script/CLI-command assumptions separately from product results.

## Evidence handling and native effects

The [capture manifest](capture-manifest.json) records SHA-256 hashes and original
dimensions for the whole, byte-identical screenshots and full macOS accessibility
snapshots. They remain in the private task evidence directory, with exact local
paths in the work diary. Whole images contain unrelated live quota and installed
native tool/skill inventory, so no screenshots are committed. The public AX files
document exactly which native terminal text indices/column are retained. No crop,
image edit or generated screenshot was used. The
[observation source manifest](observation-source-manifest.json) identifies the
original JSON files behind the normalized public observations.

Only the two owned native session IDs were queried read-only in native SQLite,
for model, counts, roles, end reason and cost status. No message/system-prompt
bodies, credential content or unrelated session records were read for publication.
Metadata (size/mtime/mode) stayed unchanged for native config, auth, `.env` and
both memory files; this is metadata evidence, not a content-hash claim. Native
`state.db` and WAL changed and their modes did not. The two history records are
retained; existing peers can also write the shared DB, so all DB changes are not
attributed to this run. No native history, caches or logs were deleted.

Cleanup used only recorded owned PIDs/groups and the private socket. A temporary
permission error on a group probe was retained as unverified until a later probe
confirmed removal; no permission bypass was used. The shared launcher group and
pre-existing native sessions were not signalled. The evidence directory is
retained after the empty fixtures were removed.

## Automated verification

- `cargo build --locked --offline -p ovrcr --features gui --bins`: passed before
  native use at the application source revision above.
- [Baseline focused tests](focused-hermes-tests-baseline.log): **3 CLI + 1 server
  lifecycle tests passed**, with real controlled child/PTY/socket/peer coverage.
  These synthetic fixtures are separate from installed-model acceptance.
- [Final-source focused checks](focused-final-source.log): the same **3 CLI + 1
  lifecycle tests passed**, plus **1 Dashboard Hermes detection test passed**.
  `cargo fmt --all -- --check` passed. Broader regression status is retained
  separately in this directory.
- [Serial full-workspace attempt](full-workspace-final-source.log): **190 passed,
  1 failed, 1 ignored**, exit 101. It stopped in unchanged
  `claude_probe_off_spawns_nothing_and_waits_for_a_managed_session` at
  `tests/claude_quota_probe.rs:166`, actual `Unavailable` vs expected `Checking`.
  Later workspace test binaries were **unrun**. Native-home environment was
  inherited, without empty-home overrides. No assertion or test was changed.
- [One exact replay](claude-quota-isolated-replay.log): **1 passed**, with initially
  empty task-owned `GROK_HOME`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `HERMES_HOME`.
  The native-home directories contained 78 files after fixture startup and were
  removed after the test exited. This environment-dependent observation does not
  prove the full suite passed or establish the failure cause. Exact-PR-head CI
  and independent review remain required before delivery.

The launcher continues to supervise whichever Hermes executable is installed at
runtime without OVRCR hooks, auth changes or native argument changes. Doctor
remains a read-only filesystem/session probe and cannot establish a current
version, active profile, authentication or model from this historical record.
