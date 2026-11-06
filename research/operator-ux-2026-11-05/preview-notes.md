# Operator design previews

2026-11-05 follow-up to the current-state review. Design only; no app implementation or approval implied. Uses the tui-design visual and interaction guidance while preserving OVRCR's existing full-screen Dashboard, shared sidebar, conversation journal and shortcuts.

## What to review

- **Guided start:** connection status before submission, request starters, explicit workspace target, draft retained while connecting. New chat is named consistently rather than Quick message.
- **Working chat:** existing compact conversation with visible Chats/New chat controls, next-message target selection, expandable command details, clear archive wording, and failed-input reuse.
- **Find conversations:** full titles and workspace context in a searchable list, Active/Archived, Rename and Restore. This is a drill-down view rather than another permanent panel.

The dominant loop is choose chat, inspect target, compose, send, inspect reply/tool evidence, switch back to work. The common actions are New chat, Chats, Change target, Send, Stop, inspect details and Archive. Connection is a first-use/recovery action.

All preview data is fictional. Connect, Send, Stop, Archive, Restore and Copy are local demonstrations. No account connection, command execution, clipboard write, or app mutation occurs. Product shortcut hints describe intended behavior; the preview implements native Tab/button interaction, text entry, Enter on inputs, Escape on inputs and search arrow selection, not OVRCR's full terminal key dispatcher.

## Wide and floor sketch

```text
Wide / 120 columns                     Floor / 60 columns
OVRCR       [New chat] [Chats]          OVRCR [New chat] [Chats] [Browse]
OPERATOR       | Review auth handoff    Review auth handoff
> Auth review  | ChatGPT / model        ChatGPT / model
  Pipeline     |
[All chats]    | You                    You: Which sessions need attention?
               | Which sessions...?     Two sessions need attention. [Details]
PROJECTS       | [Command details]
consigint      | Two sessions...        Target: consigint / auth-handoff
  auth-handoff | [Copy reply]           [Change target]
               | Target: ...           ----------------------------------
               | [Change target]       > Message
               | > Message             [Send]
Enter Send  Ctrl-l Chats ...            Enter Send  Ctrl-l Chats ...
```

Exact pinned ASCII frames for 120×32, 80×24, 60×24 and 60×16 are in `preview-frames.txt`.

## Responsive behavior

| Size | Behavior |
| --- | --- |
| 120×32 | Shared Operator/Projects sidebar and conversation; no third pane. |
| 100×28 | Same spatial model with a 29-cell sidebar in this mockup. Actual implementation should retain the user's configured width. |
| 80×24 | Conversation wins the full width; Chats opens a drill-down list. Model and target remain visible. |
| 60×24 | Same single-pane model; prose wraps, list fields truncate; target selection and conversation management use full-width drill-down views. |
| 60×16 | Short result summary; secondary receipts/starters/copy controls yield to the composer, target and recovery actions. Full evidence remains an intended details view. |
| Below 60×16 | Truthful too-small message, Browse action, explicit statement that work/drafts are retained. No broken grid. |

The preview size control offers these sizes and a deliberately too-small 50×12 case. When the host itself is narrower, the effective column count decreases without shrinking text below its product font floor. The fixed dark palette deliberately matches the reviewed OVRCR theme; a monochrome option checks that labels retain meaning.

## State coverage

- Initial/empty: Guided start, request starters and editable empty draft.
- Disconnected: connection status plus Connect, with retained input.
- Loading: saved-conversation loading message.
- Working: explicit work status, Stop and preserved next-message draft.
- Success: response summary with command disclosure and copy affordance.
- Error: retained partial evidence and Reuse request; no automatic replay.
- Archive: plain-language confirmation, history retention and accepted-operation caveat.
- Too small: named 60×16 requirement.
- Stop unconfirmed/stale server data: not separately mocked; implementation must retain the current fail-closed stop proof and must not treat a stale summary as ready.

## Clutter audit

- Border nesting depth: **0** in normal views; one sidebar divider, one composer rule. No transcript frame or nested dialog boxes. Selection screens replace the main view.
- State signals: at most one non-color label/marker plus its color. Sidebar uses a marker only for selection or ongoing work; ordinary rows have no glyph.
- Removed labels: raw conversation/session/run IDs from the default view, repeated provider/model on sidebar rows, duplicate local Operator title, generic Confirm wording.
- Fixed chrome reservation: at 120×32, two full-width rows plus six 91-cell main-pane rows occupy **786/3840 cells (20.5%)** for navigation, title/model, target/action, rule and send action. This deliberately counts reserved blank space in those rows. At 60×16 the same eight rows occupy **50%** of the grid: the floor is operable, but is a compromise for reading long answers. The sidebar contains useful navigation data and is not counted as decorative chrome.
- Tradeoff: visible controls cost space. The conversation library stays temporary, and starters disappear at the floor. No permanent inspector, extra right rail, or new dashboard metrics.

## Verification

- Executed 360 frame combinations: six dimensions × three starting screens × five conversation states × four overlay states. Checked exact row/column counts and input bounds.
- Checked draft preservation through Connect, Send transition, target change, Archive/Cancel and 16-row composer/context/footer placement.
- Inspected the rendered preview in the browser. Clicked a starter and Connect: input remained populated; changed next-message target; filtered the library with `auth`: two matching conversations appeared.
- Corrected an initial blank-row width assertion failure before delivery. No OVRCR Rust tests, real provider, PTY lifecycle, screen-reader, or production integration checks apply to this design mockup.
- Browser preview used a temporary loopback server; it was stopped after inspection. The inline preview is self-contained and does not depend on that server.
