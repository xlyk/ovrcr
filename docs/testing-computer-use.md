# How to test OVRCR with computer use

Use this macOS smoke check after changes to the dashboard, terminal input, or CLI session controls. Run it in the checkout being reviewed. It complements the automated tests.

## 1. Launch and connect

```sh
rtk proxy just gui
```

Keep the command session open to collect its exit status and cleanup errors. The helper builds `target/OVRCR GUI.app` and creates a disposable server with two project roots and four feature workspaces: two initial root shells, four feature shells, and six fixture agents (twelve sessions). Each root is on `main` with a root marker. Feature workspaces are labeled by branches such as `gui-consigint-auth-handoff`.

With the Codex computer-use tool, initialize in a separate call using the actual checkout path:

```js
var app = await cua.getApp("/absolute/checkout/target/OVRCR GUI.app");
```

Read the returned tool documentation before continuing. Then inspect the window:

```js
await app.getAXStateAndScreenshot();
```

Use fresh accessibility indices or coordinates from the current screenshot. Sidebar row accessibility bounds can span the terminal: click within the visible sidebar or use `j`/`k`. A click that activates the window only focuses it and never reaches the dashboard; click again to act. Request the needed execution permission if local socket, PTY, or GUI access is sandbox-blocked; record a blocked check if access remains unavailable.

### Driving the window

Use Codex single-shot with the model configured in `~/.codex/config.toml`;
do not pass `-m`:

```sh
env -u OVRCR_AGENT_SOCKET -u OVRCR_AGENT_TOKEN codex exec -s workspace-write --skip-git-repo-check - < prompt.md
```

