# Terminal fallback evidence

Implementation status: the registered Pi tools, Server routes and private-file
validation are implemented. Focused production-path tests passed on earlier
provisional revisions using isolated binary Servers and PTYs. The current
accepted-stack integration awaits compilation and same-revision acceptance.
Native and installed-provider acceptance remain separate, unverified gates.

The child result file remains the primary result. Fallback produces an immutable
parent recovery when a bounded complete deliverable can be independently checked.
The recovery has no acceptance or dispatch-receipt field. Parent acceptance is a
separate explicit decision under current workflow authority.

Fallback evidence does not launch or retry a child and does not add a dispatch
receipt. It must preserve the accepted execution accounting and zero-effect retry
rules. The Server supplies the authoritative current result and validates its
reservation history. Result numbers can have reserved gaps; fallback does not
infer the current version from a history length or reuse a rejected version.
Mandatory limits and retry provenance remain covered by the exact contract digest.

## Ownership and provenance

| Item | Owner and meaning |
| --- | --- |
| Current assignment, result version, reporting binding and child control | Server authority; private file edits cannot change them. |
| Contract and brief | Existing immutable version publication; retained exact-byte digests identify the authoritative files. |
| Fallback artifact | Child-provided evidence; its presence does not prove child authorship or task completion. |
| Independent evidence files | Relevant authorized bytes inspected independently by the parent and copied into the assigned private directory. |
| Evidence assessment | Authenticated current parent's assessment of how each exact excerpt supports its acceptance requirement. |
| Capture | Server-produced active-screen evidence under current authority and reporting/control fences. |
| Recovery | Current parent publication with immutable parent session/run/reporting provenance. That provenance grants no future controller permission. |

Private files share the user's OS identity. Neither a digest nor a private file's
ownership establishes which agent authored its contents. The Server must obtain
current parent authorization separately and bind the observed child to the exact
current assignment, SessionRunId, reporting provider/invocation/conversation/
generation and child control epoch/human-input revision. Human ownership,
uncertainty, unresolved attribution or a control barrier prevents fallback.

Captures and recoveries use the existing public `WorkflowAuthority` for immutable
parent provenance, including its controller epoch, Producer and Server lifetime.
The final caller independently authenticates today's source capability under
retained-to-Session lock order. Equality with stored provenance is necessary for
using a capture as current evidence; it is never source authentication. A parent
transfer or source change requires a new attributable capture. Historical bytes
remain preserved. Source secrets are never serialized into either artifact.

A capture also preserves the existing `AssignmentControlEvidence` produced by the
Server: reporting activity/input revisions, publication identity/issue and human
input revision. Helpers receive independently derived current evidence from the
held Server context and require exact equality plus consistency with the capture
assignment, binding, result version and Server lifetime. Caller-supplied evidence
cannot provide that authority. A valid canonical child publication requires normal
collection. The raw missing/invalid original condition remains separate: canonical
`publication: None` can have no issue, and an uncommitted invalid original can have
an `Unattributed` issue. Helpers do not invent a `Missing` mapping.

An accepted `Interrupt` barrier remains a barrier until fresh same-turn settlement
is explicitly confirmed. Workflow cancellation retains evidence and permits
existing inspect/collect/reconcile behavior while closing new and resumed
dispatch. Any request for a new marker or repaired report must use that accepted
dispatch-closure/current-source input gate. Capturing evidence cannot clear an
interrupt, reopen a cancelled workflow, send input or replay uncertain delivery.

## Freshness anchor

The existing version owner generates a random 256-bit `fallback_nonce` when it
forms the immutable brief for a result version. Its durable publication order is:

1. Prepare that version's normal immutable contract and brief, including the nonce.
2. Complete the existing version transition under Server authority. Only the
   authoritative current result version and its retained contract/brief digests
   can supply a fallback context.
3. Give the current child that version's brief through ordinary scoped dispatch.
4. When normal reporting fails, the child leaves a complete fallback artifact
   containing that exact context and prints its exact byte digest as one line:

   ```text
   OVRCR_FALLBACK:<64 lowercase SHA-256 hexadecimal characters>
   ```

A prepared or rejected future brief is never current authority. A legacy brief
without the nonce returns an explicit freshness gap. Timestamps, repeated reads
and caller-supplied freshness booleans cannot fill that gap.

