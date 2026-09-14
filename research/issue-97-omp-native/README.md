# Issue 97: native Oh My Pi acceptance

Native macOS acceptance run for [#97](https://github.com/xlyk/ovrcr/issues/97), the Oh My Pi
status-feature delivery gate, following
`.superpowers/sdd/2026-09-14-pi-omp-phase5/native-gates-runbook.md` §§0–2, 3.0, 3.2 (O1–O16),
4, 5, 7, 8. §3.1 (Pi, #92) was not re-run; it is covered by
`research/issue-92-pi-native/` on `ticket/92-pi-native`.

**Read before treating anything below as a pass:** [Defects found](#defects-found),
[What could not be exercised, and why](#what-could-not-be-exercised-and-why),
[Retained failed attempts](#retained-failed-attempts) and
[The untouched check](#the-untouched-check). The turn budget was spent to its hard cap, and
four sub-criteria are open.

## Revisions

| Thing | Value |
| --- | --- |
| OVRCR source revision | `ee3cc05e852e00ccc7bc3a63805b77b1eb7d3f0d` (head of `feature/pi-omp-phase5`: merged main + #96 + the review-wording commit) |
| OVRCR built binaries | `ovrcr` `dd0b73418dc3ab9e516bd79857cb79a6abbdfbc99d05c70a8441e4a192c39c03`, `ovrcr-gui` `8f4b2bf3879b55354375145a225b999fab0aab8c7ec7e4c42334a8d266fdaf8d` |
| Application bundle | `target/OVRCR GUI.app`, assembled 2026-09-14T08:21:23Z (runbook §2 layout; `just gui` cannot be used under a shared `CARGO_TARGET_DIR`) |
| Provider executable | `/Users/xlyk/.local/bin/omp` (and `/Users/xlyk/.local/bin/pi` for the mixed-provider step) |
| Provider version (as printed) | `omp/18.1.19`; `pi` `0.85.1` |
| doctor `version_status` | `tested` (`tested_versions: ["18.1.19"]`, `supported_versions: "any compatible release; tested versions are evidence, not an allowlist"`) |
| Model | first `google-antigravity/gemini-3.5-flash-lite` (the only model the fixture auth lists as **free**) — its quota was exhausted on the first turn; then `openai-codex/gpt-5.3-codex-spark`, thinking `high`, the smallest model on a subscription rather than per-token billing |
| Fixture root | `$TMPDIR/ovrcr-native-gates.AlQ1oA` (removed after the evidence was copied out) |
| Host | macOS 26.5.2, arm64, 14 cores; load average 3.7–6.2 for the whole window |

`native/01-revision.txt`, `native/02-binaries.txt`, `native/00c-providers.txt`.

## Isolation

| Check | Result |
| --- | --- |
| Files copied into the fixture (names only) | `agent.db`, `agent.db-wal`, `agent.db-shm`, `models.db`, `models.db-wal`, `models.db-shm` from `~/.omp/agent/`, and `~/.omp/natives/`. Nothing else. No Pi credential file was copied — only the Oh My Pi files of runbook §1.3 were authorised. |
| Were the copies read? | No. They were `cp`-ed and never opened; not even `sqlite3 ".tables"` was run. Authentication was confirmed indirectly, by `omp models` listing five authenticated providers. |
| `HOME` during the run | `$ROOT/home` — confirmed from inside an OVRCR terminal (`native/04-launch-shape.txt`) |
| `PI_CODING_AGENT_DIR` | never exported |
| `find ~/.pi -newer marker-before` | one path: the directory `~/.pi/agent` itself — see [The untouched check](#the-untouched-check) |
| `find ~/.omp -newer marker-before` | 13 files under `~/.omp/logs/`, all written by Kyle's own live `omp` processes — see below |
| Fixture received the writes (`native/98-fixture-writes.txt`) | yes, 96 paths under `$ROOT/home/.omp` and `$ROOT/home/.pi` |
| Other `omp` instances alive during the window | three of Kyle's own (pids 24141, 65178, 85766), running before and after; never signalled, inspected or read |
| Other `pi` instances alive | none at the start (`pgrep -l '^pi$'` empty) and none at the end |
| Live provider settings modified | none. Oh My Pi's first-run setup and the `/model` and Tool-Approval changes all wrote to `$ROOT/home/.omp/config.yml`. |

`OVRCR_CONFIG` and `OVRCR_SOCKET` could **not** be set by the operator: `src/gui.rs:237-261`
makes `Demo` create its own disposable root and override both. The run was still isolated —
the dashboard used `$TMPDIR/ovrcr-gui-qZCSb0/{config.toml,server.sock}`, never the live ones —
but at a path the GUI chose. `OVRCR_DASHBOARD_CONFIG` is inherited and did take effect
(both alert channels fired). This repeats the deviation issue 92 recorded; the runbook §2
launch block is still wrong about it.

### The untouched check

**`~/.omp` — clean, and the hits are attributable.** The 13 files are all under `~/.omp/logs/`
and none of their PIDs belongs to a process this run started. Every `omp` this run started
logged into the fixture instead: `$ROOT/home/.omp/logs/` holds
`omp.2026-09-14.{53277,53324,84006,84111}.log`, exactly the managed launches recorded in
`native/04-launch-shape.txt` and `native/23-omp-mixed.json`. Nothing under `~/.omp/agent`,
`~/.omp/config.yml`, `~/.omp/sessions` or `~/.omp/run` changed.

**`~/.pi` — one directory mtime moved, and it was not an operator mistake this time.**
`find ~/.pi/agent -mindepth 1 -newer marker-before` is empty: nothing inside was created,
modified or deleted, and every entry still carries a pre-run mtime (`auth.json` and
`models-store.json` 2026-09-09T17:15:30-0700, `trust.json` 2026-09-01, `settings.json`,
`git/`, `skills/`, `extensions/`, `npm/`, `sessions/` all older than the marker). Only the
directory's own mtime moved, to 2026-09-14T01:59:22-0700 — the moment the managed `pi-1`
terminal was launched — which means a file was created and removed inside it.

Unlike issue 92's run, **every** `pi` invocation here had `HOME=$ROOT/home`: the `pi --version`
in `native/02-binaries.txt` and the managed `pi-1` launch, whose own state landed entirely in
the fixture (`$ROOT/home/.pi/agent/{auth.json,models-store.json,settings.json,bin,sessions}`,
with a 2-byte `auth.json` Pi created empty). So the most consistent reading is that **Pi 0.85.1
touches the real `~/.pi/agent` at startup even under a different `HOME`** — a provider
behaviour, not an OVRCR one, and not something OVRCR can prevent. It is a correlation, not a
proof: `pgrep '^pi$'` was empty at the start and end of the run but was not sampled
continuously. Recorded, not absorbed. Full detail in [`native/99-untouched.txt`](native/99-untouched.txt).

## Turn ledger

Budget: 13 real Oh My Pi turns, hard cap 15. **15 were spent.** Retries and failed attempts are
counted and retained, not hidden. Two further model calls were made by children of turn 11 (an
in-process task subagent and an external `omp -p` descendant); they are not root turns and are
listed separately.

| # | Step | Prompt (verbatim) | Outcome | Artifact |
| --- | --- | --- | --- | --- |
| 1 | O2 att. 1 | `Reply with the single word: DELTA` | **Failed attempt, retained.** `google-antigravity` returned HTTP 429 `RESOURCE_EXHAUSTED`; Oh My Pi retried once and gave up. OVRCR stayed **Busy across the retry** and settled at **Error**: no Ready, no Unread. A real provider failure, so none was invented — reused as the O6 failure evidence. | `08a-omp-failure-quota.json` |
| 2 | O2 | `Reply with the single word: DELTA` | Busy → ResponseReady, quality **Observed**, unread turn `…:2` | `08-omp-ready-1.json`, `.jpg` |
| 3 | O3 | `Reply with the single word: ECHO` | ResponseReady, unread turn `…:3`; both replies distinct on screen | `09-omp-ready-2.json`, `.jpg` |
| 4 | O5 | `Count slowly from 1 to 20, one number per line.` | Busy on cycle `…:4`; the earlier unread `…:3` **survived the new Busy** | `11-omp-continuation.json` |
| 5 | O5 | `Then reply: CHARLIE` | Queued while Busy. One cycle for two prompts: state stayed Busy on `…:4`, then exactly one ResponseReady and exactly one new Unread (`arev` 8 → 9). Screen shows 1…20, then `CHARLIE`. | `11-omp-continuation.json` |
| 6 | O6 att. 1 | `Count slowly from 1 to 200, one number per line.` | **Failed attempt, retained.** The 200-line response finished inside one computer-use round trip, so Escape arrived after the cycle settled. Produced a normal Ready `…:5`. | `13-omp-cancel.json` |
| 7 | O8 + O6 | `Use the bash tool to run: echo OVRCR_APPROVAL_OK; sleep 60` | Approval request `approval:call_7Uow…\|fc_036b…` opened, `activity == waiting_input` with the underlying activity still Busy; **Approve** → request closed, activity restored to **Busy** (not Idle, not Ready), `OVRCR_APPROVAL_OK` printed by the real bash tool. Escape during the 60 s execution → **Idle, no Ready, no new Unread**, earlier unread `…:5` preserved. | `14-omp-approval-allow.json`, `.jpg`, `13b-omp-interrupt.json` |
| 8 | O9 | `Use the bash tool to run: echo OVRCR_APPROVAL_OK` | Approval `approval:call_Xiw9…` opened; rejected through the dialog's documented `esc cancel` path → request closed, no Error, no Unread, earlier unread preserved. Oh My Pi retried the tool, opening a **new, distinctly identified** approval `approval:call_LU77…`; approved → Ready `…:7`. | `15-omp-approval-deny.json`, `.jpg` |
| 9 | O10 | `Use the ask tool to ask me: pick a colour, red or blue.` | `question:call_kZIA…\|fc_05b5…`, kind **Select**, real rich dialog (Red / Blue / Other). Answered → request closed, Ready `…:8`. | `16-omp-question.json`, `.jpg` |
| 10 | O11 | `In one message make two tool calls at the same time: the ask tool asking me to pick a colour, red or blue, and the bash tool running: echo NESTED_OK` | Oh My Pi issued the ask first. Answering it did **not** end the wait: the very next sample (18 ms) already showed the bash approval open and `activity` still `waiting_input`, with no Busy in between — closing one request did not clear the other active wait. Approved → Ready `…:9`, screen shows `Red` and `NESTED_OK`. Two ids open **simultaneously** were never observed. | `17-omp-nested.json` |
| 11 | O7 | `Use the task tool to ask a subagent to reply with the single word FOXTROT. Then use the bash tool to run: omp -p "Reply with the single word GOLF"` | In-process task subagent `FoxtrotResponder` ran to completion and the external `omp -p` descendant answered `GOLF`; the root produced exactly **one** cycle `…:10` with one Ready, binding, generation and conversation unchanged, and neither child created a binding or a terminal of its own. | `19-omp-children.json` |
| 12 | O14 gap att. 1 | `Reply with the single word: HOTEL` | **Failed attempt, retained** as a gap injection (the helper-kill loop lost the race against a ~10 ms helper). Produced a normal Ready `…:11`, which also confirms normal reporting after `/reload`, `/tree`, `/fork`, `/new` and `/resume`. | `21-omp-gap.json` |
| 13 | O14 | `Reply with the single word: INDIA` | Post-reattach confirmation: Busy → ResponseReady on cycle `…:12` at generation 8, health `Connected` throughout. | `22-omp-reattach.json` |
| 14 | O16 / O12 | `Reply with the single word: JULIET` (on `omp-2`) | `omp-2` was **not** the visible pane: Ready → **one** `afplay Glass.aiff`. `omp-1`'s unread and activity untouched. | `23-omp-mixed.json`, `18-omp-alerts.txt` |
| 15 | O12 | `Use the ask tool to ask me: pick a colour, red or blue.` (on `omp-2`) | Input request opened while not visible → **one** desktop notification `consigint / auth-handoff / omp-2 (#12)` titled `OVRCR · input needed`, plus one Glass sound. Dialog then dismissed → request closed, activity restored to Idle, **no fabricated Unread**, earlier unread `…:1` unchanged. | `24-omp-alerts.json`, `.jpg`, `18-omp-alerts.txt` |
| | **Total real Oh My Pi turns** | | **15 / cap 15 (budget 13)** | |
| | Child model calls (not root turns) | | 1 in-process task subagent, 1 external `omp -p` descendant, both from turn 11 | `19-omp-children.json` |

Free steps that spent no turn: O1, O4 (stale acknowledgement and Presented review), the whole of
O13 (`/reload`, `/tree`, `/fork`, `/new`, `/resume`), the `chmod`-based gap injection, both
`/ovrcr-reattach` runs, O15 and every snapshot, doctor and screenshot.

## Acceptance matrix

Criteria verbatim from the ticket.

| Criterion | Evidence | Result |
| --- | --- | --- |
| An isolated native macOS run launches OMP from the actual Dashboard picker and proves two distinct root responses, Busy/Observed Ready, Unread/Presented review, stale acknowledgement rejection and an earlier Unread surviving new Busy. | `05-omp-picker.jpg`, `06-omp-bound.json`, `07-omp-doctor.json`, `04-launch-shape.txt`, `08-*`, `09-*`, `10-omp-stale-ack.json`, `12-omp-review.json`, `11-omp-continuation.json` | **passed** |
| Native stop-hook continuation, retry/compaction, async continuation, failure, interruption, in-process task/advisor exclusion and external descendant exclusion behave as specified. OMP Ready never claims guaranteed finality. | `11-omp-continuation.json`, `08a-omp-failure-quota.json`, `13b-omp-interrupt.json`, `19-omp-children.json` | **partly open** — async continuation (one cycle for a queued follow-up), provider retry (Busy held across Oh My Pi's own retry), failure→Error, interruption→Idle, in-process task-child exclusion and external-descendant exclusion all passed, and every Ready in the run carried `quality: Observed`, never `Confirmed`. **Registered stop-hook OPEN**, **compaction NOT RUN**, **`--advisor` NOT RUN** (see below). |
| Question waiting from the ask tool's lifetime is demonstrated at real rich and fallback dialogs, along with actual approval allow/deny; the documented tool-lifetime approximation is shown and not overstated. Nested requests, timeout and closure produce WaitingInput as documented and no fabricated Unread. | `14-omp-approval-allow.json`/`.jpg`, `15-omp-approval-deny.json`/`.jpg`, `16-omp-question.json`/`.jpg`, `17-omp-nested.json`, `24-omp-alerts.json` | **partly open** — real rich ask dialog with `question:<toolCallId>` / kind `Select`, real approval allow, rejection through the dialog's `esc cancel`, request closure on dismissal with no fabricated Unread, and an inner closure that did not clear the outer wait, all passed. **"Fallback dialog" OPEN** (undefined and unreachable from the TUI). **Explicit `Deny` row OPEN.** **Two ids open at once NOT OBSERVED.** The tool-lifetime ordering is **not claimed**: see below. |
| Enabled native Response ready and Input needed sound/desktop outcomes are visibly/audibly verified as applicable, including privacy-safe wording, independent channel preferences, visibility suppression, request closure, deduplication and reconnect baselines. Capture fresh tiny/normal Dashboard screenshots/accessibility. | `18-omp-alerts.txt`, `18-alert-dispatch.log`, `18a-alert-dispatch-sound-only.log`, `24-omp-input-needed.jpg`, `25-dashboard-normal-layout.jpg`, `26-dashboard-tiny-layout.jpg`, `26-dashboard-layouts.ax.txt` | **partly open** — visibility suppression is proven by contrast (nine Readys and eight input requests on the visible `omp-1` produced **zero** alerts; one Ready and one input request on the non-visible `omp-2` produced exactly one alert each), the Input-needed body is identity-only, and both layouts are captured. **Response-ready notification title NOT CAPTURED** (sound only), **reconnect baseline OPEN**, **actual display/audibility OPEN** (the ticket's own caveat). |
| Native new/resume/fork/tree/in-place reload, bounded reporter reattachment during work/input waits, old-event rejection, detach/reattach, output/exit/signals and owned cleanup are proven without native restart or historical attention replay. | `20-omp-transitions.json`, `20a-plugin-refresh-identity.txt`, `21-omp-gap.json`/`.jpg`, `22-omp-reattach.json`, `27-omp-cleanup.json` | **partly open** — `/fork`, `/new`, `/resume` (including A→B→A reuse), `/tree` and the plugin-resource refresh all behaved exactly as #96 designed; a forced source gap paused as `paused_recoverable` and `/ovrcr-reattach` recovered it in place with no alert replay and no resurrected Ready; exit codes, signals and cleanup passed. **Reattachment during an open input wait OPEN**, **same-file session reload NOT REACHABLE**, **detach/reattach OPEN**. |
| Native Linux OMP provider lifecycle and platform PTY/socket/process behavior are verified separately… | — | **OPEN GATE**: no Linux host is reachable from this machine. macOS UI evidence and automated Linux fixtures are not a substitute. |
| An isolated-host fifty-session mixed-provider replay runs through real sockets with Pi/OMP and existing provider observations… | `23-omp-mixed.json` | **OPEN GATE / NOT RUN** — see [Fifty-session replay](#fifty-session-replay). The mixed-provider half is **partly open**: two independent Oh My Pi bindings and one Pi binding coexisted with distinct invocations, conversations and per-terminal Unread and alerts, and `doctor` reported each provider's own capabilities. Real Claude and Codex turns were **blocked** (only Oh My Pi credentials were authorised for the fixture home). |
| Current-revision workspace/lint/format/wire-compatibility and required hosted checks pass with executed counts… | `native/automated/` | **partly open** — format, lint, the Oh My Pi reporting units, the scoped Oh My Pi lifecycle suite, the opt-in installed-OMP test and the pinned wire snapshot all passed with counts. The **full workspace suite and doctests were NOT RUN** (this dispatch was instructed to run scoped tests only under a shared `CARGO_TARGET_DIR`). **Hosted checks OPEN** (require a push). |
| Support/setup/removal/doctor/recovery documentation describes actual capability delivery, optimistic upgrades, the tool-lifetime question surface, Observed finality, local wait-surface limits, platform evidence and absent metrics. | `07-omp-doctor.json`, `21-omp-gap.json` | **defect** — `doctor` itself is correct and complete (see below), but the CLI help for `terminal mark-reviewed` still says "Codex". Documentation review was otherwise out of scope for this dispatch. See [Defect 1](#defect-1--the-cli-still-calls-mark-reviewed-a-codex-only-action). |
| Record native/application/source revisions, commands, outcomes, counts and fixture PID/PGID ownership; remove only owned test resources after verifying cleanup. Full-support claims depend on demonstrated behavior. | this file, `27-omp-cleanup.json`, `99-untouched.txt` | **passed** |

## What passed, in detail

### O1 — the actual Dashboard picker

`n` in Browse mode → Agent field filtered to `omp` → Workspace `consigint / auth-handoff` →
Name `omp-1` → submit. The launch shape is the unmodified managed one
(`native/04-launch-shape.txt`):

```
53276 53276 43759  …/OVRCR GUI.app/Contents/MacOS/ovrcr agent run omp -- /Users/xlyk/.local/bin/omp
53324 53324 53276  /Users/xlyk/.local/bin/omp -e $TMPDIR/ovrcr-omp-cbaef99…/ovrcr-omp-reporting.mjs
```

The binding came up immediately: `provider: "Omp"`, non-empty conversation, `generation: 1`,
`activity.state: "Idle"`, `quality: "Observed"`, `health: Connected`. `doctor` reported
`version_status: "tested"`, `session_status: "bound"`, `extension: "loaded_and_bound"`,
`questions: "available_tool_lifetime"` and — the #96 change — **`recovery: "available"`**.

### O4 — review

The stale acknowledgement was refused with
`{"error":{"code":"Conflict","message":"Ready observation changed; inspect the unread response before reviewing it"}}`
and the newer unread survived. Acknowledging the presented unread returned `{"ok":true}` and
set `unread` to `null` with `activity` unchanged at `ResponseReady`; repeating the same
acknowledgement was harmless.

### O13 — transitions

| Command | Generation | Conversation | Activity | Cycle |
| --- | --- | --- | --- | --- |
| `/reload` (Oh My Pi's plugin-resource refresh — it prints "Plugins reloaded.") | 1, unchanged | unchanged | unchanged | unchanged; the whole agent summary was **byte-identical** before and after (`20a-plugin-refresh-identity.txt`) |
| `/tree` + search + Enter ("Navigated to selected point") | 1, **unchanged** | unchanged | → `Idle` | **invalidated** (`turn: null`); the server-owned Unread was retained and no historical Ready was manufactured |
| `/fork` | 1 → **2** | `01a09f05…` → `01a09f20-3225…` | `Idle` | reset, `arev` back to 1 |
| `/new` | 2 → **3** | → `01a09f20-6247…` | `Idle` | reset |
| `/resume <A>` | 3 → **4** | → `01a09f05…` (**A→B→A reuse**) | `Idle` | reset; health stayed `Connected`, i.e. no `transition_mismatch` |

The invocation id never changed across any of them, and cycle identities never repeated: after
the transitions the next cycles were `…:11` and `…:12` on the same instance prefix, exactly as
#96 specified (cycles are numbered per extension instance, never per conversation).

### O14 — gap and reattachment

The gap was **not** injected with a product test hook. `OVRCR_TEST_DROP_SEQUENCE` was never set
anywhere in this run. Two host-level attempts failed (killing the ~10 ms helper is a race);
the third worked: `chmod 000` on the supervising `ovrcr` binary that
`src/ovrcr-reporting-transport.mjs` spawns as its helper, so exactly one frame (a `/fork`
`session_start`) could not be delivered while `producer.sequence` had already advanced, then
the permission was restored and `/new` landed with the hole.

Result, exactly as designed: `health.state: "Unavailable"`, `health.reason: "source_gap"`,
`lifecycle.delivery: "paused_recoverable"`, activity frozen, the native session untouched, and
one remediation line:

> Reporting is paused for this invocation (source_gap): … Run /ovrcr-reattach in Oh My Pi, or
> send the next prompt — the next session boundary recovers reporting as a fresh generation.

`/ovrcr-reattach` printed "OVRCR reporting reattached" inside Oh My Pi, took generation 6 → 7,
restored `Connected`, published `Idle` from Oh My Pi's own `isIdle()` (never a resurrected
Ready), preserved the earlier server-owned Unread and replayed **no** alert. A second
`/ovrcr-reattach` on an already-healthy reporter was equally harmless (generation 7 → 8, still
Connected, still Idle, no alert). The next prompt then reported normally.

### O15 — exit, signals, cleanup

`omp-2` was killed through the supervisor while holding an Unread: `phase: "exited"`,
`exit_code: 143`, `health.reason: "capability_revoked"`, and the **historical Unread was
retained**; `doctor` moved it to `delivery: "lost"` with a "start a fresh managed launch"
remediation, and its materialized extension directory was removed on exit. `omp-1` and `pi-1`
closed gracefully (`{"ok":true}`) and their rows were removed. Closing the GUI window removed
the whole disposable demo root, which `Demo::shutdown` only does after the socket is gone.

## Defects found

Neither was fixed; both are recorded for separate dispatch.

### Defect 1 — the CLI still calls mark-reviewed a Codex-only action

`src/cli/args.rs:189` documents the subcommand as

```
/// Mark one observed Codex response reviewed. Changes unread only.
```

and `ovrcr terminal --help` prints it. The behaviour is already provider-neutral — this run
acknowledged an **Oh My Pi** response through it twice, and #88's decision says explicit review
"targets the Presented Unread regardless of provider" through the shared supported-readiness
predicate. Only the user-facing string is stale, so a user reading the help would conclude the
feature does not apply to Oh My Pi. Severity: documentation, user-visible.
Evidence: `native/12-omp-review.json`, `native/10-omp-stale-ack.json`.

### Defect 2 — Pi 0.85.1 touches the real `~/.pi/agent` under a foreign `HOME`

Not an OVRCR defect, and recorded here only because it is the second run in which this gate's
isolation check has flagged it. Issue 92 attributed its identical finding to operator error; in
this run every `pi` invocation used `HOME=$ROOT/home` and Pi's own state landed entirely in the
fixture, yet `~/.pi/agent`'s directory mtime still moved at the moment the managed `pi-1`
terminal started. Nothing inside changed. See
[The untouched check](#the-untouched-check) for the full attribution and the caveat that this
is a correlation, not a proof.

### Not a defect — the load-related findings from issue 92 did not recur

The 1 s `--version` admission probe and the 900 ms helper deadline both behaved normally
throughout. Load average stayed between 3.7 and 6.2 for the entire window
(`native/automated/00-load.txt` and the `uptime` line at the head of every turn's artifact),
and no spontaneous `source_gap` occurred: the only gap in this run had to be forced, and it
took three attempts. That is consistent with issue 92's conclusion that both are load-dependent,
and is recorded as new information at low load rather than as a contradiction.

## What could not be exercised, and why

- **"Fallback dialog" (AC3).** Oh My Pi 18.1.19 draws one rich select dialog for the ask tool
  and one rich Approve/Deny dialog for approvals; no fallback renderer is reachable from the
  TUI and the rich/fallback distinction is not defined in this repo's docs. **OPEN**, with the
  reason, per the dispatch instruction not to contrive one.
- **The explicit `Deny` row (AC3).** The dialog's cursor cannot be moved from this harness: the
  only key combos available in background computer use are Return, Escape, Backspace, Delete and
  Cmd-A; a bracketed paste of `ESC [ A` through `ovrcr terminal send` is swallowed as literal
  text; the mouse wheel is forwarded to some Oh My Pi overlays but not to the approval dialog;
  and a click never reaches the PTY. The rejection was therefore delivered through the dialog's
  own documented `esc cancel` path, which did close the request. **OPEN** as an explicit
  `Deny` selection.
- **Two input-request ids open simultaneously (AC3).** Oh My Pi resolved the ask before the
  approval opened. What *was* shown is the criterion that matters — the wait never lapsed
  between them, so closing the inner request did not clear the outer one — but the
  "assert both ids present, oldest first" assertion was **NOT OBSERVED**.
- **The tool-lifetime ordering (AC3).** The runbook asks for a snapshot in the window between
  `tool_execution_start` and the dialog being drawn. The poller needs two CLI round trips
  (~200 ms) to read the request set and the screen, which is coarser than that window, and in
  one sample the ordering even appeared inverted purely because the screen was read after the
  request list. **No dialog-level visibility is claimed and no sub-frame ordering is asserted.**
- **A registered stop-hook continuation (AC2).** `omp --hook` is an alias for `--extension`
  (`strings`: `oui = new Set(["--extension", "-e", "--hook", "--trusted-extension"])`) and no
  extension event lets a handler request continuation; `willContinue` is set by Oh My Pi's own
  loop. The continuation actually exercised is the queued-steering one, which is a genuine
  `willContinue: true` path in the shipped binary (`strings` @791847,
  `D4e(…, { willContinue: !EN(s.deadline) })`). Recorded as a deviation, not as the stop hook.
- **Compaction (AC2) and `--advisor` (AC2).** Both need turns the cap did not leave.
  **NOT RUN.**
- **Reattachment during an open input wait (AC5).** `/ovrcr-reattach` has to be typed into the
  composer, and an open dialog owns the input. There is no other way to invoke it from this
  harness. **OPEN.**
- **Same-file session reload (AC5).** `/reload` in 18.1.19 is the plugin-resource refresh
  ("Plugins reloaded."); `ctx.reload()` has no slash command. **NOT REACHABLE** at this version.
- **Detach/reattach and the reconnect baseline (AC4, AC5).** `q` detaches the dashboard client,
  and the GUI helper has no path to attach a new one afterwards; this agent's shell has no TTY
  to run one from either. Not attempted, so as not to end the run without a dashboard. **OPEN.**
- **The Response-ready notification title.** The alert sampler's first pattern matched only
  `afplay`, so turn 14's Ready is evidenced by its Glass sound; the `osascript` argv was
  captured only for the Input-needed alert (`OVRCR · input needed`, body
  `consigint / auth-handoff / omp-2 (#12)`). The two titles differ in
  `crates/ovrcr-tui/src/dashboard/desktop.rs`, but this run captured only one of them.
  **OPEN.**
- **Real Claude and Codex turns for the mixed-provider step (AC7).** Only Oh My Pi credentials
  were authorised for the fixture home, so neither CLI can authenticate there; the installed
  `claude` is 2.1.270, outside the accepted 2.1.267/2.1.268 admission range in any case.
  **BLOCKED.**
- **Actual alert display and audibility (AC4).** Dispatch was observed by sampling for the
  `osascript`/`afplay` children with their argv. A successful host command does not prove a
  banner was drawn or a sound was heard: Focus/Do Not Disturb, notification permission and
  output volume decide that. Same caveat as issues 59 and 92. **OPEN.**
- **Accessibility rows.** This helper publishes the whole dashboard as one `AXTextArea` named
  "OVRCR terminal" and withholds row text, so AX cannot assert rendered rows. Text assertions
  come from `ovrcr terminal read --json` (the server's own rendered screen) and visual
  assertions from the screencaptures. Recorded in
  [`native/26-dashboard-layouts.ax.txt`](native/26-dashboard-layouts.ax.txt).

## Alerts

Preferences for the run (`$ROOT/dashboard.toml`): `desktop_notifications = true`,
`ready_sound = true`.

```
2026-09-14T08:59:43Z  88585 afplay /System/Library/Sounds/Glass.aiff                     # omp-2 Ready
2026-09-14T09:00:16Z  92620 osascript -e … -- consigint / auth-handoff / omp-2 (#12) OVRCR · input needed
2026-09-14T09:00:16Z  92647 afplay /System/Library/Sounds/Glass.aiff
```

That is the complete list. Everything else in the run happened in the visible pane and produced
nothing: while `omp-1` was the pane the dashboard showed, it produced **nine** Readys and **eight**
input requests and the sampler stayed empty. The notification body carries only project /
workspace / terminal name and id — no response text, prompt text, model name or path. One alert
per logical request; no repeat alert while a request stayed open; no alert replayed by either
`/ovrcr-reattach`. No Notification Center capture is committed: it would contain the user's
unrelated notifications, exactly as issue 59 kept private. `native/18-omp-alerts.txt`.

## Automated results at this revision

| Gate | Command | Result |
| --- | --- | --- |
| format | `cargo fmt --all -- --check` | **passed**, exit 0 |
| lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **passed**, exit 0, 0 warnings |
| scoped: reporting units | `cargo test -p ovrcr --lib report::` | **55 passed, 0 failed, 0 ignored**, 1.01 s |
| scoped: Oh My Pi lifecycle | `cargo test -p ovrcr --test server_lifecycle omp` | **12 passed, 0 failed, 1 ignored, 99 filtered out**, 15.17 s |
| opt-in installed OMP (real binary) | `OVRCR_TEST_OMP_EXECUTABLE=/Users/xlyk/.local/bin/omp cargo test -p ovrcr --test server_lifecycle -- --ignored --exact installed_omp_managed_launch_binds_the_real_session_and_stays_idle` | **1 passed**, 6.21 s |
| wire compatibility | `cargo test -p ovrcr-protocol` | **33 passed, 0 failed**, 0.01 s |
| workspace | `cargo test --workspace --all-targets --all-features --no-fail-fast` | **NOT RUN** — this dispatch was instructed to run scoped tests only under the shared `CARGO_TARGET_DIR`. No count is claimed. |
| doctests | `cargo test --workspace --doc` | **NOT RUN**, same reason |
| hosted checks | — | **OPEN GATE**: requires a push and a PR; delivery is Kyle's call |

Logs in [`native/automated/`](native/automated/). All were run at `ee3cc05` in
`.worktrees/ticket-97-omp-native` with `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target
CARGO_INCREMENTAL=0`, at load average 5.3–5.6.

## Fifty-session replay

**NOT RUN**, for the two reasons the runbook §5.1 already states: the criterion says *isolated
host*, and this is a busy credentialed workstation; and even executed here it would close only
the mechanism half and leave the isolated-host half open. Writing the `#[ignore]`d 50-session
acceptance test the runbook proposes was also outside this verification dispatch, which was told
not to change product or test code. The gate stays open with that reason recorded.

## Ownership and cleanup

| Check | Result |
| --- | --- |
| Launcher exit code | **not captured** — the helper was started with `nohup … &` from a shell that this agent does not keep, so no wait status was collected. The launcher log is empty (no errors) and the disposable demo root was removed, which `Demo::shutdown` only reaches after the socket is gone, so the shutdown path completed. |
| Recorded PIDs | 43734, 53276, 53324, 83998, 84111, 84007, 43759, 90876, 308 → all **ESRCH** at 2026-09-14T09:05:23Z |
| Recorded PGIDs | 43731, 53276, 83998, 84111, 84007, 90870 → all **ESRCH** at the same instant |
| Fixture socket / demo root `$TMPDIR/ovrcr-gui-qZCSb0` | **absent** |
| Materialized extension dirs `$TMPDIR/ovrcr-{omp,pi}-*` | **absent** |
| `omp ps` inside the fixture | **no processes**; `$ROOT/home/.omp/run/` holds no live daemon |
| Kyle's own `omp` daemons | three were running before and after; none was started, signalled or inspected |
| `$ROOT` removed after the evidence was copied out | yes, 2026-09-14T09:13:43Z, after the evidence was copied into `research/issue-97-omp-native/native/` and committed |

[`native/27-omp-cleanup.json`](native/27-omp-cleanup.json).

## Retained failed attempts

Nothing was deleted or relabelled.

- **Turn 1** — the free `gemini-3.5-flash-lite` quota was exhausted (HTTP 429):
  `08a-omp-failure-quota.json`. Retained, and reused as the O6 failure evidence rather than
  discarded.
- **Turn 6** — interruption attempt 1, the response finished before Escape arrived:
  `13-omp-cancel.json`.
- **Turn 12 and two free attempts** — gap injection attempts 1 and 2 (kill the helper in a
  loop) lost the race against a ~10 ms helper: header and body of `21-omp-gap.json`.
- **O11** — the model would not issue the ask and the bash call in parallel, so two ids open at
  once was never observed: `17-omp-nested.json`.
- **NOT RUN / OPEN**: the fallback dialog, the explicit `Deny` row, a registered stop hook,
  compaction, `--advisor`, reattachment during an open wait, same-file session reload,
  detach/reattach, the reconnect baseline, the Response-ready notification title, real
  Claude/Codex mixed-provider turns, the fifty-session replay, native Linux, hosted checks, the
  full workspace suite and the doctests, and documentation review.

## Deviations from the runbook

1. `OVRCR_CONFIG` / `OVRCR_SOCKET` cannot be set for the GUI (`src/gui.rs:237-261` overrides
   them). Recorded above; isolation was unaffected. Same deviation issue 92 recorded.
2. **Launch flags.** The runbook asks for `--approval-mode always-ask --model <smallest>`, but
   AC1 requires the *actual picker*, which offers no argv. The picker launched bare `omp`; the
   model and the approval mode were then set inside that session through Oh My Pi's own
   first-run setup, `/model` and `/settings` → Tool Approval → **Always ask**, all writing to
   the fixture `config.yml` only. `--no-extensions` was never passed.
3. **The continuation is a queued steering continuation**, not a registered stop hook. See
   [What could not be exercised](#what-could-not-be-exercised-and-why).
4. **The O8 prompt has `; sleep 60` appended** to the runbook's verbatim
   `Use the bash tool to run: echo OVRCR_APPROVAL_OK`, so that the approved tool execution
   gives a Busy window long enough to interrupt. The approval evidence is unaffected —
   `OVRCR_APPROVAL_OK` was printed by the real bash tool. The O9 prompt is verbatim.
5. **O16's second Oh My Pi terminal and the Pi terminal were created with
   `ovrcr terminal create` using the picker's exact managed argv**, because the picker cannot
   be re-opened once the dashboard is in terminal mode on the first session. The unmodified
   picker path is the one proven in O1.
6. **The gap was forced with `chmod 000` on the helper binary**, not with
   `OVRCR_TEST_DROP_SEQUENCE`, which was never set anywhere in this run.
7. **Browse mode was reached by wheel-up over a pane (which opens History) followed by
   Escape**, because `Ctrl-g` cannot be synthesized from this harness. This is a harness
   workaround, not a product path.
8. Screenshots are committed as JPEG rather than PNG to keep the evidence commit small.

## Claims explicitly NOT made

- No claim that Oh My Pi Ready is a finality claim. Every Ready in this run carried
  `quality: "Observed"`, and Oh My Pi's stop hook may still continue a run after one.
- No claim of dialog-level question visibility, and no claim about the sub-frame ordering
  between `tool_execution_start` and the dialog being drawn.
- No claim that a delivered host command proves a visible alert or an audible sound.
- No Linux native claim.
- No claim that the full workspace suite, the doctests or any hosted check passed at this
  revision: they were not executed here.
- No claim that the fifty-session mixed-provider replay ran, or that real Claude or Codex turns
  were exercised.
- No accounting, merge, release, push, PR or live installation. Delivery is Kyle's.
