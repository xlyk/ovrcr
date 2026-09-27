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

The row follows the conversation reference OVRCR has recorded. It does not follow a switch OVRCR did not see. When that reference changes, including a switch back, the row shows the subject stored for the newly recorded conversation. That recompute does not require a title call. Pi and Oh My Pi already replace the saved reference on an in-place switch, including the history path. A silent Codex history switch does not update the saved reference, so the row keeps the subject for the conversation still on record. Rename pins the row and does not follow a recorded switch. Stored subjects remain, but they are hidden while the pin is set. Clear dismisses only the conversation recorded at that moment. Another recorded conversation on that row can still show its own subject.

The row keeps its current name while a title call is in flight. It never shows "Generating…" or "Thinking…".

At most two accepted subjects are shown for one conversation, both while the subject is forming. After the window closes, later work in that conversation does not rename the row.

## How the words are produced

Only a live Agent launch is eligible. Claude, Codex, Pi, and Oh My Pi are eligible only when the retained conversation reference has a history file. A missing file, an ephemeral conversation, Grok, Hermes, a Terminal, and an agent started inside a Terminal are not eligible. They keep today's names.

OVRCR does not watch the terminal and does not ask the reporting extension for prompt or response text. Reporting stays free of prompts, replies, and tool data.

While a live eligible session has an open window and no manual title, the server stats that history file every two seconds. A new length, a new modification time, or a newly recorded path makes the session due. There is no file-watch library. A call does not start while a manual title is set.

Identity is the check resume already uses for that provider. It is not one shared header. Pi and Oh My Pi require a first line of type `session` whose id matches. Codex requires a first line of type `session_meta` whose payload id matches. Those two checks read only that first line. If it does not match, OVRCR does not read the rest and does not call Pi. Claude resume does not read a header. A Claude first line of type `mode` is not a mismatch. Resume itself stays header-only.

After the Pi or Codex first-line check, and for every eligible Claude file, the title path reads at most 256 KiB from the end of the file. It does not load the rest. Claude is eligible only if a record in that tail has `sessionId` equal to the conversation id. From the tail it takes user and assistant text, skipping tool output where the format marks it, up to the last eight such messages and 8 KiB. If that window has no assistant reply, or no matching Claude `sessionId`, there is no call and no attempt is spent. An unreadable file is the same. The excerpt stays in memory for that call. It is not persisted.

The call is a separate `pi --mode rpc` process. It is not a scheduled task, and it does not go through the task supervisor. It uses the Pi executable scheduled tasks already use: `OVRCR_PI_EXECUTABLE`, or `pi` on `PATH`. It passes the task runner's isolation flags, including `--offline`, `--no-extensions`, `--no-skills`, `--no-prompt-templates`, `--no-themes`, and `--no-approve`. The scheduled-task probe still reaches a model with `--offline` set, so this call passes that flag too. It does not load the task bash extension. Its working directory is a subdirectory of `titles` beside the server socket, not the workspace. Thinking is off. The prompt asks for a few-word topic and forbids quotes, status, secrets, and explanation. The excerpt is appended to that prompt. The reply is the assistant text from the RPC `message_end` event the task runner already reads. Tool calls are not a title.

The model is `title_model` in `dashboard.toml`, or in the file selected by `OVRCR_DASHBOARD_CONFIG`. The spelling is `provider/model`, the same spelling tasks use. Both sides must be non-empty. The `ovrcr server` process reads that file at its own startup, using the same path resolution as the dashboard, and keeps the string in the runtime. The dashboard does not send it. The installed service starts that same server process, so it sees the setting when the server's environment resolves the same path. A custom `OVRCR_DASHBOARD_CONFIG` missing from the service environment falls back to `dashboard.toml` beside `config.toml`. The key does not belong in `config.toml`, because the server rewrites that file and would drop an unknown key. A missing or invalid value means no title calls. A change applies on server restart. There is no live reload.

The existing title cleaner must leave one line of at most 60 characters. A longer or empty reply is rejected. The first accepted reply becomes the subject. One later accepted reply may replace it. An identical reply is not a new subject. The window closes after the second subject is saved, or after the third attempt is recorded, whichever comes first. An attempt is a real Pi invocation made after an excerpt contained an assistant reply. A junk reply counts. A timeout counts. A missing file, an identity mismatch, an excerpt with no assistant reply, a missing model, and a missing Pi binary do not count.

A call times out after 30 seconds. The server then kills that process group. One title call runs at a time. A session that becomes due during a call runs when that call ends, even if its file does not change again. It is not forgotten and not skipped. A missing Pi binary does not retry in a loop for the rest of that server process. A restart may try again.

Each call uses a subdirectory of `titles` beside the server socket. The server deletes that `titles` directory on startup, so a crash does not leave the excerpt on disk. The call also deletes its subdirectory when it ends, whether it succeeded or failed. The excerpt and the prompt are not written to logs, alerts, or the retained record.

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
- An identity mismatch, a missing history file, an excerpt with no assistant reply, a missing `title_model`, and a missing Pi binary do not change the title. The first three do not spend an attempt. A missing binary does not retry in a loop.
- A Claude history whose first line is `mode`, and whose later records carry the matching `sessionId`, can produce a subject. The Pi `session` header check is not applied to it.
- A history larger than 256 KiB still produces a subject from its tail. The test bounds the bytes read.
- The `server` command reads `title_model` from the dashboard settings path. The dashboard does not send that setting.
- A result that arrives after Rename, Clear, a recorded conversation switch, or archive is dropped.
- A recorded switch from conversation A to B and back shows each conversation's own subject without a new Pi call. A Codex reference that does not change keeps the old subject.
- A `titles` directory left beside the socket is gone after the next server start, and its contents are not in the retained record.
- The excerpt and the prompt do not appear in logs or in the retained record.
- Shutdown kills the title process group. The test checks the process group, not only the child handle.
- Grok, Hermes, a Terminal, and an agent started inside a Terminal do not spawn Pi.

The native dashboard check uses that same fake `pi` and a fixture history file, then checks the sidebar text. It does not require a provider credential.

## Docs that ship with the implementation

Update the stable-title section of `docs/dashboard.md`, the `dashboard.toml` example, and the schema note in `docs/development/architecture.md`. Say there that a silent Codex history switch does not change the displayed subject. Add one sentence to the recovery docs: resume stays header-only, and the title path's ephemeral read must not persist transcript content.

`CONTEXT.md`, ADR 0001, and ADR 0005 are updated with this design.

## Out of scope

Terminals, an agent started inside a Terminal, Grok, and Hermes. Application-title following. A generated badge. A regenerate command. Scheduled-task records for titles. Persisting excerpts. Live reload of `title_model`. A model credential that is not the configured Pi model. Changing session identity.

## Rejected alternatives

The running agent could send the topic through reporting. That would avoid reading the history file. This design does not do that. The interactive reporting extension remains forbidden from sending prompts or replies.

OVRCR could call a model other than Pi, or read the title out of a provider's private session title. Neither is this design. Scraping the live terminal was rejected because it includes chrome and status text. A file-watch library, and a dashboard message that carries `title_model`, are not part of this design. The server stats eligible files and reads the setting itself.
