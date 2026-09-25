# Conversation subjects

Approved design, 2026-09-25. Not implemented.

An Agent launch can show a short topic for the attached conversation, so a sidebar full of branch names can be scanned later. The topic may be revised once while it is forming. It then stays. It is not a live activity title, and it is not an application window title.

## Terms

A conversation subject is a short topic for one agent conversation. It may be shown as the session title until the user renames the session or dismisses that subject. It is not the original session name, not a manual title, and not an application title.

The original session name remains the name chosen at creation. On an Agent launch, a name typed at creation is that original name and does not pin the row. On a Terminal, a name typed at creation remains a manual title.

A manual title is a name set with Rename, or a name typed when creating a Terminal. It survives reconnect, server restart, and reopen. Clear removes it, restores the original session name, and dismisses the subject of the conversation attached at that moment.

Session identity does not follow any title. Commands, resume, and archive still target the session id.

## Display

The server computes the string the dashboard already shows. There is no generated badge and no new dashboard field.

For an Agent launch, the row shows the first of these that applies:

1. The manual title, if Rename has set one.
2. The conversation subject for the attached conversation, if one is saved and that conversation has not been dismissed.
3. The original session name.

A Terminal never shows a conversation subject. Application window titles, including status text such as "Thinking…", still do not change any row. Duplicate displayed titles still include the existing short session identifier.

A conversation switch on the same row, including a switch back, shows the subject stored for the conversation now attached. Rename pins the row and does not follow the switch. Stored subjects remain, but they are hidden while the pin is set. Clear dismisses only the conversation attached at that moment. Another conversation on that row can still show its own subject.

The row keeps its current name while a title call is in flight. It never shows "Generating…" or "Thinking…".

At most two accepted subjects are shown for one conversation, both while the subject is forming. After the window closes, later work in that conversation does not rename the row.

## How the words are produced

Only a live Agent launch is eligible. Claude, Codex, Pi, and Oh My Pi are eligible only when the retained conversation reference has a history file. A missing file, an ephemeral conversation, Grok, Hermes, a Terminal, and an agent started inside a Terminal are not eligible. They keep today's names.

OVRCR does not watch the terminal and does not ask the reporting extension for prompt or response text. Reporting stays free of prompts, replies, and tool data.

When an eligible history file changes, including when a reference first gains a history file, and the window is still open, OVRCR waits two seconds after the last change so a streaming write does not start a call per token. It does not start a call while a manual title is set. It then checks the file header the way resume already does. If the conversation id does not match, it reads no further and does not call Pi. Resume itself stays header-only. The title path may read the body into memory for one call. It must not persist that body.

The excerpt is provider-specific. It includes user and assistant text and skips tool output where the format marks it. It is at most the last eight such messages and at most 8 KiB. If the reader cannot find an assistant reply, there is no call. An unreadable file or an unknown shape is the same: no call, and no attempt is spent.

The call is a separate `pi --mode rpc` process. It is not a scheduled task, and it does not go through the task supervisor. It uses the Pi executable scheduled tasks already use: `OVRCR_PI_EXECUTABLE`, or `pi` on `PATH`. It passes the task runner's isolation flags, including `--offline`, `--no-extensions`, `--no-skills`, `--no-prompt-templates`, `--no-themes`, and `--no-approve`. The scheduled-task probe still reaches a model with `--offline` set, so this call passes that flag too. It does not load the task bash extension. Its working directory is an empty temporary directory, not the workspace. Thinking is off. The prompt asks for a few-word topic and forbids quotes, status, secrets, and explanation. The excerpt is appended to that prompt. The reply is the assistant text from the RPC `message_end` event the task runner already reads. Tool calls are not a title.

The model is `title_model` in `dashboard.toml`, or in the file selected by `OVRCR_DASHBOARD_CONFIG`. The spelling is `provider/model`, the same spelling tasks use. Both sides must be non-empty. The application composition reads it at server startup and passes it into the runtime. It does not belong in `config.toml`, because the server rewrites that file and would drop an unknown key. A missing or invalid value means no title calls. A change applies on server restart. There is no live reload.

The existing title cleaner must leave one line of at most 60 characters. A longer or empty reply is rejected. The first accepted reply becomes the subject. One later accepted reply may replace it. An identical reply is not a new subject. The window closes after the second subject is saved, or after the third attempt is recorded, whichever comes first. An attempt is a real Pi invocation made after an excerpt contained an assistant reply. A junk reply counts. A timeout counts. A missing file, a header mismatch, an excerpt with no assistant reply, a missing model, and a missing Pi binary do not count.

