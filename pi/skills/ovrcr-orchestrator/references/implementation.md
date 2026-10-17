# Bounded coding and review

Apply this to a direct coding request or approved implementation work. Direct
coding uses the request and acceptance criteria; it requires no earlier Pocock
phases. Use [delegation.md](delegation.md) for common control and collection.

## Prepare the assignment

Pin the real repository/base, authorized paths, named required checks, exclusions,
provider/model constraints and required review scope. Verify accepted dependency
changes are present in that base. One implementation owner controls one task's
isolated workspace and its same-task follow-ups.

Call `ovrcr_start` with a new workspace ID and `coding` containing requested_base,
branch, authorized_paths and required_checks. The Server creates/reconciles the
workspace and records process proofs. A created worktree is not proof a process
started or failed; inspect resources before a qualified `ovrcr_retry`.

The current coding contract permits read/write/edit/check. It grants no commit,
push, merge, deploy or credential authority and is not an OS sandbox. For no-commit
work retain the complete immutable candidate, including authorized untracked files;
a HEAD-only review would omit the work. A separately authorized delivery owner may
later commit the reviewed candidate under repository rules.

## Implement and assess

The child follows task/repository methods, runs the actual named checks and
publishes actual Passed/Failed/Missing/Unrun outcomes with commands, exits and
evidence. `ovrcr_publish` lets the Server attach the authenticated complete
candidate; a worker-written candidate or parent fallback cannot replace it.
Collect without accepting. Preserve missing coverage and unresolved native prompts.

Follow the requested/repository independent-review scope. Pocock implementation
uses its installed code-review procedure and independent Standards and Spec axes;
direct coding adds only the axes its request/repository requires. Pass reviewers
the exact complete candidate and actual base. Standards receives real standards;
Spec receives the originating brief/spec. Native helpers and worker self-review
are separate evidence. If required review is unavailable, leave work unaccepted
and report that gap.

Return precise blocking findings to the same safe implementation conversation,
retaining identity, current control epoch and original corrective request. Use the
applicable finite limits/provenance, inspect new evidence and stop for exhausted
or stalled repair, unanswered decisions or missing required checks. Re-review the
changed candidate and affected requirements. A changed brief invalidates only the
affected acceptance/obligations and dependents.

## Accept and deliver

Explicit acceptance identifies the current authenticated child result and complete
candidate with actual checks. ParentRecovery cannot satisfy coding acceptance.
Keep submitted, accepted, available in a successor's base and delivered distinct.
Validate the actual integrated revision under repository gates; per-task checks
are not final integration/native/provider proof. Publication and merge permissions
remain those of the user/repository, not a workflow status.

Declare delivery and retention only with current obligations. Archive exact owned
runs after delivery; keep versioned evidence and follow repository worktree policy.

## Review/repair procedure when available

Apply the following with the actual accepted `ovrcr_review` procedure. Its absence
is a capability gap, not a requirement to import unfinished code or self-approve.

Pin the authenticated complete coding candidate and actual base, including untracked changes when commits are prohibited. Supply actual Standards documents plus the installed method only to Standards; supply the originating Spec/direct brief only to Spec. Collect attributable exact-axis reports. Route blocking findings through Repair to the same safe implementation conversation using one original request/control epoch. Omitted later limits inherit the original applicable policy/provenance and consumed rounds; the coordinator default of three is recorded as a default. Stop for exhausted/stalled repair, missing required checks or unanswered decisions. Optional check outcomes stay visible for parent judgment.
