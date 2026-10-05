# Cursor native acceptance — partial, 2026-10-05

The installed, logged-in Cursor CLI returned **CURSOR_CLI_OK** from a managed CLI launch in the actual native OVRCR GUI. Both the Dashboard picker and CLI launch established a genuine native-owned startup binding (`bound`, `Connected`). [#236](https://github.com/xlyk/ovrcr/issues/236) remains open and [PR #283](https://github.com/xlyk/ovrcr/pull/283) remains draft.

Capture source: reviewed `c2e29ac4e2cd57177f20738405fc9c487f963a07`, using the checkout's disposable `just gui` fixture. Installed release `2026.10.01-e373342` is provenance only; runtime admission maintains no release support list. Activity remains Unknown; metrics, Ready, Input requests and recovery are unavailable.

Dashboard Browse and visible pause/resume passed before model input. The same managed wrapper/native child stopped (`Ts`/`T+`) and resumed (`Ss`/`S+`) in kernel observations. Paused input returned `Conflict`; this does not claim every auxiliary descendant stopped. The Dashboard showed native **Workspace Trust Required**. Approval was requested and not received, so no trust choice or Dashboard model turn was sent.

The separate managed CLI launch reached its ordinary prompt without a trust choice; the cause of the difference is unknown. Exactly one marker-only turn was submitted through native GUI input. The prompt is AX entry 13; **CURSOR_CLI_OK** is a distinct assistant response at entry 16, not command echo. Native model observed: Grok 4.6 High Fast. No tool output was visible; tool-call count was not independently counted. Native model settings and normal permission mode were retained.

Native `exit` produced resume guidance. Before the next inventory read, the GUI, Server, socket and registry were gone. The launcher completed with exit 0; all 20 recorded task-owned process groups and the recorded Dashboard plugin directory were gone. **Native CLI exit status and the normal GUI-close action were not captured.** The cause of fixture closure is unknown; no clean native exit or close-button pass is claimed. CLI plugin removal was not separately captured.

Metadata-only comparison of 665 baseline-listed native files found 16 changed or missing paths, including CLI config, statsig cache and synchronized skills. Contents were not read; new-file inventory and the cause of individual changes were not established. Native storage was preserved. No credential copying, global plugin installation or permission-mode flag was performed.

[observations.json](observations.json) records selected nonsecret doctor fields, AX excerpts and hashes/sizes/dimensions of original captures. Whole captures remain private because they contain unrelated fixture inventory. AX excerpts select task header indices or only the right terminal column. Images were not edited: original bytes are JPEG despite the private `.png` filenames. The manifest records the actual format. Kernel/ownership and metadata records remain private, with aggregate outcomes reported here.

## Automated evidence and remaining gates

The real managed Server/PTY regression failed on the original release gate and passed with runtime admission. Seven Cursor unit tests, six CLI cases (one ignored fixture helper), two bounded-probe ownership tests, full sequential regressions (1,397 passed, 24 ignored), strict Clippy, format/diff, doctests and 47 Node tests passed. Two earlier parallel full runs failed on unchanged automatic-title and context-protocol tests; isolated replays passed, but causes remain unproven. No assertions changed.

Hosted macOS CI on capture source `c2e29ac` passed: [run 37330353686](https://github.com/xlyk/ovrcr/actions/runs/37330353686), 1,396 Rust passed, 25 skipped, no retries and 47 Node passed. Linux jobs were skipped. The subsequent rebase preserves Cursor runtime/test files byte-for-byte and combines Hermes help wording; current PR-head CI must be checked separately.

Still required: Dashboard response after explicit disposable-workspace trust approval, native exit status, normal GUI close, CLI lifecycle/native interrupt and remaining platform/auth gates. One of two authorized marker-only turns has been used. Cleanup does not complete these missing acceptance cases.