The helper re-reads exact contract and brief bytes from the held private directory.
The digests must match the authoritative current context. The brief must also
identify the same assignment and result version and contain its nonce. The artifact's immutable identity, result version, contract/brief digests and
nonce must match. It contains no binding or control claim: only the trusted Server
capture establishes those current facts under the authenticated source fences. An old marker in the same live terminal
therefore fails when it references an earlier result, a prepared future result,
an old brief/control epoch, or replaced artifact bytes.

The nonce and digest establish correlation and integrity. They do not establish
response boundaries, child authorship or semantic completeness. An old marker
for unchanged evidence in the same still-current result cannot prove when the
child finished. The parent must independently establish that the referenced
deliverable and evidence remain current; unresolved freshness stays incomplete.

## Capture and immutable recovery

Production capture must check current parent authority and child context, observe
the original missing/invalid submission condition, read the active screen, then
recheck the same context and condition under the final Server fences. Inspect
followed by ReadTerminal does not provide this atomic attribution. A takeover or
rebind during capture invalidates it. An implementation must not hold unrelated
global locks while waiting for file I/O or an external provider.

The capture preserves the exact original invalid bytes or an explicit missing
condition. A structurally valid current child publication uses normal collection.
Unreadable, oversized or escaping originals cannot be silently called missing.
Recovery preparation and publication both re-read the original and require the
captured condition and bytes to remain unchanged.

The artifact contains the complete requested deliverable and no unresolved items.
The active screen must contain its complete digest marker. Every contract
acceptance requirement requires a separate parent assessment, a confined evidence
file with its exact digest, and an exact excerpt found in those independently
re-read bytes. The child artifact cannot serve as independent evidence for itself.
These structural checks do not judge whether prose actually satisfies the scope;
the parent must assess the full deliverable and the relevance of every excerpt.

Publication repeats the file, original-condition, current-binding, control and
author checks. It freezes artifact bytes, independent evidence bytes, assessments,
the original condition/bytes and capture provenance in
`parent-fallback-r<result>-v<recovery>.json`. The held directory prevents path
replacement from redirecting I/O. Atomic immutable publication refuses a different
record at the same version; identical retries return the existing version. It
does not rewrite a child submission, publish a child receipt, accept the result,
or grant additional permissions.

## Limits and incomplete outcomes

| Limit | Bound |
| --- | --- |
| Active screen | 64 KiB, nonzero geometry, no truncation flag |
| Fallback artifact | 64 KiB |
| Complete deliverable | 32 KiB |
| Original invalid bytes | 64 KiB |
| Independent evidence | 128 KiB aggregate; one reference per contract requirement |
| Excerpt and assessment | 4 KiB each |
| Recovery versions per result | 1 through 64 |
| Serialized immutable publication | Existing Directory limit of 512 KiB; capacity failure preserves originals |

Only single filenames in the assigned private directory are accepted. Symlinks,
paths, changed digests, unsupported payloads, partial markers and missing evidence
are rejected. Frozen evidence contains only relevant authorized material and must
exclude credential values. Task contents remain outside reporting Events and
desktop notifications.

A terminal read provides the active screen. It cannot restore lost scrollback or
prove a complete response. A plausible success line, stale screen, unrelated
artifact, incomplete deliverable, replaced run or unresolved human modification
keeps the assignment incomplete. The parent requests a corrected report or
reports the exact evidence gap. No transcript service, capture loop, arbitrary
check runner or automatic acceptance is provided.

## Public parent operations

1. Read the current owned assignment with `ovrcr_inspect`. Call
   `ovrcr_capture_fallback` with its handle, `expected_identity`,
   `expected_result_version` and `expected_control_epoch`. The Server preserves a
   bounded active screen, current canonical control evidence and exact original
   bytes/absence, then reloads and revalidates before registering a capture ID.
2. Independently inspect the full deliverable and every acceptance requirement.
   Copy only relevant authorized evidence to private mode0600 files in the assigned
   directory. Call `ovrcr_recover_fallback` with the handle and a draft containing
   the registered `capture_id`, recovery `version` (1..64), artifact filename and
   digest, original condition and one evidence assessment per requirement. It
   returns the immutable recovery path, digest and full frozen recovery.
