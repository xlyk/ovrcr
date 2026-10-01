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

Startup reporting is pinned to the source-inspected release
**2026.09.10-fd3934a**. Only fresh interactive launches with no arguments, or
`--model VALUE` / `--model=VALUE`, are eligible. The managed launch checks the
native `--version` using the existing bounded one-second probe, with OVRCR
reporting credentials removed. An unsupported/failed version probe, unsupported
argv, missing reservation/channel, or failed plugin creation leaves native argv
unchanged and reporting unavailable. Doctor checks executable presence and
optionally reads existing session inventory; it never executes the provider or
starts a Server. Neither executable presence nor the source pin proves native
account/provider acceptance.

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
matching generation/session IDs, the exact pinned hook version and
`is_background_agent=false`. The private receiver requires a certified native
descendant. Duplicate startup delivery is idempotent; a different conversation
cannot replace the binding. Malformed, foreign, background or unsupported
reports are ignored. Bind refusal or an acknowledgement that remains unavailable
after the existing bounded receipt reread disables reporting.
Only whitelisted identity fields cross the private reporting channel: no email,
workspace data, transcript path, prompt/response text or model is forwarded.

The Server still observes native process lifecycle and the supervisor preserves
terminal input/output, signals and exit status. Agent activity stays **Unknown**.
Busy, Ready/Unread/review/alerts, approval/question Input requests, model/context/
usage/cost/quota, generated titles and conversation recovery are **unavailable**.
Terminal text and silence never infer them. No conversation recovery reference
is fabricated. Resume/continue, headless, ACP, management commands and other
options run natively with their original argv and reporting unavailable.
Dashboard detach/reattach uses existing PTY ownership; Reopen stays unavailable.

## Source evidence and acceptance limits

The installed release was inspected as files, without running Cursor or reading
account/profile data. Its bundled CLI entry point accepts `--plugin-dir` and
prints the exact release token for `--version`. The bundled `commands/chat.ts`
creates a fresh UUID and fires `sessionStart` with matching conversation and
generation IDs; it omits this startup event on resume. The hook executor adds
the matching session ID and waits for plugin configuration readiness. The
plugin discovery implementation loads `.cursor-plugin/plugin.json` and
`hooks/hooks.json`.

Bundle SHA-256 references for reproducing this source review:

| Bundle | SHA-256 |
| --- | --- |
| `index.js` | `230df5356d29c26fae77aeed83bd705b2ddd5fd5d1abccd3eb8c8b8c24e23468` |
| `1186.index.js` (chat) | `85f65f8083afa05663e4a16255e218b1e15edc701e1fb272b5b19b6fab17d4a0` |
| `190.index.js` (hooks) | `c8c67a674fb48f263bab61409b0624f1373ba66c45dcaad7c5d0c7100ae8815a` |
| `6081.index.js` (plugin hook readiness) | `dbd66cdfabb3c314c5910a0cccbe577b2e634cc0b0a53ab99310d53bfe040d7a` |

Current primary documentation describes [CLI parameters](https://cursor.com/docs/cli/reference/parameters),
[plugin format](https://cursor.com/docs/plugins), and [hooks](https://cursor.com/docs/hooks).
Those evolving docs are supporting references, not a compatibility claim for
other versions. A submitted prompt can be denied by another hook; a `stop` hook
can schedule follow-up work. Neither establishes a terminal Ready boundary.

Automated fixtures use disposable executables through actual managed CLI,
Server, PTY, native helper and authenticated socket. They exercise native-owned
binding, invalid/replacement identity, unknown activity despite terminal text,
native fallback, passive helper output and cleanup. They do not prove delivery
by genuine Cursor, model responses, account/auth-mode support or other releases.
Native provider Dashboard/CLI acceptance and platform checks remain open in
[#236](https://github.com/xlyk/ovrcr/issues/236). Ordinary native launch/version
execution can write Cursor's caches/logs and use its existing auth/network
behavior; separately authorized native acceptance must record those effects.
This partial harness adds the appended Cursor wire identity in protocol 27.
