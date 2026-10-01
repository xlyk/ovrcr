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

The following maps upstream 0.20.5 documentation to the OVRCR boundary. It is
source evidence, with **no tested native versions or auth modes**. Hermes owns
model selection and authentication; OVRCR does not validate either. The native
[model picker](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/website/docs/user-guide/configuring-models.md)
can open OAuth login and persist model defaults, so it is not a read-only probe.

| Native configuration choice | What upstream documents | OVRCR acceptance |
| --- | --- | --- |
| API-key provider | A provider credential and selected model; secret configuration belongs in the native `.env`. | Unverified; no credential inspection or provider request. |
| Hermes-managed OAuth | Native login/browser flows and credential storage in `auth.json`; credential sources depend on the provider. | Unverified; no login or token refresh. |
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

The existing OVRCR invocation supervisor forwards terminal input/output and signals,
preserves native exit status, and cleans up its owned native process group when the
invocation ends. The Server owns the Agent row and observes Running, Paused and Exited
process states. These observations do not certify an agent response or completed work.

Hermes activity stays **unknown**. Native activity/reporting, Ready/Unread, approval
and question Input requests, metrics, generated titles and conversation recovery
are unavailable. Terminal text and silence never imply those capabilities. No
conversation identity is fabricated and no reporting reservation displaces a
different reporter. Reopen remains unavailable under the existing recovery contract;
Dashboard detach/reattach keeps a surviving session through ordinary PTY ownership.

Doctor checks executable presence/permissions and, when requested, reads the existing
Server/offline session inventory. It does not start a Server or execute Hermes,
including `--version`. `found` is filesystem evidence only: installed version and
account/provider acceptance remain **unverified**, with no tested native versions.
The launcher has no version gate because it forwards a native command without a
version-dependent reporting adapter; this is not a compatibility certification.

## Source review and acceptance limits

The local upstream checkout was clean at
[`5ef1409f50484dddc38c9665b32a837ff1b191af`](https://github.com/NousResearch/hermes-agent/tree/5ef1409f50484dddc38c9665b32a837ff1b191af).
Its [version declaration](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/__init__.py)
is 0.20.5. The [CLI entry point](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/main.py)
defaults a bare `hermes` invocation to interactive chat. Its
[version fast path](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/hermes_cli/_startup_fast.py)
can check updates, so OVRCR diagnostics do not invoke it.

Native [event hooks](https://github.com/NousResearch/hermes-agent/blob/5ef1409f50484dddc38c9665b32a837ff1b191af/website/docs/user-guide/features/hooks.md)
exist: gateway events are gateway-only, while CLI plugin/shell hooks require native
configuration and shell-hook consent. This source review does not validate an OVRCR
root identity/reporting adapter and does not authorize changing those settings.
Reporting requires a separate verified integration before any capability is promised.

Automated tests use disposable shell executables through the actual managed CLI,
Server, socket and PTY. They prove OVRCR launch, input/output, argument and exit-status
preservation, failure handling, honest capabilities and unrelated-session isolation.
The controlled native-child fixture also checks interrupt forwarding, real
pause/resume, refused paused input, shell terminal-mode restoration, failure exit
status, private-socket cleanup and removal of the owned native process group.
Synthetic CUA can prove the Dashboard flow. Neither establishes genuine Hermes
model responses, native hook delivery or provider/auth-mode support. Issue #234 stays
open until separately authorized native Dashboard/CLI acceptance and all required
platform checks are recorded.
