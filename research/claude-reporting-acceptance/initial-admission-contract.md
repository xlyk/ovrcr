# Proposed initial fresh-start admission

2026-09-10. Read-only source investigation: `rtk proxy claude --version` returned 2.1.267; `rtk proxy claude --help` exited 0. No provider session, authentication or configuration change. This proposal is not native certification.

## Sources and concrete conclusion

Installed help and the [official CLI reference](https://code.claude.com/docs/en/cli-reference#cli-flags) agree that `--session-id` selects a conversation UUID. This establishes a documented expected-identity input; flag precedence and duplicate-option behavior were not tested.

The [official hook reference](https://code.claude.com/docs/en/hooks#common-input-fields) explicitly designates `agent_id` as the discriminator for hooks inside subagent calls. `agent_type` can also identify a root started with `--agent`. [SessionStart](https://code.claude.com/docs/en/hooks#sessionstart) documents `startup` for new sessions; resume, clear, compact and fork have separate source values. Fork includes both foreground branch and background copy. `--init-only` also emits startup hooks without opening a conversation. Hook processes have their own session, so matching their OS session/process group to the native PTY would be wrong.

These documented fields support a narrow startup admission rule without inventing a foreground flag. A pinned live fixture must still compare their behavior with 2.1.267. They are native metadata, not cryptographic attestation of arbitrary caller-supplied JSON.

## Proposed rule (implementation decision)

The supervisor generates a fresh UUID before spawn and owns the invocation-local channel and current runtime lease. Only a candidate routed through that invocation may qualify. Admit exactly once when the launch is an eligible local interactive fresh start, `hook_event_name == SessionStart`, `source == startup`, `session_id` exactly equals the supervisor's expected UUID, no `agent_id` field is present, and the current expected binding is None. Reject malformed child metadata rather than treating null/empty/non-string `agent_id` as root. `agent_type` alone must not reject a valid named root. Explicit child lifecycle events never admit.

The UUID is an identity constraint, not a secret. Preserve independent provider/origin routing; accepting arbitrary JSON that merely knows the UUID would not satisfy the plan. A foreign compatibility replay and a report from an old channel must fail. Later startup callbacks cannot reopen admission after clear, branch or closure. Never use callback arrival order to select between conversations. Uncertified transitions use the approved unavailable-identity fallback.

## Native argv boundary

For an initial-only certified route, reject these options as unsupported by that route before spawn; do not rely on which native option wins:

- Identity/history selection: user `--session-id`, `-c`/`--continue`, `-r`/`--resume`, `--fork-session`, `--from-pr`, `--teleport`.
- Different execution ownership: `--bg`/`--background`, `--cloud`, `--environment`, `--tmux`; native management/attach subcommands. The public reference additionally lists `--remote` and `--exec`, absent from this help output: keep these outside the eligible route too.
- Different lifecycle or absent hook delivery: `-p`/`--print`, `--bare`, `--safe-mode`; public-reference `--init-only`, `--init`, `--maintenance`. Help/version are passthrough operations without binding.

This is an eligibility policy, not a claim all combinations are native errors. Nonconflicting model, permission, prompt, named-root and directory options need not be banned. Explicit settings or inherited environment may disable reporting: absence of a qualifying hook must remain unavailable, never become permission to bind. Recognize native option tokens and their values, including equals forms and short aliases; do not substring-search prompt text. A narrow explicit supported argv grammar is preferable to silently passing unknown mode-changing flags as certified. Preserve native argv unchanged on the existing admission-disabled route.

## Next bounded native fixture

Use one owned isolated interactive launch with a newly generated UUID supplied only once through `--session-id`, a harmless named root, and disposable capture hooks. Retain sanitized launch argv, exact executable version, owned PID/PGID, invocation identity and complete hook field-name inventory. Keep synthetic IDs consistent across the launch record and captures.

1. Before the first prompt, capture root startup and assert exact expected UUID, source startup, no agent_id, and the named-root agent_type. Observe the actual foreground input prompt; startup alone would also occur in init-only mode.
2. Submit one bounded harmless task that invokes one subagent and returns. Capture SubagentStart/Stop and an in-subagent tool hook. Assert child identity is present and these matching-parent-session events cannot create/replace the root binding.
3. Run captured payloads through the actual hook-to-supervisor route: matching root admits once; wrong UUID, child event, present-but-malformed agent_id, foreign-provider replay, duplicate/late startup and prior-invocation delivery do not bind or refresh state. Mutated negative fixtures are synthetic, clearly separate from native captures.
4. Exit normally and verify owned cleanup. This fixture certifies initial startup only; it does not require relaxing background-fork restrictions or establish resumed startup, transition admission, settling or final accounting.

GO for implementing this bounded admission rule after the fixture passes and independent review confirms actual routing. Until then, retain admission-disabled launcher mechanics. Existing attempts 05-07 remain useful supporting observations, not substitutes for this attributable launch.
