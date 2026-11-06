# Sidebar session jump labels

Status: behavior confirmed by Kyle in Q8 on 2026-11-06; implementation subsequently authorized through implement-spec. Source baseline: `8f9569729574b0f0dedb56bd8b9ff1a1384b08fa`.

Published specification: [OVRCR #401](https://github.com/xlyk/ovrcr/issues/401), labeled `ready-for-agent` through the requested to-spec workflow on 2026-11-06. Publication did not start implementation.

## Confirmed behavior

Holding Cmd shows temporary navigation labels on visible Agent session rows. Choosing a label focuses that session. Running Agents enter Terminal mode only after Cmd release and screen readiness. Retained sessions remain in Browse; navigation never restarts a process.

Shells, Operator chats, projects, workspaces, and provisional creation rows do not get labels. The feature reaches currently drawn rows; the existing Agents search reaches running Agents outside the viewport or inside folded groups.

A session jump label is navigation, distinct from the session title, provider/model label, and session identity. Viewing a session neither reviews Unread nor answers an Input request.

Cmd-hold is available only in verified terminal setups. Other setups retain the existing Agents search; this feature adds no portable trigger. Labels appear only while the sidebar is already visible. Holding Cmd never reveals a hidden sidebar or changes its visibility preference.

## Interaction

The product remains a full-screen Dashboard with persistent sidebar and terminal panels. Its ordinary output and terminal restoration contract stay with the existing Dashboard.

The main loop is: glance at a visible Agent, hold Cmd, read its label, choose it, then inspect or reply. Related actions are cancel by releasing Cmd without choosing, return to Browse, scroll the sidebar, search hidden Agents, and switch to an Agent already in another pane.

- Labels occupy the existing five indentation cells on the title line. Keep the status glyph and title at their original columns. Never label the second provider/model line separately.
- Use the existing theme accent plus a literal bracketed label. No animation, added border, duplicated provider name, or permanent row decoration.
- Capture label-to-session/run assignments when the gesture starts. Activity updates may change statuses, but never assign an existing label to a different session during that gesture.
- A removed, archived, or replaced run invalidates its captured label. Refuse that destination without selecting a neighbor. New rows wait for the next gesture.
- Reuse the ordinary session selection and acknowledged-view input boundary. Focus an existing destination pane rather than duplicate the session. Consume shortcut keys; never replay them into either program.
- A retained destination is inspection only. Its existing explicit Resume/Reopen action remains separate.
- Escape, focus loss, modal opening, disconnection, resize, or sidebar scroll cancels the gesture. Require a fresh modifier press after cancellation. Left/right Cmd are one held state; releasing one while the other remains down is not cancellation.
- Keep forms, confirmation dialogs, command menus, History, and Copy ahead of session jumps. Their existing keyboard behavior remains authoritative. Normal Browse, Terminal, and the unobscured Operator interface are entry contexts.
- Preserve unrelated Cmd shortcuts. Copy/paste and other non-label shortcuts must not become a jump or reach a different program after navigation.

Labels appear immediately on a fresh Cmd press, without a delay or animation. A completed label selects immediately and hides the labels. Allow one selection per hold; repeats and later digits in that hold cannot select again or type into the new Agent. Running destinations enter Terminal only after both Cmd release and the existing acknowledged-view/input boundary. A key release is never a program Keystroke.

Cancellation before selection preserves the original session and input mode. After selection, cancellation of pending typing leaves the chosen destination in Browse; it does not undo the navigation. An ordinary Cmd release after selection completes the gesture rather than cancelling its pending screen readiness. Esc/Ctrl-g during loading retains the existing Agent-switch cancellation behavior.

## Label allocation

- Number eligible, drawn title rows from top to bottom, including the current Agent. Include retained Agent rows; exclude archived/provisional rows, shells, Operator chats, and container headings.
- A title line must actually be drawn with room for the badge, status glyph, and title. A clipped metadata line whose title is offscreen does not receive a label. If there is no readable title-line space, assign no action there.
- With one through nine visible Agents, show `[1]` through `[9]`. Every label is one digit.
- With ten through fifty visible Agents, show `[01]` through the zero-padded visible count. Every label is two digits. For twelve Agents, choosing the first requires Cmd+0 then Cmd+1; choosing the twelfth requires Cmd+1 then Cmd+2.
- Choose the code length and identity/run mapping once at Cmd-down. Changes in count or activity do not change them until a fresh hold. Numbering follows sidebar order, not attention priority or session IDs.
- Consume exactly the displayed ASCII digits while only Cmd is held. The last digit selects immediately; there is no timeout, Enter, or release-to-select rule. With two-digit labels, show the first digit in the temporary footer (`Jump: 0_`) and accent only matching badges without moving or hiding rows. Backspace clears that prefix.
- Releasing Cmd with a partial code cancels without navigation. An unmapped complete code clears the prefix and shows a concise `No visible agent for <code>` hint; it sends no program input and allows a retry during that hold. An invalidated destination similarly refuses its captured label without reassigning it.
- A non-label Cmd shortcut cancels the unfinished gesture and retains its ordinary host/dashboard handling. Additional Shift, Control, or Alt modifiers cannot select a label. Never strip Cmd and dispatch its key as an unmodified Browse command or program key. Claimed host configurations must deliver the digit chords; this feature does not silently reconfigure host shortcuts.

## Layout

Examples show fewer than ten eligible Agents. `?` denotes a request for input; provider metadata may occupy its ordinary second line. For ten or more Agents, `[01]` replaces `[1]` within the same existing five-cell indent.

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
Cmd held · choose label · Esc cancel
```

At 80×24, the sidebar remains its existing 40 columns and one terminal pane uses the remaining 40. At 60×24 the sidebar is capped at 30 columns; only one terminal pane remains visible. Titles truncate by cell width and provider/model metadata follows the existing stacked-row rules. Holding Cmd must not change row heights or the terminal's advertised PTY size.

```text
60 columns
OVRCR                          [Commands]
OVRCR -----------------------| Review socket shutdown
  main                       | Waiting for input
 [1] ? Review socket shutdo… |-----------------------------
       └ codex:gpt-5.4        | Agent screen
 [2]   Fix sidebar clipping  |
       └ claude:opus         |
Cmd held · choose · Esc cancel
```

When the sidebar is hidden, leave the primary pane unchanged and show no labels. The ordinary sidebar toggle remains the route to reveal it. At narrow widths, the existing single-terminal-pane fallback remains; if the user hides the sidebar, the primary pane wins. In Operator's narrow layout, labels work only when its on-demand sidebar is already open. Below 60×16, suppress labels and show the existing Operator resize guidance when in Operator; any terminal-dashboard minimum remains an implementation validation item rather than a new global threshold.

Clutter audit: zero new borders; one non-color label cue plus one accent cue; no new idle markers. A three-cell badge replaces indentation, adding zero columns or lines. In a 40-cell sidebar that is 7.5% of each labeled title row, and 10% at 30 cells. With two-character codes the four-cell badge would be 10% and 13.3%. Use one temporary footer hint instead of repeating `Cmd+` on every row. Preserve status icons because they communicate a different fact.

## Input support

The production Dashboard's current terminal path cannot observe a bare Cmd hold/release. `ovrcr-gui` can, but is a disposable development helper. A demonstration there alone cannot satisfy production acceptance.

The [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) offers opt-in modifier and event-type reporting. A supported host must also deliver the chosen Cmd chords; protocol negotiation alone proves neither host key mappings nor propagation through a multiplexer. Kyle chose Cmd-hold only in verified setups, with the existing Agents search elsewhere (Q4). No particular host has been named or verified for this feature; implementation must establish the supported configurations before advertising the gesture there.

Any enhanced reporting belongs to the outer Dashboard input boundary and its existing terminal guard. Enable only negotiated modes, restore them on exit/error/panic and temporary handoff, and preserve ordinary program input through the shared encoder. Do not use global OS keyboard monitoring or a second Server/Bridge keyboard owner to bypass a terminal limitation.

## States and implementation acceptance

| State or transition | Expected visible/input behavior |
| --- | --- |
| Idle | Ordinary sidebar, no labels or reserved width |
| Sidebar hidden | No reveal, labels, layout change, or jump interception |
| Cmd held, eligible Agents visible | Fixed labels on title lines; no row movement |
| First digit of a two-digit label | Footer shows prefix; matching badges accent; no selection or program input |
| No eligible Agents | No labels; one concise `No visible agents` hint |
| Target chosen | Hide labels and latch one selection until Cmd release |
| Target chosen, view loading or Cmd still held | Selected destination and existing loading indication where applicable; no typing replay |
| Target ready and Cmd released | Enter Terminal once the exact view and input boundary are ready |
| Retained target | Show its retained view in Browse; no process launch |
| Destination disappears or changes run | Cancel/refuse that label; never choose its neighbor |
| Focus lost, modal opens, resize, scroll, disconnect | Remove labels; preserve ordinary mode/draft/session state |
| Modifier reporting unavailable | Keep existing Agents search; no new portable trigger or unusable hold hint |
| Too small | No clipped actionable labels; preserve work and show applicable resize guidance |

Implementation acceptance should exercise the production event dispatcher, shared row geometry, real session switch, Server view acknowledgement, and input drain. Cover already-focused and other-pane destinations, retained rows, duplicate titles, fifty total sessions, a clipped first/last row, activity updates, deleted/replaced targets, repeated label keys, both Cmd keys, extra modifiers, focus loss without release, and normal copy/paste shortcuts. Verify no shortcut bytes reach old/new programs and no Unread is reviewed.

Explicit allocation boundaries are zero, one, nine, ten, and fifty visible Agents, plus fifty total with only a subset drawn. Prove nine-to-ten count changes do not alter an active mapping, partial-code release does not navigate, an invalid code retries safely, and Backspace edits only the prefix. Cover readiness before Cmd release, release before readiness, cancellation after selection, same-session no-op readiness, and every hidden-sidebar path. Exercise the outer keyboard protocol through normal exit, interrupt, errors, panic, and temporary suspend/handoff; restore its mode stack as well as the existing terminal state.

Rendered checks need wide, 80×24, 60×24, and 60×16 frames, narrow sidebar widths, monochrome, CJK/combining/emoji titles, and no layout shift during hold. Production-terminal acceptance must prove Cmd-down, visible labels, a chosen destination with a unique acknowledgement, Cmd-up cancellation, and cleanup in the supported host. Test through any claimed tmux/SSH chain separately; keep unavailable configurations explicit. Native helper evidence supplements this proof.

## Confirmation and source

Kyle confirmed this behavior in Q8 on 2026-11-06. The later implement-spec request authorizes implementation; it does not authorize merging to main. The executable work graph is recorded in [specification #401](https://github.com/xlyk/ovrcr/issues/401) and tickets [#402](https://github.com/xlyk/ovrcr/issues/402), [#403](https://github.com/xlyk/ovrcr/issues/403), [#404](https://github.com/xlyk/ovrcr/issues/404), and [#405](https://github.com/xlyk/ovrcr/issues/405).

For computer-use verification Kyle directed use of the existing disposable OVRCR TUI testing GUI. Exercise the real Dashboard input boundary there, and keep evidence from other production terminal hosts separate. Implementation acceptance remains open until the actual checks are recorded.
