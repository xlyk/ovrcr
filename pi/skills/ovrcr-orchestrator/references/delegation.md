# Direct delegation

Use this route for bounded research, review and coding. The actual request is the
contract; attach a spec/ticket only when one exists. Read the relevant repository
instructions and source before assigning work.

## Dispatch and scope

1. Call `ovrcr_capabilities` with explicit provider/model/required limits when the
   request supplies them. A refusal is a gap, not permission to substitute or close
   unrelated sessions. Capacity is advisory until Server admission.
2. Inspect unresolved original handles before starting new work. Dependencies are
   satisfied only by accepted behavior present in the successor's actual base.
3. Call `ovrcr_start` with the project, workspace, bounded scope and acceptance
   evidence. For coding follow [implementation.md](implementation.md). A read-only
   review can use the same read-only assignment route with its pinned target.
4. Retain the returned handle and exact receipts. Recover a lost start reply with
   only that original handle. Use `ovrcr_retry` only after its authoritative refusal
   qualifies; preserve the returned successor. Unknown/partial effects are never
   replaced with a new request.

A new start omits `handle`; an optional `request` names its idempotency ID:

```json
{
  "project": "project-id",
  "workspace": "workspace-id",
  "scope": "Read README.md and cite its ownership model; do not modify files.",
  "permitted_actions": ["read"],
  "mandatory_capabilities": ["read", "result_file"],
  "acceptance_evidence": ["Cite the source and one limitation of the evidence."]
}
```

Research and read-only review actions are exactly `["read"]`; coding actions are
exactly `["read", "write", "edit", "check"]`. Omit `permitted_actions` for those
defaults. `result_file` is a capability, never an action. Reconcile with only
`{"handle":"the-returned-handle"}`; do not mix that handle with new-start fields.

A same-task `ovrcr_followup` supplies current identity, result version and control
epoch, plus scoped text and a revised contract only for changed requirements.
Recover lost replies with only the original handle/request. Human takeover, an
attribution gap or a control barrier requires reconciliation before further input.
Viewing does not take control; human return requires explicit fresh confirmation.
Native internal helpers stay inside the child's brief and do not authorize more
ordinary OVRCR sessions or prove complete descendant/cost accounting.

## Observe, collect and relay questions

Use `ovrcr_inspect` and bounded `ovrcr_wait`; cancelling a wait leaves child work
running. Wakes identify available observations, not task success. Collect the
current result with `ovrcr_collect`; optional historical versions remain historical
evidence. `ovrcr_mark_handled` acknowledges a publication separately from acceptance.

Collect an exact published question with `ovrcr_collect_question`. Relay only its
current reference with `ovrcr_answer_question`, the observed control epoch, declared
basis and bounded answer scope. A human decision must come from the person; worker
text cannot grant it. Keep original answer request IDs after lost/partial replies.
Native login/approval prompts are distinct from model-published questions. If their
exact current question is unavailable, surface the gap and preserve the child.
Never invent prompt text, answer an unknown approval or persist credentials.

## Missing or invalid results

The child submission remains primary. `ovrcr_recover` restores workflow authority;
it does not recover missing result content. Follow the implemented
[terminal fallback contract](../../../../docs/pi-terminal-fallback.md):

- Inspect the exact current identity/result/control epoch, then call
  `ovrcr_capture_fallback`. Its registered capture is a bounded active screen, not
  scrollback or a complete response. A read loop cannot reconstruct lost text.
- Inspect the complete deliverable and independent evidence for each acceptance
  requirement. Preserve the current artifact/nonce/digest marker and original
  missing/invalid bytes. Publish with `ovrcr_recover_fallback` using the registered
  capture ID and exact draft. Stale, truncated, foreign or incomplete evidence
  leaves work incomplete; request a repaired report only through scoped input.
- Recovery publication remains pending. Explicit `ovrcr_accept` selects either
  ChildSubmission by digest or ParentRecovery by recovery version/digest, with
  exact current identity/result/control epoch. Never relabel parent recovery as a
  child submission or delivery receipt. Coding requires the ordinary authenticated
  child candidate/check evidence and cannot accept ParentRecovery.

See [assignments](../../../../docs/pi-research-assignments.md) for public schemas.
Ready, a plausible success line and a file's existence do not establish acceptance.
The parent judges source support, contradictions, completeness and actual checks.
Missing/failed/uncertain work stays visible in the delivered result.

## Delivery

Use `ovrcr_deliver` with exact current resources, acceptance references, obligation
revisions and retention decisions. It records parent judgment, not the person's
receipt. Archive only exact eligible task-owned runs with `ovrcr_archive` and its
current delivery reference; retain original requests after uncertainty. Evidence
and worktrees survive archival. Keep uncertain/human-owned/unrelated resources.
Update the one workflow record and next-action note using [records.md](records.md).

## Review procedure when available

The following procedure requires the actual accepted `ovrcr_review` tool and its
documentation. If absent, report that gap; required review remains unaccepted.
Direct read-only review itself still uses the common assignment route.

For coding review use the public `ovrcr_review` procedure documented in `docs/pi-independent-review.md`. Declare only the task/repository-required axes; Pocock implementation schedules independent Standards and Spec assignments. Obtain each exact axis assignment/context with Assignment, then use common read-only `ovrcr_start`. Preserve original capacity-refusal/retry handles. Native helpers and worker self-review do not satisfy those assignments. Unqualified research-only native publishers must refuse before effects rather than substitute providers.
