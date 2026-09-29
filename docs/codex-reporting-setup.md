# Codex response readiness setup

**Exact Codex CLI 0.153.0 hooks-only support passed acceptance at reviewed revision `56b84f9`.** The implemented milestone reports the terminal's **last observed root turn** using stable Codex CLI **>=0.153.0** synchronous hooks. It does not track the continuously selected history entry or collect metrics. The broader source investigation remains blocked; see the history below.

That acceptance used a disposable fixture. It does not establish that your installed OVRCR binary, running server, Codex configuration or native hook trust is ready. Follow [How to install Codex readiness reporting](codex-ready-installation.md) to prepare an exact-revision build, review the configuration change, protect existing sessions during a server transition, and verify the installed result.

After installation and native hook review, pick **codex** in the Dashboard agent
picker or launch a fresh interactive session inside an OVRCR terminal. Both use
the same managed reporting contract:

```sh
ovrcr agent run codex -- codex
```

A direct `codex` launch is untracked; the managed wrapper is required even when the hooks are configured and doctor reports a supported version. Running the wrapper in a terminal outside OVRCR also leaves reporting unavailable. Managed launches canonicalize `CODEX_HOME` and restart any CODEX_HOME-owned `app-server --managed-daemon` so Codex 0.158+ hook subprocesses inherit the current private channel (that daemon otherwise keeps a stale `OVRCR_AGENT_SOCKET` across TUI relaunches). Reporter attribution trusts the native process tree, including nested daemon-spawned helpers. The wrapper preserves ordinary native behavior when reporting cannot be admitted. Doctor reports Ready as available and Approval Input requests as available from verified root `PermissionRequest` hooks (`approval:{turn_id}`); questions remain unavailable until a distinct question open/close surface is verified. One working capability does not certify the other.

The legacy `agent run --provider codex -- codex` form also works. Native argv remains unchanged. The executable basename must be `codex`. Supported optional arguments are `--no-alt-screen`, `--full-auto`, and separate-token `--model/-m`, `--profile/-p`, `--sandbox/-s`, `--ask-for-approval/-a`, `--cd/-C` with a nonempty value that does not start with `-`. At most one initial prompt is supported; use `--` before a prompt matching a native subcommand. Trust hooks before submitting the first tracked prompt: an initial prompt supplied while Codex displays hook review may run untracked.

## Print and review configuration

```sh
ovrcr agent setup codex --print --settings /path/to/codex/config.toml
ovrcr agent doctor codex --json --settings /path/to/codex/config.toml
```

Omit `--settings` to print a standalone example. Setup prints TOML to stdout and instructions to stderr; it never writes configuration or trust. Review the output, then explicitly merge it into the intended Codex `config.toml`. Do not redirect output onto the input file. Composition preserves existing values, unrelated handlers and their order, including approval handlers and trust entries. It appends missing reporters and recognizes an already configured exact reporter; comments and formatting are not preserved. `just run` installs `~/.local/bin/ovrcr` and rewrites only the `ovrcr` token inside existing reporter commands to that binary. If the command already names that file, the bytes stay put, so the Codex trust hash stays valid. A path that actually changes needs native trust review again. Do not remove and re-add a handler just to move the path. Never copy trust hashes from retained evidence.

Run setup and doctor from the exact OVRCR binary whose absolute path the hooks will execute. Setup embeds its own executable path; output from a temporary build points to that temporary build. Doctor checks for reporters using its own path too. Moving the binary requires reviewing a new composition and any native trust prompt. Keep full compositions and backups private because they contain the supplied configuration.

Inventory and preserve any existing `hooks.json` alongside the intended TOML configuration. Codex discovers that file from each configuration layer's hook directory. `hooks.state` stores per-handler trust and enablement; it does not include or load files. Setup composes only the supplied TOML, and doctor does not inspect other hook sources.

Exact Codex 0.153.0 loads both JSON and TOML handlers, appending JSON declarations before TOML declarations within each layer. It warns when both contain handlers but disables neither source for that reason. Keep existing JSON bytes, handler indices and trust state unchanged when adding the generated TOML reporters. Synchronous handlers execute concurrently; displayed and collected-result order follow configured declaration order, not guaranteed execution or completion order. Preserve this native behavior. See the [pinned source rules](../research/issue-58-ready-install/completion/native-hook-source.md).

