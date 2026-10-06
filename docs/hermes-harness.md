# Hermes interactive harness

An executable named `hermes` on `PATH` appears in the Dashboard Agent picker.
OVRCR launches it through `ovrcr agent run hermes -- /resolved/path/hermes`.
Explicit `[[agents]]` overrides keep their command unchanged. CLI users can create
the same recognized Agent session:

```sh
ovrcr terminal create --project PROJECT --workspace BRANCH -- \
  ovrcr agent run hermes -- hermes
ovrcr agent setup hermes --print
ovrcr agent doctor hermes --json
ovrcr agent doctor hermes --session SESSION_ID --json
```

Install and configure Hermes separately using its native instructions. OVRCR needs
no Hermes hooks or settings and rejects `--settings` for Hermes setup/doctor. It
does not install plugins, approve hooks, copy credentials, select a model, or change
the native profile. Native arguments, including profile selections, pass through
unchanged. Ordinary Hermes startup and commands still have their native effects.

## Native configuration and profiles

The following maps reviewed upstream documentation to the OVRCR boundary.
The [2026-10-05 macOS acceptance](../research/hermes-native-acceptance-2026-10-05/README.md)
records the locally installed Hermes command, its configured `xai-oauth` default
and two genuine `grok-4.6` responses. It does not certify other configurations. Hermes owns
model selection and authentication; OVRCR does not validate either. The native
[model picker](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/website/docs/user-guide/configuring-models.md)
can open OAuth login and persist model defaults, so it is not a read-only probe.

| Native configuration choice | What upstream documents | OVRCR acceptance |
| --- | --- | --- |
| API-key provider | A provider credential and selected model; secret configuration belongs in the native `.env`. | Unverified; no credential inspection or provider request. |
| Hermes-managed OAuth | Native login/browser flows and credential storage in `auth.json`; credential sources depend on the provider. | The existing configured `xai-oauth` default responded in the bounded macOS run; other OAuth modes and login/refresh flows remain unverified. |
| Custom endpoint | A configured endpoint and model, with credentials where required. | Unverified; no endpoint connectivity or model-response check. |

Native [configuration](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/website/docs/user-guide/configuration.md)
places model defaults in `config.yaml` under the chosen Hermes home. Doctor's
`found` result does not establish that these files exist, that authentication is
valid, or that the selected model can respond. Setup/doctor do not read those
native files or determine the active profile.

Upstream [profiles](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/website/docs/user-guide/profiles.md)
separate Hermes state through `HERMES_HOME`; working directories are a different
setting. Upstream advises a separate profile for every concurrent agent because
agents write memory into that home. An OVRCR workspace does not create this
separation. The detected picker launch uses Hermes' native default/profile
resolution, including its sticky profile selection. OVRCR never creates or
clones a profile or copies its credentials.

For an **already configured, dedicated profile**, pass its selection unchanged:

```sh
ovrcr terminal create --project PROJECT --workspace BRANCH -- \
  ovrcr agent run hermes -- hermes -p DEDICATED_PROFILE chat
```

An explicit `[[agents]]` command can use the same managed argv. Create/configure
the profile separately through native Hermes; those actions can write state and
credentials and are outside OVRCR setup. Profile separation is not a sandbox.

## Current capabilities

The invocation supervisor still forwards terminal input/output and signals, preserves
native exit status, and cleans up its owned process group. The Server observes
Running, Paused and Exited. Those process states do not certify an agent response.

`ovrcr agent setup hermes` appends reporting hooks to the selected Hermes profile
when `config.yaml` has no `hooks:` key, and allowlists only that command. Set
`HERMES_HOME` to repeat setup for another profile. The hook process keeps session
id, turn id, model, approval id and token counts. It drops prompts, transcripts,
tool arguments and raw provider payloads.

After setup, a managed `hermes` or `hermes chat` launch can report:

- Busy from `pre_api_request`, and Ready from `post_llm_call` when that event
  includes a turn id. Interrupted turns stay unknown. Ready is not inferred from
  silence or terminal text.
- Approval Input requests from `pre_approval_request` / `post_approval_response`.
  Questions that are not those hooks stay unavailable.
- Token totals summed from `post_api_request`. Cost stays unavailable because that
  summary has no USD field.
- A generated title by reading the `title` column of that session row in `state.db`
  and showing it as the session title. This does not wait for a Dashboard and does
  not call another model.
- Resume with `hermes --resume <session id>`, plus `-p <profile>` when the database
  is under `profiles/<name>/`. A launch that never reported a session id cannot reopen.

A command that is not interactive Hermes chat keeps process supervision and does
not bind a conversation. Dashboard detach/reattach still keeps a surviving session
through ordinary PTY ownership.

Doctor checks executable presence/permissions and, when requested, reads the existing
Server/offline session inventory. It does not start a Server or execute Hermes,
including `--version`. `found` is filesystem evidence only. Doctor leaves its runtime version/auth/model
probe unverified, including after a separately recorded acceptance run; it does
not inspect the current native configuration or credentials. The launcher has no
version gate: it supervises whatever native command is installed at runtime and
forwards its arguments unchanged. Recorded versions identify observations, not a
maintained release allowlist or a compatibility certification.

## Source review and acceptance limits

The local upstream checkout was clean at
[`5ef1409f50484dddc38c9665b32a837ff1b191af`](https://github.com/NousResearch/hermes-agent/tree/5ef1409f50484dddc38c9665b32a837ff1b191af).
Its [version declaration](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/__init__.py)
is 0.20.5. The [CLI entry point](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/main.py)
defaults a bare `hermes` invocation to interactive chat. Its
[version fast path](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/_startup_fast.py)
can check updates, so OVRCR diagnostics do not invoke it.

Native [event hooks](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/website/docs/user-guide/features/hooks.md)
are the reporting source. Gateway events stay gateway-only. `ovrcr agent setup hermes`
writes the CLI shell hooks and their allowlist entries; it does not set
`hooks_auto_accept` and does not pass `--accept-hooks`.

Automated tests use disposable shell executables through the actual managed CLI,
Server, socket and PTY. They prove OVRCR launch, input/output, argument and exit-status
preservation, failure handling, honest capabilities and unrelated-session isolation.
The controlled native-child fixture also checks interrupt forwarding, real
pause/resume, refused paused input, shell terminal-mode restoration, failure exit
status, private-socket cleanup and removal of the owned native process group.
Synthetic CUA can prove the Dashboard flow. It does not establish genuine Hermes
model responses or provider/auth-mode support. The separately authorized
[2026-10-05 native record](../research/hermes-native-acceptance-2026-10-05/README.md)
proves bounded Dashboard/CLI responses, process controls and native exit on macOS.
That run also exposed a GUI-helper close panic and a stale post-CLI-control capture.
The close path joined the PTY reader inside the egui frame, which holds the context
lock the reader needs for `request_repaint`. Shutdown now happens from `on_exit`,
after that frame returns. A 2026-10-06 recheck of this helper showed CLI pause,
resume and exit in the live window, then a normal close that exited 0 and removed
the disposable demo.

A managed CLI launch on the default profile bound session id
`20261006_103930_514583`, went Busy, showed the marker reply, and became
`response_ready` with Unread set. The session title is the `title` column Hermes
writes for that row, applied without a Dashboard or another model call.
Non-approval questions and live replies on the obsidian and researcher profiles
remain unverified. Hooks are installed for all three profiles.
