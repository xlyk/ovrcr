# Sidebar session-jump acceptance

Status: Control+1–9 is implemented for the first nine eligible visible Agents, with immediate one-press selection. Kyle tested the revised native GUI and reported “it works great”. Independent follow-up reviews found no behavior issue; final workspace/hosted checks are tracked in PR #406. Historical attempts below retain their tested revisions. No general production-terminal compatibility claim.

- Baseline: `8f9569729574b0f0dedb56bd8b9ff1a1384b08fa`.
- [Native GUI first attempt](native-01/review.md): reviewed integration `2ae54eee`, failing Cmd+1 alongside working existing Cmd+K. Original captures and cleanup inventories retained.
- User explicitly selected the existing disposable OVRCR GUI as the computer-use route. Its input must go through the real Dashboard outer PTY and parser. Other terminal hosts remain separate/unverified; iTerm CUA was denied and no alternate control channel was used.
- Automated core/adapter tests do not substitute for native input/visible outcomes. Final corrected captures, integrated regression counts, exact revision and remaining platform gaps will be appended here.

Feature: [#401](https://github.com/xlyk/ovrcr/issues/401), slices [#402](https://github.com/xlyk/ovrcr/issues/402), [#403](https://github.com/xlyk/ovrcr/issues/403), [#404](https://github.com/xlyk/ovrcr/issues/404), [#405](https://github.com/xlyk/ovrcr/issues/405).

## Correction checkpoint

[Native correction review](native-02/review.md), source `8575c5f6`: actual Cmd+1 selection and separate destination marker, retained Browse inspection/no restart, subsequent running marker, and existing Cmd+K/modal priority passed. All owned resources were cleaned up. Native held-label/prefix screenshots remain unverified because the control tool serializes observations after release; timing evidence is retained. Adaptive and Operator integration/final regression remain pending.

## Committed branch checkpoint — integration approval pending

The [focused reports](automated/) distinguish tested branch revisions from final integrated acceptance. Integration product code at this checkpoint is `f0ce7255` (core402, retained404, GUI correction and native evidence). Adaptive403 is committed separately at `7055167a`; Operator405 at `91994038`; the final focus-ordering/freshness correction at `ad23734b`. Those branches are clean and remain unmerged while direct feature-branch merge approval is pending. Main is not authorized to merge.

The freshness correction has 27/27 input unit cases and 1/1 real GUI-encoder/Dashboard PTY case green; GUI-only Clippy passes with `--no-deps`. Full Clippy still stops at the inherited #404 predicate lint, whose fix is on #405. Final combined workspace tests, lint, independent reviews, adaptive/Operator native checks and current hosted CI remain open; no final acceptance claim. The bounded recorded-frame lookup found no matching GUI frame and did not fill the held-visual gap.

Original raw capture files named in this report are preserved byte for byte in the [artifact index](raw-artifacts.md).

## Approved integration and single-digit revision

Kyle explicitly approved local feature-branch merges and remaining checks for #406, while forbidding main merge. All four slices and review corrections were integrated and pushed as `5916f78c`. Independent [Standards](automated/review-standards-followup.md) and [Spec](automated/review-spec-followup.md) follow-ups resolved the reported issues. Full all-target/all-feature Clippy and formatting passed on that source.

[Native combined-build attempt](native-03/review.md) confirmed single-digit selection in a short list, a separate destination acknowledgement, Operator cancellation/draft retention, and hidden-sidebar refusal. The GUI suite passed13/13 sequentially; a default-parallel workspace run had earlier stopped at GUI startup waits. A later full run encountered explicit disk exhaustion; six affected suites passed139/139 on retry after space recovered, with two helper entrypoints ignored. Those failed attempts remain distinct from passing checks. Final workspace/hosted outcomes are still being collected.

After trying the ten-Agent adaptive interaction, Kyle said, “we should only use single digits”. The [parent specification](https://github.com/xlyk/ovrcr/issues/401) and [long-list ticket](https://github.com/xlyk/ovrcr/issues/403) now record the replacement: Cmd+1 through Cmd+9 for the first nine eligible drawn Agent rows, regardless of list length. Further rows stay visible and reachable by scrolling or Agents search. There is no zero-padded code, partial prefix, or prefix editor. All other readiness, cancellation, identity, retained, Operator, layout, and support-gate contracts remain in force. This source change and its final checks are in progress; no ready-PR or main-merge claim.

## Control-hold preview requested

Kyle subsequently chose holding Control instead of Cmd and asked to test the updated native TUI GUI directly, without screenshot work. The current design is Control+1 through Control+9 for the first nine eligible visible Agents. Existing non-digit Control commands and Cmd host shortcuts retain their ordinary behavior. The spec and all four tickets were updated and reread. A focused Control implementation and GUI preview are in progress; these older Cmd results do not qualify the new interaction.

The old source `5916f78c` completed [hosted run42278110709](https://github.com/xlyk/ovrcr/actions/runs/42278110709) successfully: macOS acceptance, offline Cursor and aggregate checks all passed. Rust reported2660passed, including two recovered flaky tests, with33skipped. The retry names and exact job metadata are preserved in the automated archives. Manual-only Linux jobs were skipped.

The complete local serial attempt remains a failed run: fixture disk exhaustion caused several failures and aborted two test harness outputs; a later quota HTTP fixture hit WouldBlock. The six previously named suites passed139/139 on retry. The two aborted suites subsequently passed21/21 and47/47 (two ignored helper entrypoints), and the single quota case passed1/1 separately. Assertions were retained. The existing hosted-filtered partial-resize case passed in the local Cargo run; it is not counted as a Nextest pass.

## Control and single-digit user acceptance

The [updated native GUI trial](native-04/review.md) ran exact source `6095a236` with ten Agent shell stand-ins. Kyle physically tested the revised Control gesture and accepted it: “it works great”. Final accessibility state showed Control test 08 selected in Terminal mode. Screenshots were omitted as requested. The GUI closed normally, launcher exited 0, fixture root/socket were removed and all 16 owned session groups were absent. Full revised-source checks remain distinct from the old baseline results above.
