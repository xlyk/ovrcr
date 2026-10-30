# Built-in Operator acceptance — 2026-10-29–30

The actual native GUI completed two ChatGPT subscription turns using the existing
stored sign-in. The second turn executed Bash and the matching OVRCR CLI, then
recalled the first reply. A separate local-provider GUI run proved concurrent
streams, independent Stop and background completion/Unread.

Accessibility copies omit trailing terminal padding; screenshots are unchanged.
Original captures remain in the private evidence directory.

## Source and executable identity

- Baseline: `54cc146f39344d8608dd81dbd9a7372f8298b875`.
- Production source: `c9b8227cccae53d34f2bd4fa44964ab82d17fcd6`.
- Final local postchecks: `10f8279df15298cdb2b0e360e79820acbb348cba`
  (acceptance header assertions only).
- Build: `cargo build -p ovrcr --bins --features gui,acceptance-diagnostics --locked --offline`,
  separate `target/provider-acceptance`, two jobs. Dev/test debug info and incremental
  compilation disabled to contain task-owned disk usage.
- The build receipt names HEAD `104c7246`: the reviewed header compatibility fix
  was present but uncommitted during compilation, then committed unchanged as
  `c9b8227c`. No production source changed in `10f8279d`.
- Native bundle: `target/OVRCR GUI.app`, assembled through the `scripts/gui.sh`
  equivalent with these frozen matching binaries after prior owners exited.
  [Hashes and build identity](attempt-2-bundle.json).
- CLI SHA256: `3070888da62c11ad823507ce922641a92cd906a70fb2110e1e606a84e8b7f7f9`.
- GUI SHA256: `b8f72b767efd86ebe3bcc19b981040631b909e0a329bc9dd02f8d4053c7cecc6`.

## Real ChatGPT provider

Kyle explicitly authorized actual ChatGPT GUI testing with the stored token.
No credential store existed inside this worktree; the authorized CLI sign-in was
in the default OVRCR instance. The disposable instance received an access-only
snapshot with refresh/id tokens omitted. A normal production `operator models
chatgpt` refresh later updated the original account record as the access token
approached expiry. Credentials, account records and private database backups are
excluded from this evidence package.

1. Native CUA submitted `Say exactly: OVRCR_CHATGPT_OK. Do not use tools.` using
   `chat_gpt / gpt-6.1-sol`. The GUI displayed `OVRCR_CHATGPT_OK` and **complete**.
   [Screenshot](03-corrected-chat.jpg), [accessibility](03-corrected-chat.ax.txt).
2. Native paste and Shift-Enter composed a multiline follow-up containing `界🙂`.
   [Draft screenshot](04-multiline-draft.jpg), [accessibility](04-multiline-draft.ax.txt).
3. The follow-up requested one Bash call printing `OVRCR_TOOL_OK` and its PID,
   then `"$OVRCR_EXECUTABLE" --json terminal list`. The actual tool succeeded
   (exit0); CLI output parsed as13 rows, including one global Operator row.
   The final reply recalled `OVRCR_CHATGPT_OK`, confirmed command success and
   rendered `界🙂` visibly. [Screenshot](05-live-tools-and-context.jpg),
   [accessibility](05-live-tools-and-context.ax.txt).
4. The journal recorded two Complete turns, Session13 runs1→2 and saved model
   context. There was one ToolCall and one successful ToolOutput in the follow-up.
   [Safe proof receipt](live-attempt-2-proof.json).

The first native attempt on `104c7246` failed with safe `kind=http` diagnostics.
A bounded transport investigation found HTTP200 with valid SSE and no
Content-Type header; Rig rejected that response before consuming the stream.
The regression failed on the original behavior. The fix supplies the SSE type
only for a successful headerless ChatGPT streaming POST to the configured
Responses route. Explicit wrong types, other statuses/routes/methods, malformed
streams, incomplete responses and Grok retain rejection behavior. TLS and
credentials are handled by the original transport.

The unauthenticated probes returned401. One explicit authorized SDK POST returned
200 and the exact marker before the GUI retest; default redirect count was not
measured. These diagnostic calls are separate from the two successful native
turns. No real Grok provider call was made.

