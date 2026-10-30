# Operator inline activity and input editing

Baseline: `5e79a744aab8d5a37aa79bb18794bd55d3f20bfa`. This follow-up implements the user's explicit correction to activity placement and their request for common composer commands. It supersedes only the previous activity strip's placement.

## Intended experience

The Operator manages OVRCR workspaces and sessions. Keep the approved Working journal, existing theme tokens, sidebar and quick-message behavior. Show the existing Sending / Responding / Waiting / Stopping / Closing / Stop unconfirmed state immediately after the latest durable message or tool activity, inside the transcript. Restore the entire context/error row above the composer. The indicator is one clipped row, using the existing animated glyph and cadence; it disappears under the existing stopped-proof rules. Scrolling to older messages naturally scrolls it away, and returning to the tail reveals it. Never pin it to the bottom of the terminal or force the reader to follow.

At wide sizes the sidebar and journal retain their current geometry. At 80×24 and 60×16 the journal retains the same responsive fallback, editor capacity and minimum-size contract. The indicator uses one transcript row without acquiring a durable message ordinal, selection, disclosure or review authority. Three durable viewport rows remain required for review acknowledgement. No duplicate activity display in the context row, extra border or repeated label is added.

## Composer commands

Apply these only to editable full Operator message input and editable Quick Message:

| Key | Behavior |
| --- | --- |
| Ctrl+A / Ctrl+E | Beginning / end of the current logical line |
| Ctrl+B / Ctrl+F | Previous / next grapheme |
| Ctrl+H / Ctrl+D | Delete previous / next grapheme; empty Ctrl+D is a no-op |
| Ctrl+U / Ctrl+K | Delete to beginning / end of the current logical line, retaining newline boundaries |
| Ctrl+W | Delete preceding whitespace and preceding whitespace-delimited word within the logical line |
| Alt+B / Alt+F | Previous / next whitespace-delimited word boundary within the logical line |
| Ctrl+C | Clear a literally non-empty draft (including whitespace). With an empty full composer, a separate Press stops only the selected exact response through existing Stop behavior. Empty Quick Message is a consumed no-op and never stops its underlying chat. |

The Ctrl+C policy was explicitly chosen by the human. Repeat/Release Ctrl+C must be consumed without clearing or stopping; holding the key cannot turn a clear into Stop. Creating/Submitting Quick Message and closing chats stay frozen. Preserve existing Ctrl+X, Enter, Shift+Enter, Home/End whole-buffer behavior and Up/Down logical-line movement. Unsupported modifier combinations must not insert stray letters; Ctrl+Alt is not a Ctrl-only shortcut. Other editors, overlays and terminal PTY encoding retain their current behavior.

## Approach and constraints

Use the canonical transcript row pipeline with optional durable anchors rather than a second status widget or fabricated message. Reuse `conversation_activity`, existing spinner and shared draw timestamp. Keep current business/runtime/protocol state unchanged. This is the smallest approach that obeys the user's inline correction and preserves history navigation.

Extend TextCursor with a modifier-aware entry point used only by the two message editors; reuse its existing grapheme operations. Keep its current `key(KeyCode)` contract for other callers. No selection model, kill ring, yank, undo/history/completion, new dependencies, configuration, provider or global keymap changes. Routine line/word semantics in the table are controller decisions, not additional human choices.

Never mutate frozen Submit/Quick payloads, queued requests, another chat's draft or run. Editing produces no PTY bytes, Shutdown or Detach. Empty full-composer Ctrl+C uses the existing exact-identity Stop path and deduplication. Quick retry edits must retain existing seeded/unseeded draft synchronization.

## Acceptance

Meaningful regression tests must fail behaviorally on baseline where practical, then pass through owning render, public Dashboard and production dispatch paths. Cover wrapped Unicode anchors, scrolled history, transient-row interaction, current-turn tool matching and completion removal; both theme families and the existing width ladder. Cover multiline and Unicode edits, repeat safety, frozen payloads, overlay priority, exact selected Stop, independent chat and ordinary terminal Ctrl+C.

Run the actual reviewed checkout's disposable native GUI with CUA. Capture inline silent and pending-tool animation pairs, history scroll-away/return, retained context/draft and completion disappearance. Exercise full and Quick editing and Ctrl+C outcomes using real input. Keep source revision, screenshots, AX, original owner waits and cleanup receipts. Headless and native evidence are separate. Update affected user/developer docs, the existing draft PR and the work diary. Preserve historical failures and evidence; no merge or installed-binary replacement.
