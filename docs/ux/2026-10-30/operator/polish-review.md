# Operator chat polish review

2026-10-30 follow-up to the fresh native screenshots at product-equivalent head
`f2c7d6b0`. This is an aesthetic review and interactive proposal, not an implemented
native change. Preserve the selected Original structure and the user's existing
sidebar/lifecycle requirements.

## What the screenshots show

- Transcript border depth is0; the quick dialog's depth is1. More boxes are not the
  answer. The main problem is flat hierarchy and administrative repetition.
- In the short concurrent fixture's main panel,141 of247 nonspace characters (57%)
  are header/context/role/prompt metadata. This is a measurement of that fixture,
  not a whole-product average. The request appears in three places, the target in
  two, and one tool ID appears twice while collapsed.
- The composer is a bare prompt at the bottom with little visual distinction.
  A sparse reply leaves a large gap between reading and writing.
- Paired call/output disclosures expose transport IDs; the fixed16-row quick
  dialog is much larger than a two-line first message.
- Synthetic `A before tool` / `QUICK final` test copy is intentionally terse.
  More realistic orchestration examples help assess the design, but do not erase
  the hierarchy, spacing and input-affordance problems.

## Recommended direction: Calm conversation

Keep the combined sidebar and one-line chats. Give the transcript a consistent
two-cell gutter, subtle user-message surface, normal-weight assistant prose and
one blank row between logical turns. Stop repeating the full request as a main
header. Keep current target near the composer and historical context expandable.
Retain the88-cell prose cap already present in source.

Use the existing theme tokens throughout; add no palette or global theme changes:

| Existing token | Proposed role |
| --- | --- |
| BASE / CRUST | Conversation canvas and quiet supporting chrome |
| SURFACE0 | Writing surface and selected sidebar row |
| SURFACE2 | Thin separators and subtle structure |
| TEXT / SUBTEXT | Primary prose and secondary metadata |
| MAUVE | Prompt, selected marker, focused controls |
| BLUE | Section hierarchy and workspace/session references |
| TEAL / GREEN | Completed outcomes and active work |
| YELLOW / RED | Questions and failures, paired with explicit text |

Make the composer an obvious borderless writing surface with a single top rule,
one-cell inner padding, compact target breadcrumb and contextual hints. Grow the
quick dialog with its message up to the existing limit. Keep Enter/Shift-Enter,
Ctrl+O, dismissal and captured-target semantics unchanged. Never imply queuing
while a chat cannot accept a new turn.

Hide raw tool IDs until expanded. A combined call/result presentation is proposed,
but it must preserve durable ordinal anchors, running/failed/interrupted states,
expansion, complete raw details and independent scrolling. Do not manufacture
structured workspace/session receipts from arbitrary Bash output. The preview's
session summary is assistant prose, not a new typed operation protocol.

## Alternatives shown for comparison

- **Working journal:** a fine left guide joins assistant/tool activity and the
  composer uses rules rather than a filled surface. More operational; slightly
  more visible structure.
- **Prompt focus:** short exchanges sit near the composer and the editor is the
  strongest surface. Less empty separation; bottom alignment would require care
  around follow-tail and durable history anchors.

The controls and all displayed outcomes in the comparison are simulated. They do
not prove native rendering or lifecycle behavior. Local prototype checks covered
carousel, disclosure, quick editor/dismissal and narrow browser layout; native
implementation and acceptance have not started for this refinement.

## Terminal floor

At80×24 the conversation wins; the same sidebar is summoned on demand. Keep one
header, one target line, a two-row empty editor and a compact hint line; spend the
remaining rows on messages. At60×16 keep the same single-pane contract and remove
optional vertical padding. Model/secondary hints shorten before user content.
Below60×16 retain drafts/work and show the existing resize guidance. Do not
confuse the responsive browser comparison with a verified terminal-size test.

```text
Wide                                             60-column split
OPERATOR ──────── [+] │ Operator    model          OVRCR Operator     ^O New
▌ Release reviewers  │                           Operator           model
✓ Inspect sessions   │ ❯ Reuse the workspace…     ❯ Reuse the workspace…
PROJECTS ─────────── │
  spacelift-agent    │   Reused release-check.      Reused release-check.
  …                  │   Claude #42 is running.     Claude #42 is running.
                     │   Pi #43 needs a choice.     Pi #43 needs a choice.
                     │ ▸ Bash · completed         ▸ Bash · completed
                     │
                     │ ────────────────────────   ─────────────────────────
                     │ target / workspace         target / workspace
                     │ ❯ Message Operator…        ❯ Message Operator…
                     │ Enter send · ^G browse     Enter send · ^G browse
```

## Inspiration and scope

[Claude Code](https://code.claude.com/docs/en/interactive-mode) documents compact,
expandable tool details; [Pi](https://github.com/earendil-works/pi/tree/main/packages/coding-agent#interactive-mode)
separates messages, editor and footer. [Codex](https://github.com/openai/codex) and
[Grok Build](https://x.ai/build) are the other requested terminal-agent references.
The proposal borrows their readable conversation/editor emphasis while keeping
Operator's project/workspace/session role. It does not copy another agent's keymap,
add coding-only panels, invent usage metrics or replace OVRCR's global palette.

The known stale busy-error after close/restore remains a separate correctness
finding from the native pass. No code/test/app/PR mutation is part of this review.
