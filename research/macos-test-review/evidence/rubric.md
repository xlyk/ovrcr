# Review rubric (frozen)

Score four dimensions with integers 0-4 each. Read the test's FULL body, its assertion helpers, its fixture behavior, and enough production code to know what the assertions actually protect. Names, length, assertion count, passing status, or keyword classification do not justify scores. State a concrete plausible defect the assertions would detect; if none exists, say why.

C = criticality of protected behavior
0 = no identifiable product/test-infrastructure consequence
1 = cosmetic/incidental detail or minor maintenance consequence
2 = bounded feature failure with a straightforward workaround
3 = major workflow failure, unusable terminal/session, lost work, wrong ownership, or durable-state integrity threatened
4 = credential/privacy breach, destructive wrong-target action, irreversible data loss, or violation of the one-server / 50-session guarantees

S = regression detection signal
0 = tautological, unreachable/vacuous, or no effective assertion
1 = broad smoke check permitting most plausible wrong outcomes
2 = specific happy-path regression caught, important wrong outcomes escape
3 = meaningful boundary/failure/transition assertions that reject plausible defects
4 = precise state-preserving positive/negative assertions, or a decisive complete invariant, that catch the stated subtle defect

U = distinct coverage
0 = fully subsumed by named retained coverage; no extra boundary
1 = mostly redundant with only a small evidenced distinction
2 = distinct input/boundary within an already-covered contract
3 = distinct failure mode or integration boundary in reviewed coverage
4 = only evidenced protection for an important failure mode after cross-suite comparison

F = fidelity to the claimed behavior
0 = only source text, a copied algorithm, or a stand-in for the subject
1 = a plumbing check replaces the subject's behavior
2 = real component with a meaningful boundary replaced (state the limit)
3 = production behavior through its relevant API with appropriate external fixtures
4 = the full boundary its claim needs, including real OS/process/storage when that boundary IS the subject

Derived scores (compute, never invent): criticality_score=25*C; usefulness_score=10*S+10*U+5*F; retention_score=10*C+6*S+6*U+3*F.
Runtime, observed retries, and maintenance friction NEVER lower these scores. Score value only.

Safeguards:
- A pure algorithm test can earn F=4 for directly testing the full production algorithm; spawning a subprocess does not itself earn F=4.
- An `is_ok()` assertion can protect a historical wire-format contract. Two tests touching the same message type are not automatically duplicates.
- Similar negative cases can protect different authority states; compare fixtures and branches before assigning low U.
- Generated/table-driven matrices can contain distinct boundaries; judge the real assertions and independent expectations, not loop size.
- A body that executes in many binaries/binaries is reviewed once per body but must keep one output row per execution ID. If the execution contexts are equivalent, pick the lexicographically smallest full test_id as the retained representative and mark the rest as overlap candidates of it.
- Mocks and test-only constructors lower F only when they bypass the claimed behavior. A fake provider can legitimately test OVRCR composition but cannot prove native provider acceptance.
- Nextest runs each test in a separate process; a process-local mutex does not serialize hosted cases.
- Sleeps, polling, subprocesses, PTYs and Git are observed mechanisms, not measured durations or proven causes of flakiness.
- Claim historical regression value only with inspected source or history evidence; an unverified comment is not proof.

Recommendation (ordered, first match wins):
1 investigate — unresolved behavioral evidence, low confidence, or contested overlap; specify the precise boundary and the next verification. Never a label for a test you did not read.
2 consolidate — high-confidence, independently confirmed redundant coverage or repeated execution with existing named retained protection; for C>=3 explicitly document preservation of the high-criticality invariant.
3 prune_candidate — high-confidence, C<=2, usefulness<=25, and either no effective behavioral protection or demonstrated complete subsumption by retained tests.
4 improve — S<=1 or F<=1 while meaningful behavior needs protection (C>=2 or U>=2), with no proven equivalent coverage; name the missing assertion or production path.
5 keep — remaining grounded coverage.
Do not force a pruning quota. Cost and flake concerns are reported in cost_drivers and never turn a valuable test into a prune_candidate.

confidence: high = body + production behavior + any overlap claim directly checked; medium = behavior understood with a stated boundary/comparison uncertainty; low = material behavioral inference remains.

Test level (independent of Cargo target kind): unit = isolated function/module/model logic; integration = multiple OVRCR components with controlled external dependencies; acceptance = compiled CLI/Dashboard/server or packaged artifact exercised through a real external boundary.

Output record — one JSON object per execution ID, exactly these keys:
{"test_id", "test_level", "production_refs", "behavior", "failure_caught", "criticality", "signal", "uniqueness", "fidelity", "criticality_reason", "signal_reason", "uniqueness_reason", "fidelity_reason", "confidence", "recommendation", "recommendation_reason", "overlap_test_ids", "cost_drivers", "reviewer"}
- production_refs and overlap_test_ids are arrays of strings (production_refs items are `path:start-end`; overlap_test_ids are full test_ids from your shard materials or the wider inventory).
- Scores are JSON integers 0..4. confidence in {high,medium,low}. recommendation in {investigate,consolidate,prune_candidate,improve,keep}.
- behavior and failure_caught are concrete descriptions, never name paraphrases. The four reason fields cite evidence as `path:start-end`. cost_drivers records observed mechanisms only.
- Never emit a row you did not read. Never use placeholder text such as TBD/TODO/N/A. Reasons must be at least 12 characters.
