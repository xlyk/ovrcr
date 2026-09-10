# Attempt10: correlated permission wait and exact transcript association

Native Claude Code2.1.267; disposable fixture only. Launched via `rtk proxy script -q /dev/null python3 <attempt10>/launch.py` in the fixture workspace, with isolated existing CLAUDE_CONFIG_DIR and authorized subscription token in memory. Hook settings are synchronous command hooks with two-second timeout. Native session ID selected in launch metadata, consistently pseudonymized as conversation-1. Tool session16583, native PID/PGID75748; normal /exit completed0 and both IDs verified absent. No credentials were written by this launcher.

## Permission observation

Actual native Write tool requested creation of a marker file inside the disposable attempt directory. UI displayed the proposed contents and Yes/No approval selector. Before answering, capture contains UserPromptSubmit, PreToolUse, PermissionRequest and Notification(permission_prompt), all with the same prompt ID. The notification field inventory includes prompt_id and has no agent_id. Default Yes approved only this operation; no auto-approve option or permission-policy change was selected. Native wrote the marker and emitted a distinct assistant `OVRCR_SOURCE_DONE` response.

This supports observed waiting for a matching root permission_prompt notification. It does not make PermissionRequest itself proof of human waiting or certify arbitrary async hook ordering. API failure and elicitation remain unverified.

## Transcript association

The fixture opened only the exact path from SessionStart.transcript_path. `fstat` and filtered identity-bearing rows were captured before answering approval and after the turn. The same path/device/inode grew from21 to33 records. Twenty of21 initial records identify the selected session; assistant usage records carry matching sessionId, isSidechain=false, message.id and requestId. The remaining initial record is file-history-snapshot metadata without session identity. Full field-name inventories are retained; prompt/text/tool-input content is excluded.

Three assistant usage rows after the turn represent two message/request identities. The duplicated pair has equal usage and distinct record UUIDs. This supports deduplication by message/request instead of record UUID; it still does not prove last-wins semantics for differing usage values. No live differing-value replacement was captured. Recognized root-transcript records are the strongest supported subset label; aggregate/auxiliary coverage remains unverified. Nested iterations/cache/output-details are not additional independent totals.

Status-line context/cost snapshots are captured separately. Their field inventories do not establish source measurement identity or freshness. No managed metrics publication, complete accounting, final-write completion, Linux or GUI acceptance is claimed.
