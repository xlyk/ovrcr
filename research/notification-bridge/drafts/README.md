# Approved contract reconciliation drafts

These local body files reconcile Kyle's 2026-10-03 decisions: original bundled
short tones plus system default with the selector retained, and documented System
Settings opening with manual Notifications → Bridge instructions. They have not
been sent to GitHub. All source issues were OPEN when read.

| Body file | Narrow change |
| --- | --- |
| [#220](issue-220.md) | Replace Glass choice/resource assumptions; specify the accepted Settings destination and preserve all ownership, privacy, sound opt-in and failure requirements. |
| [#221](issue-221.md) | Replace the Glass-specific exceptional-use and direct-pane requirements with original provenance/supported resource facts and documented app opening/manual navigation. Preserve verification-only scope. |
| [#224](issue-224.md) | Retain the selector for original tones plus default; preserve flags, existing settings authority and full settings-to-native-audio acceptance. |
| [#226](issue-226.md) | Verify each original/default choice in assembled-release acceptance. |

System default for a missing setting is a proposed implementation choice; no
shipped selector or selected Glass setting exists to migrate. The candidate IDs,
fallback policy and native test bounds are in the
[sound contract](../proposed-sound-contract-20261003.md).

The local ADR 0006 diff is the corresponding reviewable documentation amendment.
Historical native reports and Glass investigations remain unchanged. The
[manifest](manifest.json) records the live body hashes and exact replacement
counts. Other issue content and unchecked acceptance boxes are preserved.

Publishing requires separate authorization for these exact four issue bodies and
any status/comment update. Re-read the live bodies before publication to preserve
later owner edits. No issue close, parent status change, label change, PR, merge or
deployment is included. Draft bodies are not proof that the underlying production
feature or native release acceptance passed.
