# Codex response readiness setup

**Exact Codex CLI 0.153.0 hooks-only support passed acceptance at reviewed revision `56b84f9`.** The implemented milestone reports the terminal's **last observed root turn** using exact Codex CLI **0.153.0** synchronous hooks. It does not track the continuously selected history entry or collect metrics. The broader source investigation remains blocked; see the history below.

Inside an OVRCR terminal, launch a fresh interactive session:

```sh
ovrcr agent run codex -- codex
```

A direct `codex` launch is untracked; the managed wrapper is required.

The legacy `agent run --provider codex -- codex` form also works. Native argv remains unchanged. The executable basename must be `codex`. Supported optional arguments are `--no-alt-screen`, `--full-auto`, and separate-token `--model/-m`, `--profile/-p`, `--sandbox/-s`, `--ask-for-approval/-a`, `--cd/-C` with a nonempty value that does not start with `-`. At most one initial prompt is supported; use `--` before a prompt matching a native subcommand. Trust hooks before submitting the first tracked prompt: an initial prompt supplied while Codex displays hook review may run untracked.

## Print and review configuration

```sh
ovrcr agent setup codex --print --settings /path/to/codex/config.toml
ovrcr agent doctor codex --json --settings /path/to/codex/config.toml
```

Omit `--settings` to print a standalone example. Setup prints TOML to stdout and instructions to stderr; it never writes configuration or trust. Review the output, then explicitly merge it into the intended Codex `config.toml`. Do not redirect output onto the input file. Composition preserves existing values, unrelated handlers and their order, including approval handlers and trust entries. It appends missing reporters and recognizes an already configured exact reporter; comments and formatting are not preserved. If a previous installation points at a different OVRCR binary, remove only its exact reporter handlers yourself before composing the replacement. Never copy trust hashes from retained evidence.

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

Keep these command hooks synchronous and use direct `exec` so receiver process attribution reaches the native Codex parent. The helper returns successful exit with empty stdout as a native no-op response, including outside a managed launch. It never returns an approval decision. Review and trust the hooks through Codex's native UI; setup and doctor do not bypass trust or change permission policy. To remove this setup, remove only the exact added reporter handlers, preserving other handlers and their order.

Doctor invokes only the selected executable's bounded `--version` probe. Its default executable is `codex`; `--executable PATH` overrides it. It neither needs a running OVRCR server nor starts a provider conversation or reads credentials. Supplied TOML can establish that expected commands are present; effective configuration, native trust and hook delivery remain unverified. `--session` does not inspect a Codex session; use `ovrcr session usage ID` for the current server snapshot. Doctor names the accepted version contract; it does not verify effective hook trust or delivery in a user configuration.

## What the indicator means

An authenticated root `UserPromptSubmit` binds the observed conversation and marks Busy. Its matching `Stop` marks ResponseReady with Observed quality once; this means the matching root Stop was observed and the response is available to review, not task success or confirmed settling. Other native Stop hooks may still affect provider behavior. `Interrupt` closes that turn as Idle without Ready. Native Ctrl-C produced Idle in the retained check; an earlier Escape attempt left Busy. Session end or reporter loss marks reporter health Unavailable. The snapshot retains its last activity as a historical observation; that retained activity does not establish current readiness.

The last Ready observation survives dashboard reconnect while the server remains alive. New observed activity replaces it; reporting becoming Unavailable preserves the last observation with Unavailable health. The next authenticated prompt clears Ready to Busy and may rebind after the prior turn closed. In-process backtracking without a new prompt does not change the displayed last-observed-turn identity. No foreground-history guarantee is implied.

Missing or unknown callbacks cannot synthesize Ready. An API error without a matching ending hook can leave Busy; a subsequent prompt while the previous turn remains open disables reporting. No timeout, transcript, screen text or metrics infer completion. Restart a fresh managed invocation after correcting hook configuration if reporting becomes unavailable.

Initial resume, picker/last, fork, exec, remote and unknown versions/options are unsupported for reporting and retain ordinary native behavior. First release excludes continuous selected-history tracking, subagent readiness, WaitingInput, context/usage/cost metrics, confirmed completion, sound, unread acknowledgement and transcript collection. No Linux native GUI acceptance, metrics support or concurrent provider-launch throughput is claimed. Existing Claude acceptance does not cover Codex.

## Acceptance evidence

The [macOS native acceptance](../research/codex-response-ready-acceptance/native-final/README.md) used actual setup output and verified background Ready, reconnect with the same provider PID, a second Busy→Ready turn, child completion while the root remained Busy, next-prompt rebinding after backtracking, and retained Ready with Unavailable health at 50 columns after exit. Interruption evidence from Task 2 is retained for the unchanged hook code. Metrics remained null.

The [final verification record](../research/codex-response-ready-acceptance/final/README.md) records four passing hosted CI jobs in run `34570470022`: 663 macOS and 637 Linux tests, with 14 intentional ignores on each platform and separate passing capacity and memory gates. Linux evidence is automated. Local full-suite attempts failed and remain retained; no local full-suite pass is claimed. Failed native input attempts are also retained separately from the accepted retry. All 21 recorded processes and 16 process groups were verified absent, and task-owned socket, temporary root and private credential copy were removed.

## Earlier broader investigation

The [history invalidation probe](../research/codex-reporting-acceptance/history-invalidation.md) demonstrated that native backtracking can switch lineage without an observation hook until the next prompt. The [source contract](../research/codex-reporting-acceptance/source-contract.md) therefore remains blocked for continuous foreground identity and ongoing rollout metrics. Its earlier prohibition on Tasks 2–3 applies to that superseded broader scope. The separately approved hooks-first milestone uses last-observed-turn semantics and no transcript route. Historical attempts and failures remain evidence; they are not release acceptance for this implementation.