The [installed-binary check](codex-ready-installation.md#verify-the-installed-path-natively) uses a temporary OVRCR registry, socket and workspace while retaining the intended authorized Codex configuration; it can run without stopping the normal server or its sessions. Verify the effective native hook list and each handler's trust status separately from delivery. A listed handler still needing review is not evidence of dispatch.

The schema matches the [native Task 2 configuration](../research/codex-response-ready-acceptance/native-task-2/isolated-config.toml), without generating its trust state. Setup substitutes the current absolute OVRCR executable path and shell-quotes paths with spaces or apostrophes:

```toml
[[hooks.SessionStart]]
matcher = "startup|resume"
[[hooks.SessionStart.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"

[[hooks.UserPromptSubmit]]
[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"

[[hooks.PermissionRequest]]
[[hooks.PermissionRequest.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"

[[hooks.PreToolUse]]
[[hooks.PreToolUse.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"

[[hooks.PostToolUse]]
[[hooks.PostToolUse.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"

[[hooks.Interrupt]]
[[hooks.Interrupt.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"

[[hooks.SessionEnd]]
[[hooks.SessionEnd.hooks]]
type = "command"
command = "exec '/absolute/path/to/ovrcr' report codex --stdin"
```

Keep these command hooks synchronous and use direct `exec` so receiver process attribution reaches the native Codex parent. The helper returns successful exit with empty stdout as a native no-op response, including outside a managed launch. It never returns an approval decision. Review and individually trust the OVRCR reporters through Codex's native UI. The PermissionRequest reporter never returns an approval decision; preserve unrelated approval handlers and their order. Preserve the existing trust and enablement of unrelated JSON and plugin hooks, including any that still need review; trusting the OVRCR reporters does not establish that those other hooks ran. Setup and doctor do not bypass trust or change permission policy. To remove this setup, remove only the exact added reporter handlers, preserving other handlers and their declaration order.

Doctor invokes only the selected executable's bounded `--version` probe. Its default executable is `codex`; `--executable PATH` overrides it. It neither needs a running OVRCR server nor starts a provider conversation or reads credentials. `probe_status: supported` establishes stable patch-range compatibility. `configuration.status: supplied_file_supported` establishes the presence of expected synchronous commands in that supplied file. The `effective_configuration`, `hook_trust` and `delivery` fields remain `unverified`, including when those checks pass. Its `release_status` names the accepted implementation contract; it is not an installed-session result. `--session` does not inspect a Codex session; use `ovrcr session usage ID` for the current server snapshot and complete the [native installation check](codex-ready-installation.md#verify-the-installed-path-natively).

## What the indicator means

An authenticated root `UserPromptSubmit` binds the observed conversation and marks Busy. A matching root `PermissionRequest` opens an Approval Input request (`approval:{turn_id}`), so effective activity is WaitingInput and an input-needed alert can fire once per request identity; the next attributable root activity closes it. `PreToolUse`/`PostToolUse` alone never open a request. Its matching `Stop` marks ResponseReady with Observed quality once; this means the matching root Stop was observed and the response is available to review, not task success or confirmed settling. Other native Stop hooks may still affect provider behavior. `Interrupt` closes that turn as Idle without Ready. Native Ctrl-C produced Idle in the retained check; an earlier Escape attempt left Busy. Session end or reporter loss marks reporter health Unavailable. The snapshot retains its last activity as a historical observation; that retained activity does not establish current readiness.

The last Ready observation survives dashboard reconnect while the server remains alive. New observed activity replaces it; reporting becoming Unavailable preserves the last observation with Unavailable health. The next authenticated prompt clears Ready to Busy and may rebind after the prior turn closed. In-process backtracking without a new prompt does not change the displayed last-observed-turn identity. No foreground-history guarantee is implied.

Missing or unknown callbacks cannot synthesize Ready. An API error without a matching ending hook can leave Busy; a subsequent prompt while the previous turn remains open disables reporting. No timeout, transcript, screen text or metrics infer completion. Restart a fresh managed invocation after correcting hook configuration if reporting becomes unavailable.

Optional [desktop notifications](dashboard.md#desktop-notifications) can alert
from an active dashboard when a new root Ready response arrives, including for
terminals that are selected or shown in visible panes. They default off. Set
`desktop_notifications = true` in `dashboard.toml`, or press uppercase `N` in
dashboard browse mode for a session-only toggle. Alerts contain
terminal/project/workspace identity only. Attaching, reconnecting or enabling
alerts does not replay existing Ready observations. An alert retains the same
Observed meaning as the indicator. An independent `ready_sound` setting, or
uppercase `S` in browse mode, plays a [sound](dashboard.md#ready-sound) for the
same responses under the same rules.

Unread state is separate from activity and alerts. Each accepted managed Codex
root Ready observation replaces the terminal's previous unread observation.
Selecting the terminal, viewing its output, receiving Busy, or delivering an
alert does not mark it reviewed. Use the explicit [mark-reviewed
action](dashboard.md#unread-responses) when you have reviewed the response.
Acknowledgement names the observation you saw, so a delayed acknowledgement
cannot clear a newer response. Repeating an acknowledgement is safe.

The server retains at most one unread observation per terminal in memory.
Dashboard detach and reconnect preserve it while the server lives. Reporter loss
retains it alongside Unavailable health; an unread historical response does not
prove current readiness. Mark-reviewed changes only unread state, preserving
activity, quality, reporter health and native behavior. Removing the terminal or
losing the server loses its unread state; there is no durable response history.
Unread tracking requires no notification preference or additional hook setup.

Initial resume, picker/last, fork, exec, remote and unknown versions/options are unsupported for reporting and retain ordinary native behavior. Approval Input requests are available from matching root `PermissionRequest` with identity `approval:{turn_id}`; the next attributable root activity (`PreToolUse`/`PostToolUse`/`Stop`/`Interrupt`/new prompt) closes the set. Questions remain unavailable (no distinct question surface). This scope still excludes continuous selected-history tracking, subagent readiness, context/usage/cost metrics, confirmed completion and transcript collection. No Linux native GUI acceptance, metrics support or concurrent provider-launch throughput is claimed. Existing Claude acceptance does not cover Codex.

## Retained conversation recovery

Managed fresh compatible Codex CLI **0.153.x** launches capture the exact conversation UUID
and hook-supplied history path at an authenticated root startup or prompt. The
bounded native `session_meta` header must match the UUID and CLI source. Missing
or mismatched history leaves that identity unavailable; it never selects an older
saved conversation. OVRCR stores metadata only, not prompts or transcripts.

`ovrcr terminal reopen ID` runs managed `codex resume UUID` with no new prompt,
preserving the row, title and reference even across another immediate restart.
The shared ownership acknowledgement, capacity and stale-run rules still apply.
Missing history, executable, working directory, CODEX_HOME or an explicit profile
file prevents launching and retains a retryable row. Native resume failure also
retains the row; Retry never starts fresh. Start new conversation is separate.

Saved options are separate-token `--model/-m`, `--profile/-p`, `--sandbox/-s`,
`--ask-for-approval/-a`, absolute `--cd/-C`, and `--no-alt-screen`. Task prompts
are discarded. Other options, including `--full-auto`, inline configuration and
relative `--cd`, leave recovery unavailable without changing existing reporting.
CODEX_HOME (default `$HOME/.codex`) must be absolute and match on reopen; explicit
profiles must still exist at `$CODEX_HOME/<profile>.config.toml`. Configuration
contents and credentials are not copied or fingerprinted. The operator must keep
the referenced configuration/account and native history stable during recovery.
Native hook trust and approval settings are never bypassed.

**Initial resume reporting remains unavailable for the entire resumed invocation.**
The Dashboard says `Codex resume: reporting unavailable; attachment not confirmed`;
CLI inventory reports `reporting_unavailable: true` and `recovery.attached: false`.
Process launch does not prove conversation attachment. Old Busy, Ready, Unread,
input requests and timing are not restored. A later prompt in this resumed run
does not enable reporting or update its retained identity.

Recovery names the **last authoritative hook identity**, not continuously selected
history. Native backtracking can switch conversations without a hook until the
next prompt; an unobserved switch cannot update recovery. Initial raw resume,
picker and fork launches do not capture a new recoverable reference. Independent
native continuity, reporting and platform acceptance remains open in
[#128](https://github.com/xlyk/ovrcr/issues/128); controlled PTY tests do not certify
native Codex continuity.

## Acceptance evidence

The [unread acceptance record](../research/issue-61-unread/README.md) documents
exact-observation acknowledgement, reconnect and lifetime behavior, retained
failures, independent review, and separate automated/native results.

The [macOS native acceptance](../research/codex-response-ready-acceptance/native-final/README.md) used actual setup output and verified background Ready, reconnect with the same provider PID, a second Busy→Ready turn, child completion while the root remained Busy, next-prompt rebinding after backtracking, and retained Ready with Unavailable health at 50 columns after exit. Interruption evidence from Task 2 is retained for the unchanged hook code. Metrics remained null.

The [final verification record](../research/codex-response-ready-acceptance/final/README.md) records four passing hosted CI jobs in run `34570470022`: 663 macOS and 637 Linux tests, with 14 intentional ignores on each platform and separate passing capacity and memory gates. Linux evidence is automated. Local full-suite attempts failed and remain retained; no local full-suite pass is claimed. Failed native input attempts are also retained separately from the accepted retry. All 21 recorded processes and 16 process groups were verified absent, and task-owned socket, temporary root and private credential copy were removed.

## Earlier broader investigation

The [history invalidation probe](../research/codex-reporting-acceptance/history-invalidation.md) demonstrated that native backtracking can switch lineage without an observation hook until the next prompt. The [source contract](../research/codex-reporting-acceptance/source-contract.md) therefore remains blocked for continuous foreground identity and ongoing rollout metrics. Its earlier prohibition on Tasks 2–3 applies to that superseded broader scope. The separately approved hooks-first milestone uses last-observed-turn semantics and no transcript route. Historical attempts and failures remain evidence; they are not release acceptance for this implementation.

## Version compatibility

Doctor reports `compatible_versions`, `tested_versions`, `version_compatible` and `version_tested` separately. Later stable 0.153.x patches are eligible without claiming new native acceptance. New minor/major releases require review. See [provider version policy](agent-versions.md).
