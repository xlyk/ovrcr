# Use the built-in operator

## Connect your subscription

Use the same instance configuration as your Dashboard:

```sh
ovrcr operator login chatgpt
ovrcr operator models chatgpt
```

Replace `chatgpt` with `grok` for Grok. Complete the browser or device-code sign-in,
then choose a model returned by the command. Each instance connects one account
per provider.

To inspect or disconnect that account:

```sh
ovrcr operator status chatgpt
ovrcr operator logout chatgpt
```

Use the global `--json` flag for status, models or logout, for example
`ovrcr --json operator status grok`.

## Run concurrent conversations

1. In the Dashboard, press `Ctrl-g` to reach Browse, then `o`, or click the
   **Operator** control in the title bar. Saved conversations load above Projects
   in the existing sidebar before you open Operator.
2. Create a conversation with `Ctrl-n`. Choose ChatGPT or Grok and edit the model
   identifier if needed. Press Enter to create it.
3. Type your request and press Enter. The operator executes requested Bash and
   OVRCR CLI actions without per-command approval dialogs; existing CLI guards apply.
4. Use `Ctrl-l` or Tab to focus the conversation sidebar, then Up/Down and Enter
   to choose a conversation. Click a row to open it, or use `[+]` / `Ctrl-n` for
   manual provider/model creation while another conversation continues streaming.

The sidebar shows up to five one-line conversations. Scroll to browse longer lists;
click the Operator heading or use Left/Right with sidebar focus to collapse/expand.
The collapsed heading retains the conversation count. Provider/model details stay
in the conversation view.

Each conversation keeps its own draft, captured workspace/session context, scroll
position and status. Provider and model stay fixed after creation. Selecting a
different chat or returning to a terminal does not pause active work.

`Shift-Enter` inserts a newline; paste edits the draft without submitting.
`PageUp` and `PageDown` scroll the transcript. `Ctrl-g` returns to Browse while
preserving the draft. Completed background replies stay Unread until their
loaded tail is actually shown.

The target shown beside the composer is captured with each submitted message.
Check it before asking about "this workspace" or "this session". Later navigation
does not change a running turn's captured target.

## Set defaults and reopen history

Change Operator defaults in Settings, or in the instance's `dashboard.toml`:

```toml
[operator]
provider = "chat_gpt"
chat_gpt_model = "gpt-6.1-sol"
grok_model = "grok-4.7"
```

These values prefill new conversations; they do not change existing ones.
A model's presence in the selector or catalog does not prove that your account
can use it.

Conversation history survives Server restart. Opening saved history starts no
worker. New Conversation starts fresh model context and keeps previous chats.
If model context reaches its supported limit, start a new conversation.

Each active turn uses one of the existing fifty live-session slots. Saved idle
conversations use none. At capacity, stop or close an existing session through
the normal controls, then retry the preserved draft.

## Stop or recover a turn

Select the conversation and use its **Stop** control or `Ctrl-x`. Stop affects
that turn's model and Bash work; an OVRCR operation already accepted may finish.
Visible partial text and command outcomes remain in history. Generic
`terminal kill` and `terminal close` refuse Operator conversations; use native Stop.

To close a conversation, focus its sidebar row and press `x`, or hover and click
`[x]`. Confirming stops its exact retained run and archives the conversation while
preserving model history and the journal. A nonempty draft is discarded only after
archive is acknowledged. Cancellation or failure retains it. Closing one chat leaves
other conversations usable. Ordinary `x` in the message or model field remains text.
Archived sessions offers Unarchive for saved Operator conversations.

Wait for confirmed worker stop before submitting again. **Finishing / stop
unconfirmed** is not confirmation. After a crash, inspect the Operator rows with
`ovrcr --json terminal list`. Their `scope` identifies the conversation.

Only after independently confirming the old worker and background processes have
stopped, acknowledge its retained session:

```sh
ovrcr terminal acknowledge-stopped SESSION_ID
```

Acknowledgement also marks an unfinished turn Interrupted. Then submit fresh
instructions. Interrupted or uncertain commands are never
automatically replayed.

See the [runtime and authentication reference](development/operator.md) for Bash
limits, credential storage, provider contracts and offline acceptance.
