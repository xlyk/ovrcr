# Issue 59: desktop notification acceptance

Implementation base: `3574e92` on `codex/issue-59-desktop-alerts`.
The exact reviewed PR #56 head `86ef46b8eff3128566f76022c2cfef49714031ac`
is an ancestor of this base. Its merged state and four successful head checks
were verified through GitHub on 2026-09-11.

This record is in progress. Unfinished checks below are not passes.

## Scope

Opt-in desktop notifications from the active dashboard for new accepted Codex
root Ready responses whose terminal is not shown in a visible pane. Alerts mean
that a response is available to review. Sound, unread acknowledgement, metrics,
Confirmed completion, release and live installation are outside issue 59.

The implementation reuses the accepted binding, generation, turn and activity
revision. No protocol change or replacement reporting source is planned.

## Acceptance matrix

| Requirement | Evidence/status |
| --- | --- |
| Exact reviewed hooks base | Verified ancestor above |
| Default disabled, config and dashboard control, identity-only content | Pending code and tests |
| Root-only deduplication; stale, duplicate, child, interrupt and exit suppression | Pending managed CLI/PTY/socket/dashboard tests |
| Visible-pane suppression without acknowledgement | Pending geometry tests and native GUI |
| Initial attach/reconnect baseline; no disconnected replay | Pending real dashboard reconnect tests |
| Bounded nonblocking delivery, denial/failure/timeout | Pending host recorder and subprocess tests |
| Actual desktop delivery | Pending macOS native acceptance |
| Synchronous ownership and 50-session regression | Pending current-head hosted gates |
| Isolated fixtures and owned cleanup | Pending native ownership/cleanup record |
| Independent review, exact revision, docs and work diary | Pending delivery checkpoint |

## Native evidence boundary

`native/codex-fixture.py` is a deterministic native child, not the installed
Codex CLI. It drives the public managed launcher and hook reporter from an actual
PTY and socket. It supplies private-content canaries that must never appear in
notifications. This isolates notification acceptance from credential access and
native trust changes. Existing PR #56 evidence covers the unchanged actual Codex
0.153.0 hook semantics; this task does not recertify that provider.

The macOS check must observe a real Notification Center result, separately from
recorded subprocess dispatch. Linux automated coverage does not establish a
visible Linux desktop notification. No Linux native delivery claim is made until
it has an independent desktop observation.

## Attempts

Preserve each failed and successful run with its command, source revision,
exit status and executed count. Native captures and process ownership belong in
`native/`; final current-head test and CI results belong in this record.
