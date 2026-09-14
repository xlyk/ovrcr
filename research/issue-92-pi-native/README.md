# Issue 92: native Pi acceptance

Native macOS acceptance run for [#92](https://github.com/xlyk/ovrcr/issues/92), the Pi
status-feature delivery gate, following
`.superpowers/sdd/2026-09-14-pi-omp-phase5/native-gates-runbook.md` §§0–3.1, 4, 5, 7, 8.
§3.2 (Oh My Pi, #97) was not run: it waits on #96.

**The run is not clean.** Two defects were found, one isolation rule was broken by the
operator, and several criteria could not be exercised. Read
[Defects found](#defects-found), [The untouched check failed](#the-untouched-check-failed)
and [Retained failed and unrun attempts](#retained-failed-and-unrun-attempts) before
treating anything below as a pass.

## Revisions

| Thing | Value |
| --- | --- |
| OVRCR source revision | `af41fef657ed37c68922bc43188e7043189b06c1` (merge of #106; ancestor of `origin/main`, verified) |
| OVRCR built binaries | `ovrcr` `d07b737c8993518c8a77de7d13934adb4b6eccf48b49ead80f304f2edeadc27f`, `ovrcr-gui` `0e9c7f7438633a61e7aa7cd9380c257e563f42c76dcd9e1c17e6761e34ca1768` |
| Application bundle | `target/OVRCR GUI.app`, assembled 2026-09-14T07:00:19Z (runbook §2 layout; `just gui` cannot be used under a shared `CARGO_TARGET_DIR`) |
| Provider executable | `/Users/xlyk/.local/bin/pi` |
| Provider version (as printed) | `0.85.1` |
| doctor `version_status` | `tested` (final run); `unknown` in the first run — see Defect 1 |
| Model | `openai-codex/gpt-5.3-codex-spark`, OAuth (ChatGPT subscription quota, not per-token billing), thinking off |
| Fixture root | `$TMPDIR/ovrcr-native-gates.hBK8Z5` (removed after the evidence was copied out) |
| Host | macOS 26.5.2, arm64, 14 cores |

`02-binaries.txt`, `01-revision.txt`.

## Isolation

| Check | Result |
| --- | --- |
| Files copied into the fixture (names only) | `auth.json`, `models-store.json` from `~/.pi/agent/`; nothing else, nothing from `~/.omp` |
| Fixture `settings.json` | written fresh: `defaultProjectTrust: "never"`, `defaultProvider`, `defaultModel`, `defaultThinkingLevel: "off"` |
| `HOME` during the run | `$ROOT/home` — confirmed from inside an OVRCR terminal (`04-launch-shape.txt`) |
| `PI_CODING_AGENT_DIR` | never exported |
| `find ~/.pi -newer marker-before` | **NOT EMPTY** — see below |
| `find ~/.omp -newer marker-before` | 7 paths, all written by Kyle's own live `omp` processes; nothing in this gate touched `~/.omp` |
| Fixture received the writes (`98-fixture-writes.txt`) | yes, 14 paths including `$ROOT/home/.pi/agent/sessions/` |
| Other `pi` instances alive during the window | none |
| Live provider settings modified | none — but see below |

`OVRCR_CONFIG` and `OVRCR_SOCKET` could **not** be set by the operator: `src/gui.rs:256-257`
makes `Demo` create its own disposable root and override both. The runbook's §2 launch block
is wrong about this. The run was still isolated — the dashboard used
`$TMPDIR/ovrcr-gui-*/config.toml` and `server.sock`, never the live ones — but at a path the
GUI chose. `OVRCR_DASHBOARD_CONFIG` is inherited and did take effect.

### The untouched check failed

`find ~/.pi -newer marker-before` printed `/Users/xlyk/.pi/agent`. That is the directory's own
mtime; `find ~/.pi/agent -mindepth 1 -newer marker-before` printed nothing, and every entry
inside (`auth.json`, `models-store.json`, `settings.json`, `trust.json`, `sessions/`,
`extensions/`, `skills/`, `git/`, `npm/`) still carries a pre-run mtime. A file was created
and removed inside `~/.pi/agent` at 00:14:09.

The cause is the operator's, not OVRCR's. While characterising Defect 1 the real Pi binary was
timed and sampled against the **real** home instead of the fixture home
(`/usr/bin/time -p /Users/xlyk/.local/bin/pi --version`, and a `sample` run at ~00:14). Pi
created and removed a transient file in its own agent directory. No credential, session,
setting or trust file was written, changed or deleted, and nothing was read out of any of
them. Every other measurement in `05a-pi-probe-failure.txt` used `HOME=$ROOT/home`, and the
measured run itself (from `04-launch-shape.txt` onward) was entirely inside the fixture.

This breaks the runbook's hard rule "Nothing is written to `~/.pi`". It is recorded, not
absorbed. Full detail in [`native/99-untouched.txt`](native/99-untouched.txt).

## Turn ledger

Budget: 10 real turns, hard cap 12. **12 were spent.** Retries and one accident are counted
and retained, not hidden.

| # | Step | Prompt (verbatim) | Outcome | Artifact |
| --- | --- | --- | --- | --- |
| 1 | P9 | `Reply with the single word: ALPHA` | Provider refused the model (`gpt-5.4-mini` is not supported on a ChatGPT account). Settled as **Error**, no Ready, no Unread. A real provider failure, so no synthetic one was invented. | `14-pi-error.json` |
| 2 | P2 | `Reply with the single word: ALPHA` | Busy → ResponseReady, quality `Confirmed`, unread turn `…:1` | `07-pi-ready-1.json`, `.jpg` |
| 3 | P3 | `Reply with the single word: BRAVO` | ResponseReady, unread turn `…:2`, both responses distinct on screen | `08-pi-ready-2.json`, `.jpg` |
| 4 | P6 att. 1 | `Count slowly from 1 to 20, one number per line.` | **Failed attempt.** Response finished in under 2 s, so the Busy window was never sampled and the follow-up could not be queued. Retained. | `11-pi-continuation.json` |
| 5 | P6 att. 2 | `Count slowly from 1 to 20, one number per line.` | Busy observed; the earlier unread `…:3` survived the new Busy | `11-pi-continuation.json` |
| 6 | P6 | `Then reply: CHARLIE` | Queued while Busy. State stayed Busy on the same turn and revision, then exactly one ResponseReady and exactly one new unread: one cycle for two prompts. | `11-pi-continuation.json` |
| 7 | P8 att. 1 | `Count slowly from 1 to 200, one number per line.` | **Failed attempt** for cancellation; instead produced a spontaneous native `source_gap` mid-stream (Defect 2), which became the P14 gap evidence. | `20-pi-gap.json`, `.jpg` |
| 8 | P14 | `Reply with the single word: DELTA` | Pi answered on screen; reporting gapped again immediately after the reattach and the response was never reported as Ready (Defect 2). | `21-pi-reattach.json` |
| 9 | P8 att. 2 | `Count slowly from 1 to 200, one number per line.` | **Failed attempt.** 200 lines streamed in ~1 s; Escape could not be delivered in time. See "cancellation" below. | `13-pi-cancel.json` |
| 10 | P10 | `Reply with the single word: ECHO` (on `pi-child`) | Child Ready with its own unread; root untouched. Background alert dispatched. | `15-pi-root-child.json`, `.jpg` |
| 11 | P13 | `Reply with the single word: FOXTROT` | Normal Busy → Ready in the session created by `/new` | `19-pi-transitions.json` |
| 12 | P13 | `Reply with the single word: FOXTROT /tree` | **Accident.** Pi's input box still held the previous prompt, so `/tree` was appended to it and submitted as an ordinary prompt instead of invoking the command. `/tree` is therefore NOT RUN. This spent the last turn of the cap. | `19-pi-transitions.json` |
| | **Total real turns** | | **12 / cap 12 (budget 10)** | |

## Acceptance matrix

Criteria verbatim from the ticket.

| Criterion | Evidence | Result |
| --- | --- | --- |
| An isolated native macOS run of the reviewed OVRCR build launches Pi from the actual Dashboard picker and proves two distinct responses, Busy/Ready, explicit review, an earlier Unread surviving new work and a stale acknowledgement unable to clear a newer response. | `05-pi-bound.json`, `06-pi-doctor.json`, `04-launch-shape.txt`, `07-*`, `08-*`, `09-pi-stale-ack.json`, `10-pi-review.json`, `11-pi-continuation.json` | **passed** |
| Native Pi continuation/retry/compaction, failure, cancellation, root/child isolation and actual extension-dialog WaitingInput are demonstrated. No built-in approval surface is fabricated for Pi; a real extension dialog provides the required wait evidence. | `11-*`, `12-pi-compaction.json`, `14-pi-error.*`, `13-pi-cancel.json`, `15-pi-root-child.*`, `16-pi-waiting.*`, `17-pi-waiting-closed.json` | **partly open** — continuation, failure→Error, root/child isolation and the real `ctx.ui.select` / `ctx.ui.confirm` WaitingInput all passed. **Cancellation OPEN**, **compaction OPEN**, **provider retry OPEN** (see below). |
| Enabled background Response ready and Input needed delivery is visibly/audibly verified as applicable, with current preferences, visible-pane suppression, duplicate suppression, closure and reconnect baselines. Screenshots/accessibility cover tiny and normal Dashboard layouts. | `18-pi-alerts.json`, `18-pi-alert.excerpt.txt`, `18-pi-alert-dispatch.log`, `18-pi-prefs-enabled.jpg`, `17-pi-waiting-closed.json`, `15-pi-root-child.ax.txt` | **partly open** — both alert kinds dispatched with identity-only bodies and one Glass sound each, preferences enabled, per-identity dedup and closure all observed. **Visible-pane suppression OPEN**, **reconnect baseline OPEN**, **tiny layout OPEN**, **actual display/audibility OPEN** (ticket's own caveat). |
| Native new/resume/fork/tree/reload, bounded reporter reattachment, old-event rejection, detach/reattach, native output/exit/signals and owned cleanup are proven without replaying historical attention events or modifying live user settings. | `19-pi-transitions.json`, `20-pi-gap.json`, `21-pi-reattach.json`, `22-pi-cleanup.json`, `99-untouched.txt` | **defect + partly open** — `/new`, `/resume`, `/fork`, reattachment, exit, signals and cleanup all passed with no replayed Ready. **`/tree` NOT RUN**, **reload NOT RUN**, **detach/reattach OPEN**. Reattachment exposed **Defect 2**. "Without modifying live user settings" is qualified by the untouched-check failure above. |
| Native Linux Pi lifecycle and relevant PTY/socket/process behavior are tested separately… | — | **OPEN GATE**: no Linux host is reachable from this machine. macOS UI evidence and automated Linux fixtures are not a substitute. |
| An isolated-host fifty-session replay exercises the Pi reporting path through actual sockets… | — | **OPEN GATE / NOT RUN**: see [Fifty-session replay](#fifty-session-replay). |
| Current-revision workspace tests, lint, formatting, wire compatibility and required hosted checks pass with executed counts. | `native/automated/` | **partly open** — see [Automated results](#automated-results-at-this-revision). Formatting, lint and scoped Pi suites passed; the full workspace run was deliberately not executed here; hosted checks require a push. |
| Support/setup/removal/doctor documentation states tested source evidence without exact-version allowlisting… | — | **NOT RUN**: no documentation work was in scope for this verification dispatch. `doctor` already reports `supported_versions: "any compatible release; tested versions are evidence, not an allowlist"` and `tested_versions: ["0.85.1"]` (`06-pi-doctor.json`), but the docs themselves were not reviewed. |
| Evidence records executable/source/application revisions, commands, results, counts, fixture PID/PGID ownership and verified resource cleanup. Completion does not claim OMP support, accounting, merge, release or live installation. | this file, `22-pi-cleanup.json` | **passed** |

## Defects found

Neither was fixed; both are recorded for separate dispatch.

### Defect 1 — the 1 s `--version` probe disables Pi reporting under host load

`src/report/admission.rs` `probe_command()` bounds the `<exe> --version` admission probe at
`Instant::now() + Duration::from_secs(1)`. `src/report/extension.rs` `receiver()` turns a
`None` from that probe into `receiver.disable()` and prints
`"Pi reporting unavailable (version probe unavailable); running native command"`.

At host load ~88 on 14 cores, `pi --version` took **6–11 s** (user CPU 0.45 s — the rest is
scheduler starvation), the probe failed, and the first picker launch bound nothing:
`probe_status: "unavailable"`, `session_status: "unbound"`, no Ready, no Unread, no alerts for
that session. At load ~7.8 the identical command takes **0.21 s** and the same picker launch
binds cleanly with no workaround.

So this does **not** reproduce on an idle machine. It reproduces reliably on a busy one —
which is when background Ready alerts are worth the most. The same class of fixed sub-second
budget appears in Defect 2.

Evidence: [`native/05a-pi-probe-failure.txt`](native/05a-pi-probe-failure.txt), including the
correction after re-measuring at low load. Both P1 attempts are retained.

### Defect 2 — a single dropped frame pauses reporting, and it recurs immediately after recovery

`src/ovrcr-reporting-transport.mjs` spawns one short-lived `ovrcr report pi --stdin` helper per
frame and `SIGKILL`s it at `deadlineMs = 900`, while `producer.sequence` has already advanced.
A killed helper therefore leaves a hole, and `Receiver::admit` reports
`sequence > last + 1` as `Admission::Gap` → `pause("source_gap")`.

This happened **spontaneously**, with no test hook set (`OVRCR_TEST_DROP_SEQUENCE` was never
set anywhere in this run), while Pi streamed a 200-line response at load ~85. Observed
afterwards, exactly as the design intends: `health.state: "Unavailable"`,
`health.reason: "source_gap"`, activity frozen, native session untouched,
`lifecycle.delivery: "paused_recoverable"`, and a remediation line naming `/ovrcr-reattach`.

`/ovrcr-reattach` recovered correctly — generation 1 → 2, health back to `Connected`, activity
from Pi's own `isIdle()` (never a resurrected Ready), and the in-Pi notice
"OVRCR reporting reattached". **But the very next prompt gapped again immediately**: Pi
answered `DELTA` on screen while OVRCR stayed frozen at Idle in `source_gap` and never
reported the response. A second `/ovrcr-reattach` (generation 3) then ran normally once load
had dropped.

On a loaded host the feature is therefore not merely degraded but effectively unusable, and
the failure is silent to anyone not watching `doctor`. Measured at load ~29 the helper itself
spawns in 10 ms, so the 900 ms budget is generous in the ordinary case and brittle in exactly
the case that matters.

Evidence: [`native/20-pi-gap.json`](native/20-pi-gap.json),
[`native/21-pi-reattach.json`](native/21-pi-reattach.json).

### Observation — a non-cycle Pi error does not move OVRCR's activity

`/compact` failed inside Pi with `Error: Compaction failed: Nothing to compact (session too
small)`. OVRCR stayed at `ResponseReady` on the previous turn and did not flip to Error, did
not create a second Ready and did not move the binding. That looks correct — compaction is not
an assistant cycle and no settled boundary fired — so it is recorded as an observation, not a
defect. `12-pi-compaction.json`.

## What could not be exercised, and why

- **Cancellation (P8).** `ovrcr terminal send` is a *paste* channel: `Session::send_text`
  routes through `encode_paste(text, bracketed_paste)`, and Pi has bracketed paste on, so a
  bare `ESC` arrives as literal pasted text and never as an interrupt. Proven by sending
  `ESCPROBE`, then `ESC`, then `ESCPROBE2`: the input box read `ESCPROBEESCPROBE2` with the
  buffer uncleared. The alternative — pressing Escape in the GUI — lost its race twice: the
  model streamed 200 lines in about one second, shorter than one computer-use round trip.
  **OPEN.**
- **Compaction (P7).** Pi refused to compact: the session was 1.4 % of a 128 k context.
  Growing it enough would have cost far more than the remaining turn budget. What *was*
  observed across the attempt — no second Ready, binding unchanged — is recorded. **OPEN.**
- **Provider retry.** No provider-side retry occurred and none could be forced without
  spending turns on a failing provider. **OPEN.**
- **`/tree`, reload.** `/tree` was consumed by the input-box accident of turn 12 and the cap
  was reached. Reload was never reached. **NOT RUN.**
- **Visible-pane suppression, detach/reattach, tiny layout, Ctrl-D exit.** All need keystrokes
  in the dashboard. Partway through the run the computer-use background input path stopped
  reaching the window (screenshots kept working; a fresh window later accepted input again,
  so the window instance, not the tool, was the problem). `computer_batch` and every
  display-scope call were refused with "user interrupt" for the whole session, and the CLI has
  no `terminal select`, no detach and no way to resize the native window. **OPEN.**
- **Accessibility dumps.** This window publishes the whole dashboard as one `AXTextArea`
  named "OVRCR terminal" and withholds row text, so AX cannot assert rendered rows here. Text
  assertions therefore come from `ovrcr terminal read --json` (the server's own rendered
  screen) and visual assertions from the screencaptures. Recorded in
  [`native/15-pi-root-child.ax.txt`](native/15-pi-root-child.ax.txt).
- **Actual alert display and audibility.** Dispatch was observed by sampling for the
  `osascript -e 'display notification …'` and `afplay …/Glass.aiff` children with their argv.
  A successful host command does not prove a banner was drawn or a sound was heard: Focus/Do
  Not Disturb, notification permission and output volume decide that, and macOS may silently
  suppress `osascript` notifications. Same caveat as issue 59. **OPEN.**

## Alerts

Both kinds fired while neither Pi pane was the visible pane, each with exactly one Glass sound:

```
2026-09-14T07:45:31.339Z NOTIFICATION  consigint / worktree-lifecycle / pi-child (#14) OVRCR · response ready
2026-09-14T07:45:31.512Z SOUND afplay
2026-09-14T07:46:49.985Z NOTIFICATION  consigint / auth-handoff / pi-root (#13) OVRCR · input needed
2026-09-14T07:46:50.142Z SOUND afplay
2026-09-14T07:46:56.120Z NOTIFICATION  consigint / auth-handoff / pi-root (#13) OVRCR · input needed
2026-09-14T07:46:56.201Z SOUND afplay
```

Only project / workspace / terminal name and id appear — no response text, prompt text, model
name or path. Request identities `p3`, `p4`, `p5` produced one alert each, and no repeat alert
was dispatched for `p5` while it stayed open. No full Notification Center capture is committed:
it would contain the user's unrelated notifications, exactly as issue 59 kept private.

## Automated results at this revision

| Gate | Command | Result |
| --- | --- | --- |
| format | `cargo fmt --all -- --check` | **passed**, exit 0 |
| lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **passed**, exit 0, 0 warnings |
| scoped: Pi reporting units | `cargo test -p ovrcr --lib report::` | **55 passed, 0 failed, 0 ignored**, 1.00 s |
| scoped: Pi lifecycle | `cargo test -p ovrcr --test server_lifecycle pi` | **12 passed, 0 failed, 1 ignored, 99 filtered out**, 15.19 s |
| opt-in installed Pi (real binary) | `OVRCR_TEST_PI_EXECUTABLE=/Users/xlyk/.local/bin/pi cargo test … --ignored --exact installed_pi_managed_launch_binds_the_real_session_and_stays_idle` | **1 passed**, 2.17 s (at low load; this test would have failed during the Defect 1 window) |
| opt-in installed Pi (via probe shim) | same with the fixture shim | **1 passed**, 1.95 s |
| workspace | `cargo test --workspace --all-targets --all-features --no-fail-fast` | **NOT RUN** — this dispatch was instructed to run scoped tests only under the shared `CARGO_TARGET_DIR`, with sibling worktree sessions building concurrently. No count is claimed. |
| doctests | `cargo test --workspace --doc` | **NOT RUN**, same reason |
| hosted checks | — | **OPEN GATE**: requires a push and a PR; delivery is Kyle's call |

Logs in [`native/automated/`](native/automated/).

## Fifty-session replay

**NOT RUN.** The runbook §5.1 proposal requires authoring a new `#[ignore]`d 50-session
acceptance test; nothing existing covers it (`fifty_sessions_survive_detach_and_leave_no_process_groups`
drives plain shell sessions, `history_memory_measurements` measures memory, neither drives the
Pi reporting path). Two reasons not to run it here:

1. The ticket's criterion says **isolated host**. This machine is a busy credentialed
   workstation that ranged from load 7 to load 90 during the window, with other agent sessions
   building and testing throughout. Its queue, memory high-water and control-latency numbers
   would not be evidence of anything — as Defects 1 and 2 demonstrate, load is the dominant
   variable on this host.
2. The runbook itself says that even if executed here it would close only the *mechanism* half
   and leave the *isolated host* half open.

The gate stays open with that reason recorded.

## Ownership and cleanup

| Check | Result |
| --- | --- |
| Launcher exit code | 0 on all three `ovrcr-gui` runs |
| Recorded PIDs | 25749, 25802, 26177, 25967, 25989, 26029, 26071, 26076, 26088, 26097, 26115, 26138, 26175, 16864, 31611, 96467 → all ESRCH |
| Recorded PGIDs | same set → all ESRCH |
| Fixture sockets | all `$TMPDIR/ovrcr-gui-*/server.sock` absent |
| Demo roots | all `$TMPDIR/ovrcr-gui-*` absent |
| Materialized extension dirs `$TMPDIR/ovrcr-pi-*` | absent (removed by `receiver.disable()`) |
| `pi-child` exit by `SIGTERM` | `phase: "exited"`, `exit_code: 0`, `health.reason: "supervisor_disconnected"`, historical Unread **retained** |
| `pi-root` graceful close | `{"ok": true}`, row removed |
| Ctrl-D exit | **NOT RUN** — no keystroke path, see above |
| `omp` daemons | Kyle's three were running before and after; none were started, signalled or inspected |
| `$ROOT` removed after evidence copied out | yes |

[`native/22-pi-cleanup.json`](native/22-pi-cleanup.json).

## Retained failed and unrun attempts

Nothing was deleted or relabelled.

- **P1 attempt 1** — picker launch at load ~88, reporting disabled by the failed probe:
  `05a-pi-probe-failure.txt`. Retained as the Defect 1 reproduction.
- **Model attempt 1** — `openai-codex/gpt-5.4-mini` refused by the provider:
  `14-pi-error.*`. Retained, and reused as the P9 Error evidence rather than discarded.
- **P6 attempt 1** — Busy window missed: header of `11-pi-continuation.json`.
- **P8 attempts 1 and 2** — cancellation never observed: `13-pi-cancel.json`,
  `13-pi-cancel.preclose.json`.
- **Turn 12** — `/tree` submitted as a prompt: `19-pi-transitions.json`.
- **NOT RUN**: `/tree`, reload, compaction, provider retry, visible-pane suppression,
  detach/reattach, tiny layout, Ctrl-D, the fifty-session replay, native Linux, hosted checks,
  workspace and doctest suites, documentation review.

## Deviations from the runbook

1. `OVRCR_CONFIG` / `OVRCR_SOCKET` cannot be set for the GUI (`src/gui.rs:256-257` overrides
   them). Recorded above; isolation was unaffected.
2. `native/version-probe-shim.sh` — during the Defect 1 window a shim named `pi`, placed first
   on the dashboard's `PATH`, answered the bare `--version` probe instantly and `exec`'d the
   real `/Users/xlyk/.local/bin/pi` for every other invocation, so the agent process, its PTY,
   its argv and its extension loading were the genuine Pi 0.85.1. It was needed only to get
   past the probe. **The final P1 in `05-pi-bound.json` used no shim**: the unmodified picker
   path launched `ovrcr agent run pi -- /Users/xlyk/.local/bin/pi` and bound cleanly.
3. `native/ovrcr-gate-dialog.mjs` was added with `-e` for P11, as the runbook allows. It
   reproduces `detect_agents`'s managed argv verbatim and only appends the helper extension.
   The unmodified picker path is the one proven in P1.
4. The P14 gap was **not** injected. It occurred on its own; `OVRCR_TEST_DROP_SEQUENCE` was
   never set.
5. Most steps after the GUI window stopped accepting synthesized keys were driven through
   `ovrcr terminal create` with the exact managed argv the picker builds, rather than through
   the picker. Stated per step above.

## Claims explicitly NOT made

- No Oh My Pi support claim of any kind. #97 was not run and #96 is open.
- No accounting, merge, release, push, PR or live installation. Delivery is Kyle's.
- No Linux native claim.
- No claim that a delivered host command proves a visible alert or an audible sound.
- No claim that the full workspace suite, the doctests or any hosted check passed at this
  revision: they were not executed here.
- No claim that `~/.pi` was left untouched. The directory's mtime moved; see above.
