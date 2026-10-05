# Cursor CLI harness: startup identity only

Detected `cursor-agent` launches through the managed OVRCR process supervisor.
Configure Cursor separately, then choose it in the Dashboard Agent picker or run:

```sh
ovrcr agent run cursor-agent -- cursor-agent
ovrcr agent setup cursor-agent --print
ovrcr agent doctor cursor-agent --json
```

An explicit `agent` executable may also be used with the `cursor-agent` provider.
Generic `agent` executables are not automatically detected as Cursor. Custom
presets keep their supplied argv; plain native launches are not adopted.

## Admission and capabilities

Startup reporting uses the installed CLI's capabilities at runtime, with no
release whitelist. Only fresh interactive launches with no arguments, or
`--model VALUE` / `--model=VALUE`, are eligible. The managed launch reads native
`--help` using the existing owned process-group probe: a one-second deadline,
16 KiB output limit, and OVRCR reporting credentials removed. Help must advertise
`--plugin-dir` as an option. Missing support, a failed/oversized probe,
unsupported argv, missing reservation/channel, or failed plugin creation leaves
native argv unchanged and reporting unavailable. Doctor checks executable
presence and optionally reads existing session inventory; it never executes the
provider or starts a Server. Advertised plugin support permits a startup attempt;
only a valid native-owned hook establishes the conversation binding.

An eligible launch receives one invocation-owned `--plugin-dir`. Its private
temporary directory contains a `.cursor-plugin/plugin.json` manifest with an
invocation-specific name and
`hooks/hooks.json` with one passive `sessionStart` command. It does not install a
plugin, rewrite global/project settings, configure auth, approve trust, change
native permissions, or replace other hook configuration. The hook emits no
response JSON, additional context, environment changes or follow-up prompt.
The existing Reporter removes the directory when reporting ends or native exit
releases its ownership; failed materialization removes partial files too.

The hook may bind only its native canonical UUIDv4 conversation identity, with
matching generation/session IDs and `is_background_agent=false`. Release metadata
does not admit or reject an identity. The private receiver requires a certified
native descendant. Duplicate startup delivery is idempotent; a different conversation
cannot replace the binding. Malformed, foreign, background or unsupported
reports are ignored. Bind refusal or an acknowledgement that remains unavailable
after the existing bounded receipt reread disables reporting.
Only whitelisted identity fields cross the private reporting channel: no email,
workspace data, transcript path, prompt/response text or model is forwarded.

The Server still observes native process lifecycle and the supervisor preserves
terminal input/output, signals and exit status. Agent activity stays **Unknown**.
Busy, Ready/Unread/review/alerts, approval/question Input requests, model/context/
usage/cost/quota, generated titles and conversation recovery are **unavailable**.
Terminal text and silence never infer them. Personal account quota can be read independently through the opt-in [Cursor dashboard adapter](provider-quota.md#cursor-personal-account); it does not add quota or activity fields to native session reports. No conversation recovery reference
is fabricated. Resume/continue, headless, ACP, management commands and other
options run natively with their original argv and reporting unavailable.
Dashboard detach/reattach uses existing PTY ownership; Reopen stays unavailable.

## Runtime contract and acceptance limits

The installed CLI must load `.cursor-plugin/plugin.json` and `hooks/hooks.json`
from the invocation-owned plugin directory. A fresh interactive `sessionStart`
hook must provide matching canonical UUIDv4 conversation, generation and session
identifiers, with `is_background_agent=false`. The receiver validates that
contract for each invocation; it does not infer compatibility from a release
string. An absent or incompatible hook cannot manufacture a binding. The hook's
version, email, workspace, transcript and model metadata are not forwarded.

Primary documentation describes [CLI parameters](https://cursor.com/docs/cli/reference/parameters),
[plugin format](https://cursor.com/docs/plugins), and [hooks](https://cursor.com/docs/hooks).
A submitted prompt can be denied by another hook; a `stop` hook can schedule
follow-up work. Neither establishes a terminal Ready boundary.

Automated fixtures use disposable executables through actual managed CLI,
Server, PTY, native helper and authenticated socket. They exercise runtime plugin
capability detection, native-owned binding, invalid/replacement identity,
unknown activity despite terminal text, native fallback, passive helper output
and cleanup. They do not prove delivery by genuine Cursor, model responses,
account/auth-mode support or native platforms. Native provider Dashboard/CLI
acceptance remains tracked in [#236](https://github.com/xlyk/ovrcr/issues/236). The [partial native macOS record](../research/cursor-native-acceptance-2026-10-05/README.md) verifies genuine startup binding and a managed CLI response; required gaps remain explicit.
Ordinary native launch/help execution can write Cursor's caches/logs and use its
existing auth/network behavior; acceptance records those effects. The Cursor
wire identity was appended in protocol 27; admission does not change the wire
protocol.