3. Publication leaves acceptance pending. To select it explicitly, call
   `ovrcr_accept` with handle, exact current identity/result/control epoch and:

   ```json
   {"target":{"ParentRecovery":{"recovery_version":1,"digest":"<recovery digest>"}}}
   ```

   `ChildSubmission` uses `{"target":{"ChildSubmission":{"digest":"<child digest>"}}}`.
   The existing `result_digest` tool argument remains a child-only shorthand.
   Supply exactly one target. One existing acceptance slot is immutable per result:
   identical retries keep its reference and a different selection requires the
   normal next-result follow-up. A recovery never sets child result digest,
   publication identity, handled state, execution counts or dispatch receipts.

The existing version record stores at most 64 trusted capture references and 64
recovery references. Capture files use bounded per-result slots, including a
failed-save orphan: a conflicting orphan blocks that slot visibly instead of
creating unbounded new filenames or being adopted as authority. The registry
refuses serialized assignment metadata above 2MiB without evicting history.
Contracts, briefs, artifacts, independent evidence and original condition/bytes
are frozen into the recovery. Historical verification checks these exact bytes
and registered digests; later edits to mutable evidence sources or later parent
transfer do not erase an accepted historical decision. Current capture/recovery
publication and a new acceptance still require today's authenticated source and
full control fences. A transferred parent needs a fresh capture for an unaccepted
result.

`ovrcr_collect` exposes `recoveries`, `accepted_recovery` and `acceptance_ref`
separately from authentic child `submission`/`publication`. `submission_issue`
reports a child read failure without erasing an independently valid accepted
recovery. Optional `version` returns historical selection in `selected_acceptance_ref`,
`selected_recoveries` and `accepted_recovery`, while current identity/publication
fields stay current. A later
real child publication remains separate. An invalid occupied original is never
removed; repair uses the ordinary next result version. Follow-up archives all
recovery references/acceptance with its prior version and clears current slots.

Cancellation does not close metadata-only acceptance, but ordinary Human,
Uncertain, attribution and Interrupt/return barriers still refuse it. None of
these tools starts, resumes or sends input to a stopped child. New decisions
currently require the existing live, Running child reporting fence; retained
historical evidence stays inspectable.

Recovery and acceptance never answer a structured question, mark an answer
`Applied`, clear a native prompt or resolve an uncertain delivery receipt. An
`Accepted` answer still means its admitted input has not been confirmed delivered;
fresh input remains blocked by the existing answer barrier. Inspect the question,
answer receipt and native-input gap separately from the selected result.

The parent must decide whether each unresolved question prevents the requested
result from being complete. An independently verified ordinary deliverable can be
accepted while an optional clarification remains visible. If completion depends
on a human decision, keep acceptance pending and surface that decision to the
person. A terminal marker, recovery artifact or accepted result cannot supply
human approval or authorize an unrelated action. Repair/follow-up dispatch uses ordinary
cancellation and scoped-input rules.

The JSON representation of a legacy child acceptance reference keeps its exact
field order and shape, including `result_digest`, so nested immutable declaration
hashes survive. Parent recoveries use explicit target/version/digest fields.
Mixed or unknown representations refuse. New decision artifacts record schema2
and complete current public WorkflowAuthority; old schema1 artifacts and hashes
remain unchanged. Public provenance, a nonce and a digest are not authentication.

Coding acceptance requires the ordinary authenticated child result and its
immutable candidate, pinned base, repository/workspace identity and named check
evidence. A parent fallback artifact has no authenticated CandidateSnapshot.
Selecting ParentRecovery for a coding contract must refuse before publishing an
acceptance decision. Its captured and recovered evidence remains inspectable;
it cannot replace the ordinary selected-version coding verifier.

Delivery references the exact typed acceptance target. A verified research
ParentRecovery does not require a valid child submission and must not create a
child publication. Each newly committed capture, recovery or acceptance advances
the existing obligation revision because it adds retained evidence or a decision.
Exact recovery/acceptance retries preserve both immutable bytes and the revision.
A cleanup declaration made before that evidence is stale; the parent must declare
against the current revision before archive. Opaque evidence indices cannot
substitute for verified recovery acceptance.

The combined source is integrated onto accepted #324/#333. Compilation and final
same-revision headless/native acceptance remain unrun at this checkpoint.
