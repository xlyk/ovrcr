# Bounded evidence consistency review

Two material omissions in the current closeout draft:

1. **Record the already-observed Nextest retry.** `research/dashboard-outbox-2026-10-31/verification.md:30` says retries must remain explicit but only describes them prospectively. The still-running log `60-macos-nextest-ci.log:164–186` already records `doctor_interrupt_cleans_its_owned_version_probe_before_exit` as **TRY 1 FAIL**, followed by **TRY 2 PASS**. Add that observed retry while keeping the overall run pending; a future successful gate cannot be described as clean first-pass evidence. `gate-manifest.json` already lists this retried case.

2. **Retain the third failure marker from cancelled run 39.** `verification.md:57–61` discusses the two diagnosed fixture races and cancellation without recording that `39-linux-workspace-node.log:930` also reports `client_with_wrong_protocol_version_is_refused ... FAILED`. Its owning binary was interrupted before a final summary/failure body, so the exact cause is unavailable from that attempt. State “two diagnosed fixture failures plus one additional protocol-refusal FAILED marker; body unavailable before cancellation,” rather than allowing the two fixture corrections to appear to exhaust the observed failures. The private `gate-manifest.json` field `cancelled_for_fixture_correction[].failures_captured: 2` should likewise distinguish two captured failure bodies from three observed failing names. Do not infer this attempt's cause from the later log-50 preamble timeout.

No other concrete inconsistency found in this bounded pass. Recomputed completed-run totals match logs 18, 49 and 50; focused logs 42–45 and 53–56 each contain one passing case. Named receipts and relative Markdown links resolve, and ignored/filtered cases are not counted as passing cases.

Logs 57 and 60 remain pending for this review. No code re-review, tests, builds, processes, network, UI, or source/document edits were performed.
