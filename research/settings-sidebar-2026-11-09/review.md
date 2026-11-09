# Independent review — #413

Initial candidate: `f3d21df13ac9200a021a065033c0b6973d530726`.
Base: `9e027779bfc94cb217f4921a04e30ed78747ed92`.
Both reviews were read-only and ran independently. Full workspace acceptance was held until review corrections.

## Standards

All findings refer to `crates/ovrcr-tui/src/dashboard/settings_editor.rs` in the initial candidate.

- **P1, lines 2306–2307:** Re-resolve collection rows before committing. A frozen Add index can overwrite an agent appended by another client while the draft is open. Violates the repository's user-data protection rule.
- **P2, lines 2420–2422:** Settle successful unchanged saves. The Server can return Ok without SettingsChanged for an already-explicit identical value; the candidate leaves the request pending forever.
- **P2, lines 1341–1344:** Select saved sounds by canonical value. An inactive annotation such as `Tap · inactive` does not match picker labels, so Enter unexpectedly selects System default.
- **P2, lines 1559–1564:** Bound picker hit-testing to the complete options rectangle. A sidebar click aligned with an option row currently changes the preview.
- **P2, lines 2803–2806:** Render editor notices. Empty submissions and boundary/reset refusals populate `Editor.notice`, but the replacement renderer never displays it.

No additional serious baseline-smell finding. Initial correctness verdict: incorrect; correction required.

## Spec

All findings refer to the same initial candidate and file.

- **P1, lines 2306–2307:** Refresh collection write targets. #421 requires unrelated entries to remain intact; stale append metadata can overwrite a newly published root or agent.
- **P2, lines 2419–2422:** Complete unchanged saves. #413 requires pending/confirmed/refused states; successful no-op saves stay marked `saving…`.
- **P2, lines 2431–2439:** Associate collection confirmations with their own edit. Under #422's late-response requirement, one agent-array publication must not settle two requests and suppress the second request's refusal.
- **P2, lines 2444–2449:** Compare normalized values. Valid TOML `1_0` publishes as `10`; raw string equality leaves the confirmed typed value pending.
- **P2, lines 1341–1344:** Preserve inactive sound selection. The saved choice must remain selected despite the display-only inactive annotation.
- **P2, lines 3251–3252:** Show consent consequences before activation. At 80×24/60×24, metadata pushes notification/iTerm permission warnings outside the initially visible details area.
- **P2, lines 2294–2296:** Guard open-manager edits after an unparseable reading. Add can still submit while ordinary Settings editing is disabled.
- **P2, lines 1037–1039:** Reject zero-option theme layouts. At 60×8/60×9, Enter submits a selection whose option row cannot be drawn.

No scope-creep finding. Initial correctness verdict: incorrect; correction required.

Initial totals: Standards 5 findings (worst P1); Spec 8 findings (worst P1). Three findings overlap across axes; the reports remain separate.

## Final Standards re-review

At `a98ffde42bc110238c9333bfdc2298e4860d3d66`, all five original Standards
findings are resolved. Collection no-op saves now compare the published child
at the actual request path while retaining document-source and request-owned
acknowledgement requirements. Live Server regressions cover all three affected
collections. The other four findings were cleared at `f6d15cf5`.

Verdict: correct within the scoped re-review; no remaining findings.

## Final Spec re-review

At `a98ffde4`, all eight original Spec findings are resolved. The final
collection-child correction preserves own-response and document-source gates;
the other seven findings were cleared at `f6d15cf5`.

Verdict: correct within the assigned scope; no remaining findings.

Final totals: Standards 0 remaining, Spec 0 remaining. No unresolved severity
remains in either axis. Reviewers did not run checks; execution evidence is
recorded separately.

## Acceptance-driver follow-up

The full regression run exposed legacy Settings navigation in three real-PTY
alert/sound tests and a palette test. Driver migration `a39d2ecc` passed all
three PTY cases and the complete 440-case Dashboard target. Spec re-review
confirmed that save/refusal, permission, preserved opt-ins, no-preview and
no-replay assertions were retained.

Standards re-review identified one additional test-only Linux assumption:
metadata was required to be initially hidden even when shorter platform
guidance left room. `cdedb3e9` removes that absence assertion while retaining
visible consent guidance, geometry and reachable metadata. The exact frame
test passed; Standards cleared the correction.

Final state remains Standards 0 outstanding, Spec 0 outstanding. Production
code has not changed since `a98ffde4`; subsequent changes are test drivers and
verification documentation.