On 2026-10-02 (codex-cli 0.155.1, `codex login status`: "Logged in using
ChatGPT", `CODEX_HOME` unset, model `gpt-6.1-sol`) every call failed with
"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT
account", with or without the `OVRCR_*` variables
(`research/quota-display-2026-10-02/codex-cua-attempt.txt`). No working Codex
invocation is recorded yet. Until one is, record that failure and fall back to
the scripted System Events harness in `research/quota-display-2026-10-02`:
`drive.py` sends real key events to the helper process by PID and checks focus in
the same AppleScript call as each key, so a key never lands in another
fixture's window; `winid.swift` finds the window for `screencapture -l`, and
the helper's accessibility static text is saved beside each screenshot. When
another worktree drives its own fixture at the same time, run each step only
after its driver has gone idle (`research/settings-editor-2026-10-02/run-step.sh`),
so neither run steals focus mid-step. The
helper's window cannot be shorter than 22 rows. To check layouts shorter than
that, run a second isolated `ovrcr` Dashboard in a fixture shell after
`stty rows N` (`nested-ladder.sh`).

## 2. Prove real input and rendering

Select `consigint / gui-consigint-auth-handoff / local`, then press Return to enter terminal mode. Paste this command through the GUI and press Return:

```sh
printf 'CUA_%s\n' INPUT_OK
```

Confirm a separate output line containing `CUA_INPUT_OK`; the echoed command alone is insufficient. Inspect both the screenshot and accessibility text for readable rows, correct selection, and overlapping or clipped content. Resize the window and check the layout again. `Ctrl-g` returns to browse mode; `j`/`k` select sessions and Return resumes terminal input.

Check Unicode in the same shell:

```sh
printf '\033[31mRED 界🙂 END\033[0m\n'
```

Confirm readable red ASCII/CJK and a visible emoji, not replacement squares. The macOS helper uses native fallback artwork for missing glyphs, including color emoji. Compare the screenshot with accessibility text; correct text alone does not prove rendering. Repeat after opening a split with `Ctrl-g`, `v`, Enter (keeps the suggested session in the new pane), then Enter again to type, after resizing, and after reattachment. Check wide-glyph spacing, cursor position, and clipping at the pane boundary.

Check modified, composed, and dead keys in the same shell. Put the PTY in raw
mode so the line editor cannot consume the escapes:

```sh
stty -icanon -echo min 1
cat -v
```

Press Ctrl-Left, Shift-Enter, Alt-Backspace, and Option-b. Confirm `^[[1;5D`,
`^[[13;2u`, `^[^?`, and `^[b`; Option-b must print the meta form, never the
composed glyph.

Then press Option-e, a US-layout dead key, and record exactly what arrives. This
is an open question, not a known expectation: the helper publishes an IME area, so
winit's macOS `keyDown:` runs `interpretKeyEvents` first, and a dead key
(Option-e, `i`, `u`, `n`, or backtick) calls `setMarkedText:` and suppresses the
`KeyboardInput` event, meaning no `Key::E` reaches the helper. So Option-e may
start a composition (no bytes, an accent pending) where Option-b sends `^[b`.
Report which happens, and whether the following key commits an accented character
into the shell. If dead keys compose, Meta for those five keys needs a decision:
either the composition is correct for this helper or `option_as_alt` must be set.
Press Ctrl-C, then restore the shell:

```sh
stty sane
```

At the restored prompt, select a CJK input method and type a composition (with
Pinyin, `ce` then Space). Confirm only the committed characters reach the command
line and that no pre-edit letters leak into it. Check the candidate window's
position against the screenshot: it must sit at the cursor cell, not merely
somewhere over the terminal, because macOS places that panel from the single rect
the helper publishes. Record the input method used.

**IME composition: unverified as of 2026-09-08.** This machine enables only the
U.S. keyboard layout and the Character Palette (`defaults read
com.apple.HIToolbox AppleEnabledInputSources`), and no desktop computer-use tool
was available to drive the native window, so the composition, dead-key, and
candidate-window checks above were not run against the helper. The modifier and
meta encodings they exercise are covered by the unit tests
`special_keys_carry_modifiers`, `alt_letter_encodes_meta`, and
`unmodified_letter_keeps_its_text` in `src/gui/input.rs`; an automated egui test
cannot prove composition or panel placement, so the IME path still needs this
native check.

Return to browse with `Ctrl-g`. Press Space. Confirm a compact bottom-right
list above the footer, with the surrounding dashboard still visible. Press
`t` and confirm the terminal submenu names the selected session and shows
`Space t`. Press Backspace and confirm the group list returns. Press Escape,
then `?`, then Enter to open the Terminal group. Confirm Pause is listed for a
running session and Resume is absent. Disabled actions should show a reason.
Press Backspace, then `w`, then `X`; confirm the workspace picker highlights
the selected `consigint / gui-consigint-auth-handoff` and omits the
repository-root workspace. Type to filter to a different workspace, then press Enter; confirmation must name the chosen target.
When that workspace has running sessions, the confirmation names them by title
and offers to stop them before removing it, with no Force wording.
Escape cancels. Repeat with `Space p X` for projects, then `Space t X` for direct
terminal-close confirmation, cancelling both; confirm lowercase `x` in `Space t`,
`Space w` and `Space p` does nothing. Open tasks with `Ctrl-t`, press
`n`, and Tab to Project; check the same filtering, Up/Down, and Tab/Enter controls.
Escape cancels the editor, then Escape returns to the dashboard. Click project and workspace sidebar rows and
confirm `Space` shows only their applicable groups. The pane must show a card,
never a blank screen: the project card lists the repository path and each
workspace with its session count; with a workspace selected the pane header
names `project / branch` and the card shows the `~` path, its sessions with
status, title, agent and state, and the next keys. Narrow the window to 80
columns and confirm each session row still ends with its state and age. With a workspace selected, press `j` and confirm it opens the session the
card's `j open …` line names. Resize the window and scroll to
the last entry; confirm it remains visible and clickable. Press Escape.
Confirm the Space group list holds only the groups, Register project and Detach,
with no notification or sound toggles.
Press `:` and type `theme`; confirm a `Settings: Theme` row with
`Appearance · theme` below it, press Return, and confirm Settings opens on the
Theme row; press Escape. In the unfiltered palette confirm session rows read
`project / branch / title` and that Take control, Resume and Mark reviewed are
absent while they cannot run. Press `v`; confirm the second pane opens behind a
`New pane · choose its session` list that highlights its session and omits the
left pane's session. Type part of another session's branch, press Return, and
confirm the new pane shows it. Press `o`, then `Ctrl-g`; confirm the one
Operator conversation closes at once, the terminal panes return and the footer
reads `BROWSE  Space Menu  : Commands  ? Help`; press `o`, confirm the same
history and draft remain, and press `Ctrl-g` again. Click another application, then click a running
terminal pane once: the footer must stay BROWSE; a second click focuses the pane
for typing.
Press Space then `w` then `t`; confirm the key popup closes and the terminal form opens
with `Start: Terminal`. Confirm Workspace shows the selected `consigint / gui-consigint-auth-handoff`,
Name is optional and blank, and Command is optional. Submit with both blank.
Confirm terminal mode on the new session, then paste this command and press Return:

```sh
printf 'CUA_%s\n' N_SHELL_OK
```

Confirm a separate output line containing `CUA_N_SHELL_OK`; the echoed command alone is insufficient.

Return to browse with `Ctrl-g`, press `w`. Confirm the Create branch form shows
Project, an empty Branch name, and collapsed Workspace options. Expand Workspace
options and confirm Base appears. Type `feature/cua-workspace` as the Branch
name and wait for Base detection to finish. Submit; confirm the branch is created
without starting an Agent from this form and the workspace is selected in Browse.
Then press `n`, confirm the registered branch is selected in New agent session,
and press Enter to open the Branch choices. Verify the dialog and field rows
stay in the same positions at 80×24 and the supported 60×18 floor; the bounded
menu is anchored to Branch and may temporarily cover nearby fields. Check that
its highlighted option is visible and clickable, and that the form's Start and
Cancel controls stay in the footer. Click another visible picker field once to
switch the anchored menu, then click outside the menu and confirm it closes
without sending input to the terminal. Reopen Branch, press Escape once to
close only the menu, and again to cancel without starting a provider session.

```sh
printf 'CUA_%s\n' W_SHELL_OK
```

Confirm a separate `CUA_W_SHELL_OK` output line. Run `pwd` and confirm it ends in
`/feature-cua-workspace`, the folder named after the branch (`-2`, `-3`, … when a
folder of that name already exists), not a 32-character hex ID. Record this shell's PID and
process group with the other fixture processes before cleanup. Return to
`consigint / gui-consigint-auth-handoff / local` for the checks below.

Return to browse with `Ctrl-g`. Press `a`. Tab through a configured `picker_roots` directory to the demo repository (a `git` marker sorts it first). Confirm Name is the repository basename and Workspace root is `<config dir>/workspaces/<name>`. Press Tab once more: focus moves to Name instead of descending into the checkout. Escape without submitting if this run should not register a project; otherwise one Enter registers it.

For the first run, start a second Dashboard with its own empty `OVRCR_HOME` and
`OVRCR_SOCKET` in a fixture shell, as `nested-ladder.sh` does. Confirm the
welcome card shows `1  a  Register a repository`, `2  w  Create a branch` and
`Space actions · : commands · ? keys`, and no Home or Socket
path. Press `Space v e` and confirm `Home:` and `Socket:` head the Events list
and name that fixture's directories. Escape, register a disposable repository
with `a`, and confirm the pane then shows its project card leading with
`Next: press w to start your first agent`.

### Workspace launch and titles

In the disposable fixture, also verify:

1. Create a branch from the standalone Branch form; confirm the new checkout is
   selected and has no sessions, and that its card says `No sessions yet` and
   offers `n new agent session here`; use `Space w t` for a terminal.
2. Open **New terminal** with `Space w t`, choose a custom command that prints
   a unique marker and waits for input, then confirm exactly one terminal and
   its actual output. Confirm the named action starts at Terminal without an
   Agent choice.
3. In that session emit `printf '\033]2;CUA title 界\007'` followed by a unique
   output marker; confirm the output appears but sidebar and pane titles keep
   the original session name. Repeat with OSC 0 and a different title.
4. Set a title using Rename terminal; emit another app title and confirm the
   manual title holds. Clear Rename and confirm the original name returns and
   stays unchanged after further application title updates.
5. Give two sessions the same manual title and confirm their visible identifiers
   distinguish them. Confirm both still receive input in the intended pane.
6. Exercise an owned failing launch after workspace creation. Confirm the
   workspace remains, the form turns into Retry / Choose another agent / Open
   shell, and recovery starts a program there without recreating it.
7. Exercise Agent using a disposable configured preset, recording that this proves
   the launch UI only, not provider reporting. Verify its project preference after
   dashboard restart. Verify Terminal custom command text is absent from settings.
8. Exit an owned session. Verify Reopen explicitly asks you to confirm that old
   agent/background processes stopped; cancel once and verify no new process.
   After independently verifying the fixture processes are gone, confirm.
   Check the same row/title, absence of old output, and a separate fresh-shell
   execution marker. An older capture without this acknowledgement is not
   evidence for the current recovery policy.
9. Select the `feature/cua-workspace` session created above, then run
   `git checkout -b cua-switched`. Confirm the workspace row becomes
   `cua-switched` without replacing that session. Paste a unique printf in the
   same shell and confirm its output. The old branch label must not remain as
   that workspace's name.
10. Select consigint's root workspace (`main` with the root marker). From there,
    `w` then `X` must be unavailable (`repository-root workspace cannot be
    removed`). In its `local` shell, run `git checkout -b cua-root-drift`.
    Confirm the root warning and that `Space`, `w`, `t` leaves the New terminal
    action disabled with `root workspace is not on the default branch`; the
    existing shell must still accept a unique printf. Press `n`: New agent
    session remains available so another registered branch can be chosen. Repeat
    the removal check while drifted. Restore with `git checkout main` and leave
    the root on `main`.
11. Press `w`, enter an existing branch name (for example
    `feature/cua-workspace` again), and submit. Confirm the branch-only form
    keeps the values and shows `That branch already exists in this repository`,
    with no error code, internal id, or failed sidebar row. Escape.
12. Remove a disposable workspace that has a running shell. Confirm the
    confirmation names the session by title and offers to stop it; confirm, then
    verify the shell's process group is gone and the workspace row disappears.
    No Force dialog appears at any point.
13. Verify the branch-to-agent prompt path without a paid provider. Use an
    isolated Dashboard fixture whose PATH contains an owned `claude` stand-in;
    it must answer only `claude auth status --json` with fixture data, print each
    launch argument, then run a shell. Do not use an installed account or a
    configured `agents` override. Press `n`, open **Branch**, choose **Create
    new branch…**, and create `feature/cua-agent-prompt` with detected Base.
    Confirm the parent form returns with that registered branch selected and
    focus on Agent harness. Tab to Initial prompt and enter
    `--help me fix the build`; submit with Alt-Enter. Confirm the stand-in
    receives exactly one native argument `--help me fix the build`, immediately
    after `--`, and that the prompt is not the session name or remembered launch
    choice. Reopen/recovery must not replay the prompt; retained-session tests
    cover that behavior.

For a deliberate server-loss check, record the fixture paths and owned processes
first. Verify restored rows without launching processes, then explicitly confirm
that old agents and background processes have stopped before reopening a shell.
A host reboot is not required: `retained_ownership` tests deterministic boot-ID
transitions and independently checks the native reader on macOS and Linux.

The GUI helper deliberately preserves its fixture after an unexpected server
exit. Record that cleanup-guard result separately. Shut down any replacement
server through the fixture socket, verify every owned process group is gone,
stop only the task-owned GUI launcher, and remove only its recorded disposable
root. Do not weaken the normal cleanup guard to make a crash check pass.

### Other-running-agent search

Use two explicitly Agent-launch fixtures in different disposable projects or
workspaces. The helper's labeled shell fixtures still have Terminal launch kind
and must not count as search destinations. Use a disposable configured Agent
preset or the production `CreateSession` request with an input-acknowledging
shell command; never start a paid provider just to populate this check.

From Browse, press lowercase `s`; also check `Space v s`. Confirm Agents lists
the other Running agents and shows the selected project/workspace. Filter using
words from a different project and session title. Press Enter once and confirm
Terminal mode after the matching screen is ready, without a second Enter. Send
a unique marker and confirm a separately
printed acknowledgement only in the chosen Agent, not the previous PTY.
From Terminal, press `Ctrl-g` then `s`; confirm the current Agent is omitted.
Escape must preserve the selection in Browse. Pause an owned candidate while
the picker is open, then confirm it: the unavailable reason must remain visible
and no neighboring row may be selected. Resume it; it stays unavailable until
reopening. While a view is loading, input and paste must be rejected rather than
replayed; cancel pending typing with `Ctrl-g` and verify a late completion stays
in Browse. Repeat selection into an Agent already in the other pane and confirm
it is focused without duplication. Keep screenshots and matching accessibility
text for these outcomes. The outer-PTY acceptance fixture deterministically
holds a real view's final acknowledgement to prove loading-input rejection,
including a pending keyboard/paste burst released across acknowledgement cutover.
The event-loop regression forces acknowledgement-before-input ordering and a
multi-batch input drain; Terminal mode begins only at the quiet input boundary.

For attention-first presentation, publish accepted fixture observations through
the existing reporting protocol: Waiting Input, Busy plus Unread, reviewed Ready,
error and unavailable reporting. Verify that only Waiting Input and otherwise
Unread are promoted, and that a current attention-bearing Agent is excluded.
While the picker remains open, change an observation and confirm visible status
updates without moving its highlight or rows; reopening must use the new order.
Check duplicate titles, compact selected location and offscreen filtering at the
fifty-session limit. Preserve screenshots, accessibility text and destination
acknowledgements; opening or selecting must not clear Unread or resolve Input.

### Who needs you

Publish fixture observations the same way: one Waiting Input and two Unread
responses, one of them inside a folded workspace. Confirm the line above the
quota section reads `? 1 needs input · ✓ 2 ready` and the Browse footer ends with
`i Next input needed`. Press `i` repeatedly: selection and the focused pane go
to the waiting session first, then each unread one in sidebar order, unfolding
the folded workspace, and wrap. Clicking the line does the same. On an unread
session, confirm viewing it and pressing Enter leave the `✓`; type one
character and confirm the `✓` and the line's ready count drop without `R`. Any
key sent to the session counts as a reply, arrows and Esc included. Let a session
become ready while the pointer rests on a tree row and confirm no row moves.
Click quota above the Operator dock and confirm its details open. Shorten the
window: quota compacts to provider summaries, then a details pointer, then hides
before attention. Preserve three tree lines; the dock yields its row if needed.
Keep screenshots and accessibility text for each outcome.

### Operator

The Operator is one durable conversation run by the installed pi agent. Start
with a disposable fixture and no provider turn. Confirm the first project is
directly below the title, and a single **Operator** dock sits below attention and
quota, immediately above the footer. A closed idle dock says **Open chat**.
Confirm neither Operator shortcut appears in the title bar; clicking the retired
positions opens neither Operator surface.

Click each end of the dock, returning with `Ctrl-g` between opens. Confirm both
open the same conversation with the composer focused. Type an unsent draft,
return to Browse and confirm **Draft** in the dock. Select another session and
press `o`: confirm the text remains and the target above the composer now names
that session. The open dock has a selection marker. Save Browse and conversation
screenshots with matching accessibility text.

Return to Browse, open **Quick message** through Commands, and cancel without
sending. Check `Ctrl-o` too. In Terminal mode `Ctrl-o` must stay with the program;
`Ctrl-g` then `Ctrl-o` opens Quick message. Hide the sidebar and confirm the dock
hides too. At 80×24 and in a 60-column split, Operator opens full-width by default;
`Ctrl-l` reveals the ordinary Projects sidebar. At 60×16 the composer stays
usable; below that floor, resize guidance retains the draft.

Confirm there is no conversation count, collapse arrow, plus button, child row
or close control, and `Ctrl-n` does nothing. The conversation header reads
`Operator · pi` before model selection and `Operator · pi · provider/id`
afterwards, never an invented `unknown/unknown`.

For the separate provider and activity walkthrough below, use the loopback
helper; it is not needed for dock placement, focus or draft acceptance.

Start the GUI provider helper against the launcher's fixture root and wait for
`OPERATOR_PI_GUI_READY` before sending:

```sh
OVRCR_OPERATOR_GUI_HOME=/absolute/task-owned/ovrcr-gui-root cargo test -p ovrcr --features acceptance-diagnostics --test operator_e2e operator_pi_gui_provider -- --ignored --nocapture
```

The helper prints the fixture workspace and marker path when it starts. It does
not interpret arbitrary requests: each turn must contain one of its exact markers,
otherwise it answers only `FIXTURE_FOLLOWUP_DONE`. Send this sequence:

1. `REQUEST_ONE_REAL_TOOLS` — real `write`, `read` and `edit` operations on
   `gui-fixture-marker.txt` in the fixture workspace (it ends `OFFLINE_MARKER_v2`),
   then a Bash `printf BASH_ && "$OVRCR_EXECUTABLE" --json project list`, then
   `TURN_ONE_DONE`. Confirm the streamed text and the expandable file and Bash
   receipts.
2. `REQUEST_TWO_RESUME` — `TURN_TWO_DONE`; the same singleton continues with turn
   one's messages still in the journal. The helper writes
   `operator-pi/fixture-requests.jsonl` only when it exits, so that file is
   post-exit evidence that turn one's messages were provider context; the live
   evidence is the continuing conversation itself.
3. `REQUEST_THREE_HELD` — streams `HELD_BEFORE_RELEASE` and then holds. This is
   the held stream; the tool sequence above is a separate request.
4. While it is held, open `Ctrl-o` from another pane. The Quick overlay names
   `Operator · pi` and the captured target and edits the same draft as the full
   view. Send must preserve that draft with the busy or stop-unconfirmed reason
   instead of queuing, steering or creating another conversation. A Quick draft
   dismissed with Esc before the conversation exists leaves no conversation; once
   it exists, Esc only hides the overlay and the shared draft stays. Leave
   Operator, select another session, and reopen Quick to confirm the overlay and
   the full composer both name the newly selected target.
5. Release the held stream by creating the marker under the fixture root, for
   example `touch "$OVRCR_OPERATOR_GUI_HOME/operator-pi/fixture-release"`, and
   confirm the third turn completes with `HELD_BEFORE_RELEASE`.
6. `REQUEST_FOUR_BACKGROUND` — a Bash command with a background child, held for
   the Stop check below.

Confirm the sidebar row's activity clause tracks Sending, Responding, Waiting for
`<tool>` result and Stop unconfirmed, and that the row is the only place the
conversation appears.

While `REQUEST_FOUR_BACKGROUND` runs, press `Ctrl-x` (or `Ctrl-c` in the empty
full composer). The activity row says Stopping, the partial text and receipt stay
in the journal, and once the Server's retained `worker_stopped` proof arrives the
turn reads `interrupted` with no Stop control instead of `interrupted · stop
unconfirmed`. `operator-pi/fixture-stop` is not emitted when Operator stops: it is
an input that exits the provider helper's serving loop, so creating it mid-run
kills the provider needed for later turns. Reserve it for fixture cleanup and read
controlled-stop proof from the retained `worker_stopped` status. Then send
`REQUEST_FIVE_AFTER_INTERRUPT` — `TURN_FIVE_DONE`; the fifth turn's model context
contains the last completed checkpoint, not the interrupted request or its tool
output. Return to the terminal panes with `Ctrl-g`, run a normal shell command and
confirm independent output, then reopen with `o` and confirm history and the draft
remain. Resize to narrow and wide layouts, then reconnect the Dashboard and
confirm the same conversation is attached (startup `Current` creates nothing).
Keep fresh screenshots and AX text for every changed state.

### Pane header

Select a root shell: the pane header reads `local  shell · <elapsed>` with the pid
muted at the far right, and no `agent unknown`, `pid:` or `elapsed:` field. Select a
fixture agent: its title, `agent:model`, the state in words (`Working`, `Ready`,
`Waiting for input`, `Idle`) and the elapsed time; the second line shows `ctx`,
tokens and cost when the fixture reports them, and never repeats the title. A
just-launched agent says `starting…` until its first report; a reporter lost after
that says `status off`, never its reason or `observed`/`confirmed`. Narrow the window
until the header clips: the pid goes first, then the model, the elapsed time and the
agent label, while the state stays readable. An Unread reply reads `Ready · unread
reply`, and a narrow split pane still says `Unread`.

## 3. Mouse forwarding and wheel history

### Dashboard controls

Run these checks with the mouse for navigation; use the keyboard only to enter
text. Save a screenshot and accessibility text after each changed view. Repeat
the form and menu checks in a short window to exercise scrolling and clipping.

1. Open the action menu with its visible control and open a second terminal pane;
   press Return in the chooser that opens to keep the suggested session.
   Click each pane and run `printf 'MOUSE_%s\n' LEFT_OK` or
   `printf 'MOUSE_%s\n' RIGHT_OK`. Confirm distinct output in the
   intended pane. Switch directly from an active terminal without `Ctrl-g`.
2. Drag the sidebar border and confirm the pane narrows and widens with it.
   Drag the divider left and right. In each pane run `stty size` and
   compare the reported columns with the drawn terminal width. Make the window
   too narrow for both panes, widen it again, and confirm assignments and the
   chosen split survive. Release the drag outside the terminal and confirm the
   next click works normally.
3. Click a project or workspace name to select it without collapsing it. Click
   the first cell of a project header, or the branch glyph of a workspace, to
   collapse and expand it. Scroll the sidebar and select a row that was
   initially offscreen. Confirm the selected target by its name.
4. Open a creation form from the visible action menu. Click a nonactive field,
   edit in the middle of its value, choose a pick-list option, and operate any
   toggle. Browse a directory through the path picker. Use Cancel, reopen, and
   submit a disposable terminal; prove input with a separate `MOUSE_FORM_OK`
   output line. Cancel a destructive confirmation and confirm its target remains.
5. Open Tasks using mouse controls. Select and scroll its rows, open an editor,
   click fields and a project choice, and cancel. Exercise task controls only on
   a task owned by this fixture; inspect confirmation targets before proceeding.
6. Print a line containing `ASCII 界🙂 tail`, open Copy from the action menu, drag
   across the text, and use Copy. Paste into an owned fixture input and compare
   the selected text. Open History, scroll, select text, and close it through its
   visible control. Confirm new live output does not rewrite the frozen view.
7. While a form or menu is open, click outside it over a terminal. Confirm the
   click does not reach the terminal. Repeat a focus-changing click with Vim
   mouse tracking enabled: the click selects the other pane without moving that
   application's cursor. A subsequent click inside the focused Vim pane should
   reach Vim as described below.

Keep these observations separate from automated test results. If the computer-use
tool cannot perform a drag or inspect the clipboard, mark that check unverified
and retain any available automated evidence. Selection highlighting and emitted
OSC52 bytes do not prove clipboard delivery; test paste in a terminal that handles
OSC52 when the GUI helper cannot deliver it.

### Application forwarding

Stay on `consigint / gui-consigint-auth-handoff / local` in terminal mode. Move the pointer over the terminal pane, not the sidebar or footer, and scroll the wheel up. History should open at the tail (`HISTORY` in the footer). Each further wheel-up tick moves the header's `row a-b` up three rows. Wheel down at the newest row returns to the live prompt.

In the same or another live pane, run `vim -n -u NONE`, then `:set mouse=a`. Click in the buffer: the Vim cursor moves to that cell. Wheel should scroll Vim; History must stay closed. `:set mouse=` then wheel up while Vim is still running must not open History and must show no error (alternate screen); PageUp then shows the muted footer line `History isn't available while <title> is full-screen`, without `ERROR:` or `Conflict`. Quit Vim before repeating the plain-shell History check.

Computer-use tools often cannot hold a mouse button across Ctrl-g. Skip that gesture here; the TUI test `ctrl_g_releases_held_buttons_before_switching` covers it.

### Full-screen agent Transcript

Use a task-owned synthetic Claude/Codex conversation fixture, never a paid turn just
to populate this check. Record its accepted conversation reference and private source
file. With its program on the alternate screen, press PageUp and confirm Transcript
shows the synthetic user prompt, assistant reply and tool name/input. Check wheel-up
also opens it. Scroll with keys and the wheel, use End to reload the tail, and close
with Esc. While loading and open, type, paste and click over the terminal: no input
may reach the program. After closing, send a unique marker and confirm its separate
acknowledgement. Keep source bytes unchanged, matching screenshots/AX, and the fixture
process groups for cleanup. A plain full-screen program without a recorded supported
conversation still gives PageUp's muted explanation and leaves wheel-up quiet.

## 4. Exercise CLI controls through the GUI

First inspect each inherited path separately; macOS `printenv` accepts one name:

```sh
printenv OVRCR_CONFIG
printenv OVRCR_SOCKET
```

Confirm both paths belong to this launch's disposable directory, ending in `config.toml` and `server.sock`. Stop if either is missing or points elsewhere. Do not try a config-only CLI command: config selection does not select the server socket.

In that same shell, replace the binary path below with this checkout's absolute path. Paste and execute each line, inspecting its result before continuing. At each terminal list, save the PIDs and use host process inspection to record their process-group IDs while they are running:

```sh
c=/absolute/checkout/target/debug/ovrcr
"$c" project get consigint --json
"$c" workspace get --project consigint --branch gui-consigint-auth-handoff --json
"$c" terminal list --json
t=$("$c" terminal create --project consigint --workspace gui-consigint-auth-handoff --name cua-check -- /bin/sh)
"$c" terminal list --json
"$c" terminal send "$t" --text "printf 'CUA_%s\n' BACKGROUND_OK"
"$c" terminal read "$t" --json
```

Confirm the new sidebar row and `CUA_BACKGROUND_OK` in the read result. The original terminal must remain selected. If output has not arrived, read again after observing progress. Reads contain the current screen only.

Use `Ctrl-g`, select `cua-check`, and press Return. Confirm the marker in its visible screen and record its PID. Detach with `Ctrl-g`, then `q`; click **Restart dashboard**, reselect `cua-check`, and confirm the same PID and output.

Return to the original `local` shell, where `$c` and `$t` are still set:

```sh
"$c" terminal close "$t" --json
```

Wait for `{"ok":true}` and confirm the sidebar row disappears. Closing a shell can take several seconds.

## 5. Verify cleanup and report evidence

Use the demo paths and process-group IDs recorded above. Close the native window through computer use and wait for the launch command to exit successfully; cleanup can take about a minute.

After closing, use process/filesystem inventory. Do not query the closed app through app-specific CUA state methods: they can relaunch it and create another fixture. If that happens, record the new owned processes and paths, close that fixture too, and report its cleanup separately.

Verify the demo directory and socket are gone. For each recorded process group, `os.kill(-pgid, 0)` must raise `ProcessLookupError`; a permission error does not prove cleanup. Keep the fixture and report its path if cleanup fails.

Report the tested commit, GUI checks and observed markers, cleanup result, and any blocked checks. Keep automated test results separate from computer-use evidence. A successful build or screenshot alone does not prove terminal input or lifecycle behavior.
