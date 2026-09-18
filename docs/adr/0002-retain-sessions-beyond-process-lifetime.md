# Retain sessions beyond process lifetime

Provisional product direction, 2026-09-15. Interview defaults below are recorded for comparison; final commitment is deferred pending research into Superset's restoration behavior. No implementation is authorized by this document.

Agent and shell sessions retain identity and workspace placement across server restarts so the user can recover their working context. Agent conversation history belongs to the provider: OVRCR persists the conversation reference and session metadata, without saving agent transcripts, terminal output or previews. Restoring a session does not automatically execute work: agent conversation resume uses the provider's native capability through an explicit user action. Closing archives a session without deleting the provider's history; process exit, crash, response completion and an agent's suggestion to close do not remove it from the active list.

This extends the existing server-lifetime retention contract with durable session records. Retention covers both server restart and computer reboot; interrupted sessions return as stopped, with no automatic execution. It does not establish live process survival across server loss.

Closing a running session requires confirmation before stopping its process and archiving it. Exited sessions archive immediately; hiding or closing a pane remains separate. Session records remain until explicitly deleted. The fifty-session limit applies to live terminal processes, not stopped or archived records.

Neither agent nor shell output is persisted for interactive session retention. Shell sessions retain their row, title and working directory; reopening starts a fresh shell. The initial proposal to retain output was replaced by metadata-only retention during the design interview. Existing scheduled-task run logs are outside this decision's scope.

The first release targets verified conversation capture and native resume for Claude, Codex, Pi and Oh My Pi. Other configured agents retain their session rows, but resume requires a verified integration. This is an acceptance requirement, not a claim that current integrations already support recovery.

Reopening reuses the same OVRCR session identity, sidebar row and title. Failed resume or a missing provider conversation leaves the record visible with an explanation; starting a new conversation is a separate explicit action and never an automatic fallback.

Archived sessions are available in a searchable view. Unarchiving restores the row without starting a process. Deleting an OVRCR record does not delete the provider's conversation files.

Running sessions block workspace removal. Removing a workspace with stopped sessions requires confirmation and archives their records, retaining the original workspace path for context. Resuming requires an existing working directory.
