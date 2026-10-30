# Operator layout design

Design snapshot from 2026-10-30. These mockups preserve the comparison that led
to the native Original layout, followed by the selected Working journal refinement.
Browser previews remain design material; native verification has separate gates.

Operator manages OVRCR projects, workspaces and sessions. The examples use one
orchestration request: reuse a workspace, start Claude and Pi, and surface a
question from Pi. All displayed sessions and operations are simulated.

## Preview

Open [index.html](index.html) in a browser. The named picker compares nine layouts.
Try the sidebar collapse heading, hover `[x]` controls, session links, and the
**New Operator ^O** action. Ctrl+O works while the preview has keyboard focus.

[mockups.fragment.html](mockups.fragment.html) is the editable source used by the
inline Codex preview. `index.html` is its standalone export.

## Sidebar requirements

- Put Operator and its conversations above Projects in the existing left sidebar.
  Remove the separate conversation-navigation column from the main area.
- Each conversation occupies exactly one line: status slot, title and the
  existing hover `[x]` close control. Keep the control keyboard-reachable too.
- Reuse existing sidebar conventions: green busy spinner, teal unread `✓`,
  normal-weight titles, selected-row background and a leading mauve `▌`.
  Reviewing an unread reply clears its mark; collapsing the section does not.
- Make the Operator heading collapsible. Match the blue uppercase heading,
  muted rule and collapsed `▸ count` used by project headings.
- Omit the conversation's model from the sidebar. Preserve drafts, targets,
  scroll state and background work when changing the active view.
- `[x]` follows normal session close: confirm active work, stop only this
  conversation's owned work, then archive it while preserving history. An unsent
  draft also requires confirmation before it is discarded. Failed closure keeps
  the conversation and draft available. Restore through Archived sessions.

## Conversation area

Use a text-first transcript, compact activity summaries and a prompt that grows
with its draft. Show the captured workspace/session near the composer. Keep
paths and shortcuts out of repeated message boilerplate.

Kyle selected **Original layout** for implementation on 2026-10-30, then selected
**Working journal** to refine its appearance using the current theme. The native
renderer now reserves a two-cell transcript gutter, guides assistant/tool activity,
marks user prompts, and gives the borderless editor quiet rules. Adjacent call and
output receipts retain independent disclosures and full expanded details; the
prototype's exploratory single combined receipt is deferred. The local header
avoids repeating the request while the sidebar is visible and retains a clipped
chat title when hidden. Only Operator quiets the global title fill.

The [selected refinement spec](../../../superpowers/specs/2026-10-30-operator-working-journal-design.md)
is the native target. The other layouts remain comparison material. The original
five pinned native frame filenames are retained for the intentional journal update;
this does not claim fresh GUI, Linux, or live-provider acceptance.

| Layout | Main distinction |
| --- | --- |
| Original layout | Borderless conversation with compact action receipts |
| Quiet | Sparse transcript and inline session links |
| Action ledger | Ordered orchestration events and their targets |
| Workspace brief | Workspace outcome, session rows and an open decision |
| Dispatch console | Compact chronological dispatch receipts |
| Parallel launch | Separate progress lanes for the launched sessions |
| Session window | Conversation with a selectable session-output preview |
| Target dock | Workspace and session context beside the conversation |
| Current request | Earlier history compressed around the active request |

At narrow widths, the conversation wins and the combined sidebar opens on demand.
The design baseline includes 80×24 and a 60-column split. Proposed minimum:
60×16; below it, show the required dimensions while preserving drafts and work.
Browser previews do not establish these native-terminal acceptance gates.

## Global quick message

The requested shortcut opens a dialog to send the first message directly to a
new Operator conversation, from anywhere within OVRCR. **Ctrl+O is implemented**
and reserved for OVRCR while a child terminal has focus. The native form now starts
at seven rows, grows with wrapped draft/error content up to sixteen, and remains
at most eighty columns wide. Modal click consumption and create/submit ownership
are unchanged.

- Capture and display the current workspace/session when the dialog opens.
- Use the configured Operator provider/model defaults, without a setup step.
- Enter creates the conversation and sends its first message; Shift+Enter adds
  a newline. Whitespace alone cannot submit.
- Before submission, Esc cancels without creating an empty conversation and
  restores the previous view and its draft. After Enter, Esc dismisses the dialog
  while the accepted create-and-send operation continues once. Late replies do
  not steal focus. Other conversations keep running.
- Insert the submitted conversation as one busy sidebar row and open its view.

Native implementation must preserve underlying dialogs and confirmation state.
It must retain the first message and captured context through Create, submit
once after the acknowledged conversation identity arrives, and retain input on
failure. The preview's close controls only remove local simulated rows; native
close/archive must use the existing lifecycle/history guards.

The [implementation plan](../../../superpowers/plans/2026-10-30-operator-layout.md)
maps these requirements to the native code and acceptance checks.

## Verification

The preview checks cover all nine layouts: session navigation, separate drafts
and targets, unread handling, collapse/close, quick-dialog keyboard flow,
cancellation, multiline submission and narrow browser widths down to 320px.
They do not prove native TUI behavior or provider execution.
