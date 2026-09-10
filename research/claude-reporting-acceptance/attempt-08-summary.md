# Attempt 08: named-root startup and child metadata

Claude Code 2.1.267; 2026-09-10. This is native provider evidence, not OVRCR adapter acceptance. OVRCR source checkpoint was `406734f`; Task3 mechanics were being developed separately.

Launched an owned disposable interactive fixture with:

```text
rtk proxy script -q /dev/null python3 /private/tmp/ovrcr-claude-contract-20260910/attempt08/launch.py
claude --setting-sources "" --settings <fixture>/attempt08/settings.json --strict-mcp-config --session-id <session_id-1 UUID> --agents <fixture-root definition> --agent fixture-root
```

The launcher chose the explicit UUID before exec and recorded its argv and owned PID/PGID privately in `<fixture>/attempt08/launch-metadata.json`. The capture script preserved the complete input field-name inventory and only allowlisted values. The repository bundle pseudonymizes IDs consistently within this attempt. Prompt/response bodies, transcript paths and credentials are excluded.

Existing subscription authentication was read privately into memory for the isolated configuration; no credential file or live settings were changed. Native PID and PGID were both 94134. The launch tool returned session64646; `/exit` returned exit status0. Authorized `kill(pid, 0)` and `killpg(pgid, 0)` checks both reported absence after exit. These checks establish cleanup of the owned process/group, not arbitrary detached processes.

## Observations

- Named root emitted `SessionStart`, `source=startup`, exactly the selected session UUID, and `agent_type=fixture-root`. Its complete field inventory contains no `agent_id`. This demonstrates why `agent_type` must not be used as a child-only marker.
- The root prompt asked for one harmless subagent with no tools or file access. The native UI actually ran it as a background agent. This is child-isolation evidence, not proof of a foreground subagent route.
- `SubagentStart` and `SubagentStop` shared the root session UUID and included `agent_id`; the main child had `agent_type=general-purpose`. Additional `SubagentStop` events had a present `agent_id` and empty `agent_type`. Their auxiliary attribution was not investigated.
- Native UI displayed the child finished and distinct root output `OVRCR_ROOT_OK`. The first root Stop occurred before child completion; subsequent root processing used another `prompt_id`. Callback order remains an observation, not a certified settled signal.
- Seventeen sanitized records are in `attempt-08-live-hooks.jsonl`, including final `SessionEnd`.

## Remaining admission gates

This strengthens the pinned fresh-start discriminator proposed in `initial-admission-contract.md`. It does not implement or certify the OVRCR invocation channel. Actual routing must still reject matching-ID child events, malformed child metadata, foreign-provider replay, stale invocation and mismatched UUID, and must admit only once under the current supervisor. Conflicting launch modes must be excluded explicitly; no claim is made for resume, clear, branch or background-fork admission. No confirmed settled or complete accounting capability is inferred.
