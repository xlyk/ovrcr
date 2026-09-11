# Initial admission implementation acceptance

Base: `304a2e0`. Saved commands test that base plus this initial-admission working tree. This unit enables initial binding only; it does not add activity mappings, transcript reads, metrics, final settling, provider configuration, or a collector.

## Source boundary and eligible argv

The source contract is `initial-admission-contract.md` and independently reviewed native attempt 08. Automated provider processes here are explicitly synthetic executable fixtures using those reviewed fields. These tests establish actual OVRCR launcher/hook/private-socket/runtime routing, not a new native Claude certification. The coordinator owns the subsequent live-provider gate.

Eligibility requires inherited stdin and stdout to be terminals, caller-selected executable basename `claude`, and the same executable returning exactly `2.1.267 (Claude Code)` from a bounded one-second version probe. The probe strips all five private/runtime reporting variables, has no terminal stdin, caps output, and kills its owned process group on timeout. Invocation uses the original path, preserving normal `~/.local/bin/claude` symlink semantics.

Supported options are the conservative allowlist: `--model`, `--permission-mode`, `--agent`, `--agents`, `--settings`, `--setting-sources`, `--system-prompt`, `--append-system-prompt`, `--name`/`-n`, `--strict-mcp-config`, `--verbose`, `--dangerously-skip-permissions`, and `--allow-dangerously-skip-permissions`. Value options support equals forms. One positional prompt is supported; explicit `--` disambiguates a prompt matching a native management command. Unknown flags, optional/variadic ambiguity, help/version, resume/history selection, management commands and other execution modes remain unchanged passthrough with admission unavailable. The command's long help states this limitation.

The UUID is generated and inserted once only after reservation, private directory/socket/receiver-thread setup, eligibility, and version verification succeed. Reporting setup or probe failure preserves original native argv. In particular the reviewed P1 channel-failure behavior still drops the reservation before untracked native spawn and clears inherited private/runtime credentials.

## Ownership and routing

The runtime authenticates and frames bounded opaque requests. A single initialization handoff installs the root-owned receiver; it is not a collector queue. Runtime contains no Claude interpretation. Its callback thread owns the receiver and lease; the main process loop supervises native independently. Native completion stops/joins the receiver, whose lease drop releases using the current binding. No second writer shares the watched supervisor stream.

`report claude --stdin-json` selects the invocation channel before the legacy activity parser and sends a provider/origin envelope plus the original JSON fields. Generic/compatibility calls cannot admit. The server checks provider Claude, Claude-hook origin, exact expected UUID, SessionStart/startup, and absence of any agent_id field. Named-root agent_type is permitted. This routing proof is not cryptographic attestation against malicious code already holding the private channel token.

A conditional Bind is attempted once. On a lost reply, the retained original operation ID is queried through authenticated status after a generation-safe supervisor reattachment; no second Bind operation is allocated. Duplicate startup does not publish activity or refresh runtime revisions. Matching root SessionEnd/clear and ambiguous root clear/resume/fork transitions permanently freeze admission, marking bound identity unavailable without releasing the lease or choosing a replacement. Other SessionEnd reasons remain observations. Wrong-UUID ordinary events and foreign/child callbacks do not mutate state. Native completion remains the release boundary.

Private requests are capped at 65,536 bytes. Authentication/body reads share a 200 ms deadline; root handler work ends by 800 ms from acceptance; response writes are bounded to 100 ms. Existing hook stdin/socket calls retain their one-second deadline. Release retains the existing one-second bound.

## Saved evidence

- 01: behavioral RED through actual CLI and server PTY, native lacked the supervisor-selected UUID.
- 02: first routing GREEN, 1 test.
- 03: expanded routing/argv GREEN, 2 tests, 1 ignored native fixture. 04: allowlist grammar GREEN, 1 test.
- 05: final initial-admission lifecycle GREEN, 3 tests, 1 ignored fixture (executed inside native PTYs). Assertions cover named-root binding, exact injected UUID/unchanged user values, wrong UUID, child/null/empty/numeric child metadata, explicit child events, foreign provider, generic route, duplicate startup preserving synthetic busy/revision 9, ordinary SessionEnd, clear/branch freeze, retained lease conflict/no native spawn, no reopening, removed old channel, old token/old UUID rejection against a new invocation, fresh UUID uniqueness, and unchanged native argv on unsupported modes/version/setup failures. The probe-timeout case verifies its owned process group is absent. Settings path with spaces and setting-sources empty value run through the actual eligible CLI path.
- 06: 2 library tests passed, including a real socket fixture withholding the Bind reply and asserting retry requests the original operation's status rather than binding again.
- 07: full CLI suite, 25 passed.
- 08: mechanics compatibility, 3 passed, 1 ignored fixture (executed inside PTYs): exact stdio/process ownership, Ctrl-C once, stop/continue, runtime pause/resume/kill, TERM/exit, private auth, P1 fail-open, release and cleanup retained.
- 09: report library, 5 passed. 10: runtime library, 90 passed.
- 11: affected packages check, all targets/all features, passed.
- 12: clippy found a string-slicing style lint; retained failure. 13: corrected byte-slicing expression, affected all-target/all-feature clippy passed with warnings denied.
- 14: formatting and diff checks recorded separately.

No full workspace suite, Linux test, native GUI test, or new live Claude session was run for this implementation unit. The busy sample in the duplicate-start test is an explicit synthetic runtime observation, not a newly enabled provider activity adapter.
