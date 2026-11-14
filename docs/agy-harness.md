# Antigravity CLI

Install Antigravity's `agy` executable on `PATH` and configure its native account or
model provider. Open **New agent session**, choose a registered branch and **agy**,
and leave **Model** on **Harness default** or choose an entry from `agy models`.
A blank prompt opens the native terminal. A first prompt starts a turn and leaves
that terminal open for the next one.

To launch the same managed session from the CLI:

```sh
ovrcr terminal create --project PROJECT --workspace BRANCH -- \
  ovrcr agent run agy -- agy
```

Managed interactive launches add `--dangerously-skip-permissions` by default. This
allows the agent to approve tool requests automatically. To choose a native mode
explicitly, use a managed command such as `ovrcr agent run agy -- agy --mode plan`.
OVRCR leaves raw configured launch commands under the user's control.

## Connect and inspect reporting

Choose **Connect agent status** to review the changes to your Antigravity profile.
The Stop hook goes in `~/.gemini/config/hooks.json`. The foreground status renderer
goes in `~/.gemini/antigravity-cli/settings.json`. Setup preserves existing hook
groups and passes native status input through to an existing renderer.

```sh
ovrcr agent setup agy --print
ovrcr agent doctor agy --json
ovrcr agent doctor agy --session SESSION_ID --json
```

The foreground callback identifies the active conversation and reports its native
model, activity and title. A manual session title takes precedence. After an
observed working turn, a successful, fully idle Stop followed by foreground idle
marks its response Ready and unread. An interrupted turn creates no new alert.
The previous unread response stays unread until reviewed. Process supervision
and terminal input remain available when callbacks are missing. The adapter discards
prompt text, tool arguments, account identity and quota information from its reports.

## Reopen a conversation

Run `ovrcr terminal reopen SESSION_ID` to start a new PTY in the retained workspace.
OVRCR checks the recorded profile and exact UUID, launches `agy --conversation UUID`,
and waits for the matching foreground callback before showing the conversation as
attached. A failed check leaves the retained row available for diagnosis. The original
prompt is never replayed.

Google documents [CLI arguments](https://antigravity.google/docs/cli/reference/),
[status fields](https://antigravity.google/docs/cli/statusline/) and
[hook semantics](https://antigravity.google/docs/hooks?tab=cli).
