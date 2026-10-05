# Client startup checks

Accepted on 2026-10-05: Dashboard startup offers missing hooks and repairs to existing OVRCR hooks. It also checks required local assets and the running server before attaching.

- Run only on the no-command interactive Dashboard path. Reporting callbacks, server, help/version and inspection commands do not install or prompt.
- Detect installed local harnesses; offer Claude and Codex global hooks in their active native profiles. Managed providers with embedded or per-invocation reporting need no global installer. Use the locally installed harness; do not track releases.
- Show provider, target path and effect before each y/n offer. Default to no, including EOF and noninteractive input. Preserve native trust, approvals, unrelated handlers, comments where supported, file permissions and valid symlinks. Refuse malformed, oversized, dangling-link or concurrently changed input.
- Offer both absent hooks and repairs to OVRCR-owned hooks. Do not certify native hook trust merely from configuration.
- Ship Bridge install assets beside the CLI. Runtime does not compile Swift or require a source checkout. Retain signing, validation and replacement-identity guards. Bridge failures warn and leave Dashboard available.
- Compare the running server's captured executable build against the intended server executable, even when protocol/package versions match. Unknown legacy identity is explicit.
- Server restart has a separate y/n prompt naming the socket and warning that running sessions stop. Decline/noninteractive leaves the server untouched. Use a connection-bound controlled shutdown; never signal discovered PIDs or broad process names. Unsupported protocol/service upgrade remains an actionable refusal if controlled replacement cannot be proven.
- Preserve one server owner, one active Dashboard and fifty-session support. Tests use isolated native profiles, config, sockets and owned processes.
- `just run` builds and atomically refreshes the CLI/package, forwards arguments and honors CARGO_TARGET_DIR; client startup owns checks. No silent hook mutation in shell recipes.

2026-10-05 presentation follow-up: Kyle requested startup text match the rest of the app. Prompts share the Dashboard palette, separate action and location, emphasize the session-stop warning, and keep default-No key hints readable. Plain/no-color terminals retain the same decisions.
