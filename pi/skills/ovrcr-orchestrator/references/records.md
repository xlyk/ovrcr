# Workflow records and continuation

Keep one concise parent-owned workflow record and one short continuation note.
Choose a local Markdown/JSON index appropriate to the task; direct research needs
no empty spec/ticket/review fields. The record links the objective, decisions with
source/scope, tasks, open questions and acceptance references. The note names the
next useful action, prerequisites and original sources.

## Existing owners

| Evidence | Owner |
| --- | --- |
| Live sessions, current source/controller/control epochs, process/capacity and receipts | OVRCR Server and its existing SessionStore |
| Original intent and tool outcome | Existing private tool journals and Server receipts |
| Contract/brief, current result and coding candidate | Existing immutable assignment/version files and authenticated publications |
| Published question and scoped answer | Existing exact question references and answer receipts |
| Fallback | Registered Server capture and separately attributed immutable parent recovery |
| Objective, decisions and evaluation | Parent's one workflow record and linked evidence |
| Continuation | Parent's short note plus bounded checkpoint index |

Use the actual returned paths/references. Illustrative draft filenames are not a
new operational store, process table or authority. Preserve original versioned
contracts/results/recoveries separately; concise summaries point to them.

## Save and resume

Refresh the workflow record when a decision, scope, acceptance or next action
changes. Refresh the continuation note when its next action changes and before
compaction/handoff. Call `ovrcr_checkpoint` with its bounded index of objective,
decisions, unresolved assignments, questions, receipts, accepted evidence and next
actions. Keep transcripts, credentials and private reasoning out of that index.

On startup/resume use `ovrcr_recover` to reconcile existing workflow authority;
select an explicit workflow when discovery is ambiguous. Read the original sources
and inspect outstanding handles before input. A copied pointer or fork grants no
control; use a distinct workflow or explicit supported transfer. Recovering a
parent never silently resumes stopped children or replays uncertain requests.

Keep observations, publication, handling, acceptance and delivery separate. Editing
a record cannot dispatch, answer, take control or accept. A missing wake is not
missing work. Historical publications/acceptance remain evidence, not a current
result for a changed version. Current source/child ownership and receipts govern.

Keep evidence after owned sessions archive. Archive, provider-history retention
and worktree removal are separate decisions; uncertain resources remain preserved.

## Review records when the review procedure is available

Keep immutable review declarations/source documents, exact candidate/source targets, separately attributed reviewer publications, finite policy provenance/counts and original repair receipts in their existing owners. Actual source prose/findings/corrective text remain private task artifacts; bounded refs/digests/policy stay retained metadata. Changed review requirements advance the existing obligation revision and clear current acceptance while preserving old immutable acceptance bytes. Explicit reacceptance must pass current gates; older delivery declarations cannot satisfy changed obligations. Keep worker checks, independent review and actual integrated-revision validation separate. None grants publication, push or merge authority.