## Controlled concurrent GUI check

This used a loopback provider and fake Grok OAuth in a fresh disposable instance;
it establishes application lifecycle behavior, not real Grok account access.

- Native chats A/B used `fixture-a`/`fixture-b`. Each executed one real Bash/CLI
  operation and streamed a partial reply before a fixture-controlled final chunk.
- Both journal states were Running concurrently. [Receipt](offline-concurrent-running.json),
  [screenshot](06-concurrent-streams.jpg), [accessibility](06-concurrent-streams.ax.txt).
- Native Ctrl-x interrupted A while B remained Running, with stopped proof only
  for A. [Receipt](offline-independent-stop.json).
- Releasing B while A stayed selected completed B and showed one unread marker.
  [Screenshot](08-background-unread.jpg), [accessibility](08-background-unread.ax.txt).
- Opening B showed **complete**, `B partial waiting B final` and cleared Unread.
  A remained Interrupted. [Screenshot](09-background-complete.jpg),
  [accessibility](09-background-complete.ax.txt).
- Final states were A=Interrupted/B=Complete, both workers stopped; each Bash
  execution marker occurred once. The provider observed two requests per chat
  (tool request plus continuation), not tool replay.
  [Journal proof](offline-concurrency-2-proof.json),
  [provider receipt](offline-provider-finished.json).

An earlier controlled fixture reached its600s deadline during a tool reconnect.
That failed attempt was preserved separately and cleaned up; it is not counted
as native concurrency acceptance.

## Cleanup

Both successful GUI launchers exited0 after closing their windows through CUA.
Each instance's12 ordinary session groups and two captured worker groups were
absent, along with GUI/Server/Dashboard processes, disposable directory and socket.
The live instance's copied access snapshot was removed with its directory.
The controlled provider holder exited0. [Live cleanup](live-attempt-2-cleanup.json),
[controlled cleanup](offline-concurrency-2-cleanup.json).
The user's installed executable and existing live Server were not replaced.

## Automated verification and remaining gates

| Scope | Observed result |
| --- | --- |
| Real compiled Operator E2E after header fix |11 passed;2 deliberate subprocess/helper entrypoints ignored |
| Worker unit tests after header fix |13 passed |
| Six affected native-terminal acceptance cases after header migration |6 passed individually |
| Current `10f8279d` nonroot workspace, all targets/features, serial |897 passed;1 explicit subprocess fixture ignored |
| Current root tail (`wip_save`, `workspace_identity`) |17 passed |
| Current strict workspace Clippy, all targets/features |passed |
| Current format check |passed |
| Exact private Rust1.97.1 workspace/all-target/all-feature check, offline/locked |passed |
| Documentation test command |exit0;0 doctests in all five packages |
| Node extension regression, unchanged files from `2f93e9ea` |106 passed |

The complete local workspace command did **not** pass. Preserved attempts stopped
at old header waits, automatic-title protocol timeouts or an optional agent
startup prompt. The longest serialized run on frozen `3421618d` passed1094 and
failed6 before later targets; six affected cases passed individually after
`10f8279d`. The six-case receipts name `c9b8227c` with those test edits
pending; the edits were committed unchanged as `10f8279d`. A full67-minute prefix
was not repeated. The offline Cursor suite
also retains one missing-receipt error; its focused three-subcase rerun passed,
but the full suite remains failed. These results must not be combined into a
passing full command.

Current-head hosted macOS/Linux CI, Linux capacity/memory, installed Pi and pinned
Cursor packaging gates remain required at PR publication. Native terminal
clipboard/IME/resize and real Grok access were not established by these focused
Operator checks. Later layout mockups are separate from this implemented layout.

Local raw gate logs and original command exits are retained under
`/private/tmp/rig-operator-final-iwloadr4`; diagnostic logs under
`/private/tmp/operator-provider-diagnostic`; private native receipts under
`/private/tmp/ovrcr-rig-native-wub3bd7s/evidence`. These host-local paths are recovery
pointers, not downloadable PR artifacts.
