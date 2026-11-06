# Team review and stronger Operator alternatives

2026-11-05. Three independent read-only reviewers used Luna at max effort, each with the current review, preview source, relevant product contracts and tui-design guidance. Their assignments covered main chat, sidebar/navigation, and onboarding/recovery. The root agent synthesized their findings and created a new comparison. No product implementation or tracker changes were authorized or made.

## What the reviewers changed

| Reviewer | Strongest finding | Response in the new designs |
| --- | --- | --- |
| Main chat | The initial preview replaced the real two-to-six-row composer with one line, repeated title/actions, and spent too much of the floor on chrome. | Preserve a growing multiline composer; combine context and its action on one row; put Send in the footer; suppress the repeated chat title when its sidebar row is visible. |
| Sidebar | The picker clamped selection to index 3 even with more results; 60×16 displayed only one result; Active ambiguously meant unarchived. | Use actual filtered-list bounds and a scrolling viewport. Three two-line rows fit at 60×16. Compare an inbox with Review, Errors, Working and All rather than claiming all open chats are active work. |
| Recovery | Connect and Stop immediately appeared successful; context could be mistaken for a permissions boundary; archive and retry were too optimistic. | Show sign-in handoff and a separate account-status check; distinguish signed in from model entitlement; Stop enters Stopping, and unconfirmed stop keeps Send unavailable. Archive branches by state. Reuse restores exact failed input without submitting. |
| All three | The original carousel showed screens in one flow, not competing design choices. | Compare three different navigation/layout models with shared recovery and input behavior. |

The first preview's errors above are mockup defects, not evidence that the corresponding production behavior is broken. The real app already has stronger stop-proof and input-preservation contracts.

## Options to compare

### Refined journal — recommended first

Keep the approved shared sidebar above Projects, with one-line conversations and the Working journal. Reduce repeated controls; keep context actionable above a growing composer. A temporary All chats picker supplies search and rename. Ctrl+O remains the first-message fast path; provider/model options belong inside creation, preserving the separate manual-creation concept.

- **Best for:** alternating between a few Operator chats and terminal sessions.
- **Gain:** resolves connection/context/discovery problems with the least spatial relearning.
- **Cost:** many chats still need a picker; the short sidebar is not a complete conversation library.

### Attention inbox — a larger navigation alternative

Operator opens a searchable review list instead of a selected chat. Destinations distinguish replies to review, errors, running work, all conversations and archives. Opening a row enters the journal; the shared sidebar can show category destinations instead of individual chat names.

- **Best for:** many concurrent conversations and repeatedly checking what changed.
- **Gain:** exposes actual review/working/error states with readable labels; more room for full titles and workspace context.
- **Cost:** switching between two known chats takes an extra step; loses the familiar glanceable conversation list. It changes the selected sidebar contract and needs explicit design agreement before implementation.
- **Guard:** Review means unseen replies; Errors means recorded failures. Neither category claims the model has asked a question. Freeze ordering during navigation and retain selection by conversation ID when observations arrive.

### Focused conversation — a reading/composing alternative

Hide the Projects/conversation sidebar while inside Operator, including at wide sizes. The journal owns the full width; Chats opens a searchable drill-down and Dashboard returns to the terminal view. Context and conversation actions remain local to the journal.

- **Best for:** long requests, command output and reading complete answers.
- **Gain:** one stable spatial model from wide terminal to 60-column split; less navigation chrome in the conversation.
- **Cost:** less background-session awareness and more trips through Chats/Dashboard. This is an optional mode proposal, not a replacement for the already-selected shared sidebar.

The recommendation is to take the Refined journal improvements first, borrow the inbox picker when history grows, and only add a persistent attention-navigation model after users demonstrate that need. A permanent additional rail was rejected.

## Shared behavior and states

- Context is for the **next message**, including starting workspace/terminal metadata; it does not limit Bash access. Full context opens on demand; previously submitted context remains immutable.
- Sign-in proceeds through a browser handoff. An account-status check can establish signed-in state but cannot establish model entitlement. Sending remains an explicit separate action, and the exact draft is retained.
- Stop enters Stopping. Unconfirmed stop preserves history/draft and disables sending. Archive waits for controlled stop and acknowledgement; failures retain the conversation. A local mock cannot prove any worker stopped.
- An existing chat says Send request; New chat creates a separate conversation only on explicit first-message submission. Drafts remain separate across switching and the new-chat form.
- Error input reuse copies the saved failed request into the editor; it does not replay commands or send automatically.
- Command call and output remain separate disclosure controls.

## Floor and clutter audit

```text
Wide / 120 columns                     Narrow / 60 columns
OVRCR            New chat Dashboard    OVRCR New chat Chats Dashboard
OPERATOR All chats | ChatGPT / model   Auth handoff [Conversation]
> Auth review      | You               You: Which sessions need attention?
  Pipeline         | Which sessions?   [Tool call] List sessions
  Cleanup          | [Tool call]       Auth review needs input...
PROJECTS           | [Tool output]
  consigint        | Operator          [Context] consigint / auth-handoff
                   | Auth needs input  ----------------------------------
                   | [Context] ...     > first draft row
                   | ------------        second draft row
                   | > draft           Send / Newline / Chats
```

- **120×28 / 100×28:** Refined journal retains the shared sidebar; inbox uses a list; Focused conversation stays full width. No third pane.
- **80×24 / 60×24:** chat wins; lists/context/actions become drill-down views. List titles truncate while the selected context remains available.
- **60×16:** two draft rows remain; the conversation list shows three two-line results, with scrolling to later identities. Starters, repeated provider metadata and secondary receipts yield first.
- **Below 60×16:** name the actual minimum and preserve work. Do not squeeze into an unreadable grid.
- **Border depth:** zero nested product boxes; one shared sidebar divider and one composer rule. A single terminal outline only frames the preview.
- **Repeated signals:** no per-row decorative icons. The example sidebar has a selected marker and a working marker; ordinary entries have neither. Inbox states use words plus optional color.
- **Chrome reservation:** wide journal reserves two full-width navigation/footer rows plus three 91-cell local rows (metadata, context, rule): 513/3360 cells, **15.3%**. At 60×16 the same five rows occupy **31.3%**, with two editor rows preserved. This accounting excludes the useful sidebar navigation data; it is a frame-space calculation, not a measured user-efficiency score.

## Verification and remaining limits

- 648 combinations checked frame row/column bounds and input bounds: six dimensions × three alternatives × six conversation states × six overlays.
- Additional assertions checked draft isolation across chats, retaining a new-chat draft, sign-in handoff without submission, exact failed-message reuse path, Stop remaining blocked, multiline growth, and selecting the eighth conversation in the scrolling 60×16 list.
- Browser inspection exercised a three-line draft and switching away/back without losing it; inspected the distinct review inbox and its category labels. Evidence: `team-refined-journal.png` and pinned `team-preview-frames.txt`.
- These are ASCII-grid mock checks. They do not prove native cell-width handling, Unicode, terminal lifecycle, OVRCR shortcuts, model access, stop verification or real large-list performance. Native input shortcuts are design hints; the mock implements only a subset.
- The mock intentionally leaves active Stop/archive pending rather than inventing completion. Choosing a conversation-state preset is a simulation, never process-stop evidence. Copy/session/Dashboard controls demonstrate intent without changing the user's clipboard or app.
- Remaining design detail: Grok's device-code handoff, real auth cancellation/failure, identity-stable asynchronous reordering, paged history and successful pending archive acknowledgement are not fully prototyped.
