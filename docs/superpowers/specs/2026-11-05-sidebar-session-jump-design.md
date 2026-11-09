# Sidebar session jump labels

Status: behavior confirmed by Kyle in Q8 on 2026-11-06; implementation subsequently authorized through implement-spec. Source baseline: `8f9569729574b0f0dedb56bd8b9ff1a1384b08fa`.

Status note (2026-11-07): the Ctrl-g revision at the end supersedes the Control-hold interaction described below. [docs/dashboard.md](../../dashboard.md) and [docs/development/tui.md](../../development/tui.md) are the live contract.

Published specification: [OVRCR #401](https://github.com/xlyk/ovrcr/issues/401), labeled `ready-for-agent` through the requested to-spec workflow on 2026-11-06. Publication did not start implementation.

## Confirmed behavior

Holding Control shows temporary navigation labels on visible Agent session rows. Pressing a label focuses that session. Running Agents enter Terminal mode only after Control release and screen readiness. Retained sessions remain in Browse; navigation never restarts a process.

Shells, Operator chats, projects, workspaces, and provisional creation rows do not get labels. The feature reaches currently drawn rows; the existing Agents search reaches running Agents outside the viewport or inside folded groups.

A session jump label is navigation, distinct from the session title, provider/model label, and session identity. Viewing a session neither reviews Unread nor answers an Input request.

Control-hold is available only in verified terminal setups. Other setups retain the existing Agents search; this feature adds no portable trigger. Labels appear only while the sidebar is already visible. Holding Control never reveals a hidden sidebar or changes its visibility preference.

## Interaction

The product remains a full-screen Dashboard with persistent sidebar and terminal panels. Its ordinary output and terminal restoration contract stay with the existing Dashboard.

The main loop is: glance at a visible Agent, hold Control, read its label, choose it, then inspect or reply. Related actions are cancel by releasing Control without choosing, return to Browse, scroll the sidebar, search hidden Agents, and switch to an Agent already in another pane.

- Labels occupy the existing five indentation cells on the title line. Keep the status glyph and title at their original columns. Never label the second provider/model line separately.
- Use the existing theme accent plus a literal bracketed label. No animation, added border, duplicated provider name, or permanent row decoration.
- Capture label-to-session/run assignments when the gesture starts. Activity updates may change statuses, but never assign an existing label to a different session during that gesture.
- A removed, archived, or replaced run invalidates its captured label. Refuse that destination without selecting a neighbor. New rows wait for the next gesture.
- Reuse the ordinary session selection and acknowledged-view input boundary. Focus an existing destination pane rather than duplicate the session. Consume shortcut keys; never replay them into either program.
- A retained destination is inspection only. Its existing explicit Resume/Reopen action remains separate.
- Escape, focus loss, modal opening, disconnection, resize, or sidebar scroll cancels the gesture. Require a fresh modifier press after cancellation. Left/right Control are one held state; releasing one while the other remains down is not cancellation.
- Keep forms, confirmation dialogs, command menus, History, and Copy ahead of session jumps. Their existing keyboard behavior remains authoritative. Normal Browse, Terminal, and the unobscured Operator interface are entry contexts.
- Preserve non-digit Control shortcuts and unrelated Command/Super shortcuts. Ctrl-g, Ctrl-o, Ctrl-c, copy/paste, and other ordinary shortcuts must not become a jump or reach a different program after navigation.

Labels appear immediately on a fresh Control press, without a delay or animation. A completed label selects immediately and hides the labels. Allow one selection per hold; repeats and later digits in that hold cannot select again or type into the new Agent. Running destinations enter Terminal only after both Control release and the existing acknowledged-view/input boundary. A key release is never a program Keystroke.

Cancellation before selection preserves the original session and input mode. After selection, cancellation of pending typing leaves the chosen destination in Browse; it does not undo the navigation. An ordinary Control release after selection completes the gesture rather than cancelling its pending screen readiness. Esc/Ctrl-g during loading retains the existing Agent-switch cancellation behavior.

## Label allocation

- Number eligible, drawn title rows from top to bottom, including the current Agent. Include retained Agent rows; exclude archived/provisional rows, shells, Operator chats, and container headings.
- A title line must actually be drawn with room for the badge, status glyph, and title. A clipped metadata line whose title is offscreen does not receive a label. If there is no readable title-line space, assign no action there.
- Always show only single ASCII digits `1`–`9`, assigned to at most the first nine eligible drawn Agent title rows. Later rows remain visible and unlabelled regardless of whether ten or fifty Agents are visible.
- Capture the mapping once at Control-down. Changes in count, membership, activity, or titles do not change the captured targets until a fresh hold. Numbering follows sidebar order, not attention priority or session IDs.
- Pressing `Ctrl+1`–`Ctrl+9` selects immediately; there is no timeout, Enter, prefix, or release-to-select rule. `Ctrl+0` and other unmapped digits show a concise `No visible Agent for that number` hint, send no program input, and permit a retry during that hold.
- To reach a later row, scroll the sidebar. Scrolling cancels the hold; after releasing Control, a fresh hold labels the new first nine eligible drawn rows. An invalidated destination refuses its captured label without reassigning it.
- Releasing Control before choosing a digit cancels without navigation. Non-digit Control shortcuts and unrelated Command/Super shortcuts keep their ordinary handlers, including Operator editing.
- A non-digit Control shortcut cancels the unfinished gesture and retains its ordinary Dashboard/program handling. Additional Shift, Alt, or Command/Super modifiers cannot select a label. Never strip modifiers and dispatch their key as an unmodified Browse command or program key. Claimed host configurations must deliver the digit chords; this feature does not silently reconfigure host shortcuts.

## Layout

Examples show fewer than ten eligible Agents. `?` denotes a request for input; provider metadata may occupy its ordinary second line. When more than nine Agents are visible, later rows stay visible without labels and are reachable by scrolling or Agents search.

Wide, 120 columns; the existing sidebar is 40 columns:

```text
OVRCR                                        [Commands]
OPERATOR                                | review socket shutdown
  Release review                        | codex · Waiting for input
OVRCR ----------------------------------|-----------------------------------
  main                                  | Agent screen
       local                            |
 [1] ? Review socket shutdown            |
       └ codex:gpt-5.4                   |
 [2]   Fix sidebar clipping              |
       └ claude:opus                     |
                                        |
Ctrl held · choose label · Esc cancel
```

At 80×24, the sidebar remains its existing 40 columns and one terminal pane uses the remaining 40. At 60×24 the sidebar is capped at 30 columns; only one terminal pane remains visible. Titles truncate by cell width and provider/model metadata follows the existing stacked-row rules. Holding Control must not change row heights or the terminal's advertised PTY size.

```text
60 columns
OVRCR                          [Commands]
OVRCR -----------------------| Review socket shutdown
  main                       | Waiting for input
 [1] ? Review socket shutdo… |-----------------------------
       └ codex:gpt-5.4        | Agent screen
 [2]   Fix sidebar clipping  |
       └ claude:opus         |
Ctrl held · choose · Esc cancel
```

When the sidebar is hidden, leave the primary pane unchanged and show no labels. The ordinary sidebar toggle remains the route to reveal it. At narrow widths, the existing single-terminal-pane fallback remains; if the user hides the sidebar, the primary pane wins. In Operator's narrow layout, labels work only when its on-demand sidebar is already open. Below 60×16, suppress labels and show the existing Operator resize guidance when in Operator; any terminal-dashboard minimum remains an implementation validation item rather than a new global threshold.

Clutter audit: zero new borders; one non-color label cue plus one accent cue; no new idle markers. A three-cell badge replaces indentation, adding zero columns or lines. In a 40-cell sidebar that is 7.5% of each labeled title row, and 10% at 30 cells. Use one temporary footer hint instead of repeating `Ctrl+` on every row. Preserve status icons because they communicate a different fact.

## Input support

The production Dashboard's current terminal path cannot observe a bare Control hold/release. `ovrcr-gui` can, but is a disposable development helper. A demonstration there alone cannot satisfy production acceptance.

The [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) offers opt-in modifier and event-type reporting. A supported host must also deliver the chosen Control chords; protocol negotiation alone proves neither host key mappings nor propagation through a multiplexer. Kyle chose Control-hold only in verified setups, with the existing Agents search elsewhere (Q4). No particular host has been named or verified for this feature; implementation must establish the supported configurations before advertising the gesture there.

Any enhanced reporting belongs to the outer Dashboard input boundary and its existing terminal guard. Enable only negotiated modes, restore them on exit/error/panic and temporary handoff, and preserve ordinary program input through the shared encoder. Do not use global OS keyboard monitoring or a second Server/Bridge keyboard owner to bypass a terminal limitation.

## States and implementation acceptance

| State or transition | Expected visible/input behavior |
| --- | --- |
| Idle | Ordinary sidebar, no labels or reserved width |
| Sidebar hidden | No reveal, labels, layout change, or jump interception |
| Control held, eligible Agents visible | Fixed labels on title lines; no row movement |
| Ctrl+digit `1`–`9` | Select the matching captured target immediately |
| `0` or another unmapped digit | Show a concise retry hint; no navigation or program input |
| Later visible Agent rows | Leave unlabelled; scroll, release Control, and start a fresh hold to label the new first nine |
| No eligible Agents | No labels; one concise `No visible agents` hint |
| Target chosen | Hide labels and latch one selection until Control release |
| Target chosen, view loading or Control still held | Selected destination and existing loading indication where applicable; no typing replay |
| Target ready and Control released | Enter Terminal once the exact view and input boundary are ready |
| Retained target | Show its retained view in Browse; no process launch |
| Destination disappears or changes run | Cancel/refuse that label; never choose its neighbor |
| Focus lost, modal opens, resize, scroll, disconnect | Remove labels; preserve ordinary mode/draft/session state |
| Modifier reporting unavailable | Keep existing Agents search; no new portable trigger or unusable hold hint |
| Too small | No clipped actionable labels; preserve work and show applicable resize guidance |

Implementation acceptance should exercise the production event dispatcher, shared row geometry, real session switch, Server view acknowledgement, and input drain. Cover already-focused and other-pane destinations, retained rows, duplicate titles, fifty total sessions, a clipped first/last row, activity updates, deleted/replaced targets, repeated label keys, both Control keys, extra modifiers, focus loss without release, and normal Command copy/paste shortcuts. Verify no shortcut bytes reach old/new programs and no Unread is reviewed.

Explicit allocation boundaries are zero, one, nine, ten, and fifty visible Agents, plus fifty total with only a subset drawn. Prove only the first nine receive labels, later visible rows remain in place and unlabelled, nine-to-ten count changes do not alter an active mapping, release without a choice cancels, and an unmapped digit safely permits retry. Scroll a fifty-Agent tree, release Control, and prove a fresh hold labels the new first nine. Prove non-digit Control and Command/Super shortcuts retain their normal Control-GUI paths, including Operator draft behavior. Cover readiness before Control release, release before readiness, cancellation after selection, same-session no-op readiness, and every hidden-sidebar path. Exercise the outer keyboard protocol through normal exit, interrupt, errors, panic, and temporary suspend/handoff; restore its mode stack as well as the existing terminal state.

Rendered checks need wide, 80×24, 60×24, and 60×16 frames, narrow sidebar widths, monochrome, CJK/combining/emoji titles, and no layout shift during hold. Production-terminal acceptance must prove Control-down, visible labels, a chosen destination with a unique acknowledgement, Control-up cancellation, and cleanup in the supported host. Test through any claimed tmux/SSH chain separately; keep unavailable configurations explicit. Native helper evidence supplements this proof.

## Confirmation and source

Kyle confirmed this behavior in Q8 on 2026-11-06. The later implement-spec request authorizes implementation; it does not authorize merging to main. The executable work graph is recorded in [specification #401](https://github.com/xlyk/ovrcr/issues/401) and tickets [#402](https://github.com/xlyk/ovrcr/issues/402), [#403](https://github.com/xlyk/ovrcr/issues/403), [#404](https://github.com/xlyk/ovrcr/issues/404), and [#405](https://github.com/xlyk/ovrcr/issues/405).

## Dated design revision

On 2026-11-06, after trying the GUI, Kyle revised the interaction to use only single digits, superseding the adaptive two-digit codes, zero-padding, prefix display, matching-prefix emphasis, and Backspace prefix editing. Kyle then preferred holding Control instead of Cmd. The active contract is Ctrl+1 through Ctrl+9 for the first nine eligible visible Agent rows; later rows stay visible and unlabelled and remain reachable through scrolling or Agents search. A digit selects immediately. Non-digit Control shortcuts and Command/Super host shortcuts retain their ordinary paths. All other cancellation, readiness, identity, retained-session, Operator, layout, and verified-input-support contracts remain in force. The revision was reflected in published issues #401–#405 before this implementation.

For computer-use verification Kyle directed use of the existing disposable OVRCR TUI testing GUI. Exercise the real Dashboard input boundary there, and keep evidence from other production terminal hosts separate. Implementation acceptance remains open until the actual checks are recorded.

## Dated design revision: Ctrl-g picker

On 2026-11-07 Kyle replaced the Control-hold trigger with `Ctrl-g`. The picker
opens on a `Ctrl-g` press and stays open until a digit selects, `Esc` or `Ctrl-g`
closes it, or another key closes it and continues with its ordinary handling.
Labels use bare digits `1`–`9`; `Ctrl+digit` remains accepted. The mapping is
captured when the picker opens, and scrolling closes it so a fresh `Ctrl-g`
labels the new first nine rows.

Because `Ctrl-g` is an ordinary key event every terminal delivers, the feature
no longer depends on Kitty modifier reporting, verified frontends, the
`OVRCR_DASHBOARD_INPUT_PROFILE` environment variable, `DashboardInputProfile`,
or its iTerm2/GUI qualification. From Terminal mode, the `Ctrl-g` that returns
to Browse also opens the picker; from Operator, the ordinary `Ctrl-g` leaves
Operator and then labels the Browse sidebar if it is visible.

A selected running destination still waits for the exact view acknowledgement
and the production input-drain boundary; there is no modifier release to wait
for. A retained destination still stays in Browse for inspection. All other
label allocation, identity, cancellation, layout, and no-replay contracts from
the Control-hold design remain in force.