A call times out after 30 seconds. The server then kills that process group. One title call runs at a time. Other due sessions wait. They are not forgotten. A missing Pi binary does not retry in a loop for the rest of that server process. A restart may try again.

The temporary directory, including the Pi session file, is deleted when the call ends, whether it succeeded or failed. The excerpt and the prompt are not written to logs, alerts, or the retained record.

A result is kept only if the attempt started while the window was open, it is still for the same session and conversation, the user has not Renamed, that conversation has not been dismissed, and the session has not been archived. Otherwise it is dropped and not written.

## What is stored

The subject is not the manual title, and it is not the legacy `application_title` column. That column stays ignored.

Schema 7 adds a table owned by the retained session store, keyed by session and conversation id. Each row holds the topic, whether that conversation was dismissed, how many subjects were accepted, and how many attempts were used. Bump `SCHEMA_VERSION` and the version allowlist together. Offline readers must accept the new version and must not write it.

The row is written before the displayed title changes. If the write fails, the displayed title stays as it was, and OVRCR does not schedule another call until the history file changes again. A restart shows the last saved subject. Archive search uses the same effective title, so a saved subject remains findable after the process is gone.

Attempt counts are saved with the subject row. A restart does not buy three more calls for a closed window. An in-flight call is not resumed after a crash. The next eligible file change can retry only if the saved window is still open.

Clear on a row with an attached conversation dismisses that conversation and persists the dismissal. An in-flight result for it is dropped. Rename does not delete stored subjects.

Server shutdown kills the title process group and does not wait for the model. A subject that was not yet saved is discarded. A subject that was saved remains.

OVRCR cannot reliably detect a secret in the model's reply. The prompt forbids secrets, and the excerpt is not logged. If a secret lands in the title, Rename removes it from the display. There is no separate scanner.

## Tests

Test the server path that creates the session, reads the history file, and publishes the title. A helper that only formats a string is not sufficient. Use the live fixture, real SQLite, and a fake `pi` that speaks the RPC lines the task runner already understands. Do not call a real model in CI.

Required cases:

- An Agent creation name is shown and is not a pin. A Terminal creation name still pins.
- An application window title still does not change the row. The subject is not written to `application_title`.
- A fixture history file with one assistant reply produces one subject. The row changes only after the save. A failed save leaves the old name.
- A second accepted reply may replace the subject. A third does not. Three junk replies close the window with no subject. Restart does not reset the counts and does not call Pi again for a closed window.
- A header mismatch, a missing history file, an excerpt with no assistant reply, a missing `title_model`, and a missing Pi binary do not change the title. The first three do not spend an attempt. A missing binary does not retry in a loop.
- A result that arrives after Rename, Clear, a conversation switch, or archive is dropped.
- Switching from conversation A to B and back shows each conversation's own subject.
- The excerpt and the prompt do not appear in logs or in the retained record.
- Shutdown kills the title process group. The test checks the process group, not only the child handle.
- Grok, Hermes, a Terminal, and an agent started inside a Terminal do not spawn Pi.

The native dashboard check uses that same fake `pi` and a fixture history file, then checks the sidebar text. It does not require a provider credential.

## Docs that ship with the implementation

Update the stable-title section of `docs/dashboard.md`, the `dashboard.toml` example, and the schema note in `docs/development/architecture.md`. Add one sentence to the recovery docs: resume stays header-only, and the title path's ephemeral read must not persist transcript content.

`CONTEXT.md`, ADR 0001, and ADR 0005 are updated with this design.

## Out of scope

Terminals, an agent started inside a Terminal, Grok, and Hermes. Application-title following. A generated badge. A regenerate command. Scheduled-task records for titles. Persisting excerpts. Live reload of `title_model`. A model credential that is not the configured Pi model. Changing session identity.

## Rejected alternatives

The running agent could send the topic through reporting. That would avoid reading the history file. This design does not do that. The interactive reporting extension remains forbidden from sending prompts or replies.

OVRCR could call a model other than Pi, or read the title out of a provider's private session title. Neither is this design. Scraping the live terminal was rejected because it includes chrome and status text.
