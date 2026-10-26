# Architecture

| Location | Responsibility |
| --- | --- |
| `crates/ovrcr-protocol` | Shared wire types, validation, and framing. |
| `crates/ovrcr-terminal` | Terminal parsing, encoding, and screen/history primitives. |
| `crates/ovrcr-runtime` | Server, PTY/session ownership, registry persistence, Git/workspaces, and task execution. |
| `crates/ovrcr-tui` | Dashboard state, input routing, rendering, and task UI. |
| Root `src/` | Application composition, CLI/client, reporting, service integration, and optional GUI helper. |

One Reporter (`src/report/reporter.rs`) owns every managed invocation's reporting lifecycle: lease ownership, bind receipts, the single revision set, publish-or-disable, Producer fencing and the identity budget, pause and recovery, and teardown. `report/extension.rs` (Pi, Oh My Pi), `report/codex.rs`, and `report/admission.rs` (Claude) are frame receivers: they translate one provider's frames into observations and hold nothing else. Each receiver chooses which lifecycle parts its provider needs and they differ — only the extension receivers pause and recover, only Claude finalizes its accounting or retries a lost bind receipt, and only receivers with their own transport check the native root — so read the Reporter's own documentation for which is which. Add lifecycle behaviour to the Reporter, never to a receiver.

- Settings follow [ADR 0007](../adr/0007-one-settings-document-read-by-the-server.md): `ovrcr_runtime::settings` is the one loader of the settings document, and its typed `Settings` and `SettingsReport` live in `ovrcr-protocol` so the CLI and the Dashboard share them. Add a setting there, never a second parser.
- Runtime and TUI depend on protocol/terminal primitives, not on each other. Keep root facades thin; put implementations in the owning crate.
- Keep modules focused and exports small. Reuse existing helpers and dependencies before adding abstractions or libraries. Do not introduce Tokio, an agent SDK, or a new crate without a concrete requirement in the approved design. SQLite is already approved for project and workspace metadata (#112 / #113).
- Update constructors, exhaustive matches, public re-exports, CLI output, and optional-feature callers together when shared types change. Which providers may produce Ready, Unread, review targets and alerts is decided once by `AgentProvider::supports_readiness`; never compare a provider literal at a consumer.

Root `src/` and control-role tests speak to the server through `ovrcr_protocol::client` for request/response round trips; dashboard-role code that consumes events reads frames directly. Do not hand-roll `ClientMessage` frames for a plain request.

Project, workspace, and retained interactive-session metadata use SQLite as the
sole writable store. The database path is `registry.sqlite3` in the instance directory. Initial migration imports legacy projects/workspaces;
schema version 2 adds session metadata; version 3 adds archive disposition.
Neither migration changes settings or task storage.
Schema version 6 persists each workspace as a stable `id`, checkout `path`,
`git_identity`, and `setup_pending` flag. Schema version 7 adds retained
conversation subjects keyed by session and conversation id, with topic,
dismissal, accepted-count and attempt-count fields; offline readers accept it
read-only. Schema version 8 adds the instance identity and non-payload delegation
receipts, owned by the same SessionStore connection and migration transaction.
Schema version 9 protects versioned assignment metadata from older writers; its
upgrade preserves existing receipt identities and private evidence. Schema version
10 protects per-child control owner/epoch, human-input revision and bounded control
receipts from older writers; it preserves prior immutable intent digests. Missing
legacy input epochs never acquire current write authority.
Schema version 11 adds one bounded workflow-authority row in the same connection.
It records current parent Session/run/full binding, source identity, Server lifetime
and complete immutable transition history separately from assignment provenance.
Source credentials stay in memory. Existing records without certified workflow
authority remain visible read-only gaps; migration never invents ownership.
Schema version 14 protects immutable delivery declarations, obligation revisions
and archive receipts with reserved continuation capacity from older writers.
Schema version 15 protects coding workspace creation/process proofs and original
candidate provenance from older writers. Default new wire fields do not change
existing authenticated research document bytes.
Schema version 16 protects bounded trusted fallback capture/recovery references
and typed acceptance in the same assignment/version rows. It adds no table or
state owner. Task payloads stay in confined private files; frozen historical
evidence is not current source authentication. See [terminal fallback](../pi-terminal-fallback.md).
Schema version 17 protects bounded workflow dependency graphs, per-assignment input
witnesses and authenticated review contracts from older writers. Complete inherited
bytes and actual review source documents remain private task publications.
Schema version 18 protects bounded independent review references, applicable finite
corrective policy and original repair intents from older writers. Actual source
prose, findings and corrective text remain immutable private task files. Human-readable
absent review fields preserve old authenticated bytes; binary framing carries the option.
Schema version 19 additionally protects retained native resolved-model, profile/executable
identity and the generated-policy manifest from older writers. It adds no owner or
store. Native discovery creates no Session or Reporter lease. The actual postallocation
Reporter model/binding and every existing effect fence remain authoritative; preflight
is not installed TUI enforcement proof.
Immutable task contracts, briefs and submissions remain private files outside
child checkouts; see [Pi research assignments](../pi-research-assignments.md).
Schema version 19 protects Server-authenticated readable review-view references in
the existing review rounds. Upgrading preserves old retained rows and immutable
contracts/publications; absent legacy view metadata remains a visible continuation
gap. Complete readable images/context stay in private assignment files.
SQLite owns identity, path, and root-setup metadata. The stored
`branch` is captured metadata: the protected root's setup default (used when
default-branch detection is only a HEAD fallback or fails), and a feature
workspace's branch at creation or last explicit mutation—not a promise that
later checkout changes are written back.
Live checkout labels are derived observations; Git is the authority for the
current branch. `observe_registry` overlays those labels on a returned clone
for inventory and offline inspection and is not written back. Retained sessions
and task runs keep the workspace `id`; scheduled-task source branches stay as
configured.
`git_identity` matches a write-once generation marker in the checkout's Git
administration directory. Only creation and writable migration capture that
marker; inspection and removal never stamp an unknown checkout. This separates
a replacement worktree from the original even if the filesystem reuses its inode.
The checkout observer runs Git outside the session dispatcher and publishes
validated observations keyed by project and workspace identity. Dispatcher
snapshots use that cache without Git calls or registry writes.
After each migration transaction commits,
later starts and every save use SQLite only. Offline readers open the database
read-only. They read preserved TOML only if the database is absent, or both its
application ID and version are zero and it has no user objects. That uninitialized
state can be retried on startup. A foreign, nonempty unversioned, or unsupported
database fails without replacement and without falling back to TOML.
`dashboard.toml` and the `tasks/` directory stay outside this store.
`retained::SessionStore` owns durable session IDs, per-row run identities, title
metadata, original directory and launch kind, and boot-aware recovery state.
Current process runs remain `Arc<Session>` values; inactive rows allocate no PTY
or terminal parser. The existing mutation lock reserves one of fifty live slots
from admission through spawn publication. Reopening retires the prior run's
capability, view and history; run-tagged events and snapshots cannot affect its
replacement. No raw argv, environments, output, prompts, live activity or timing are
stored. Recovery stores only validated nonsecret references and options. Provider
recovery uses the [shared recovery contract](recovery.md); adapters never own
processes or a separate persistence path. Archive disposition is separate from
process-stop proof. Archive and unarchive invalidate the previous run identity,
so delayed recovery requests and process callbacks cannot cross these transitions.
Returned rows are stopped without launching; unresolved ownership still requires
acknowledgement before reopening.

Cursor cloud profiles are typed `Settings.cursor_cloud_accounts` values in the
same settings document. Ordinary Agent creation generates `cursor-cloud:ACCOUNT`
choices with explicit managed argv, independent of PATH discovery. Protocol 60
appends `AgentProvider::CursorCloud`; local Cursor remains `Cursor`. Preview
capabilities include no readiness, titles or conversation recovery. See
[Cursor cloud](../cursor-cloud.md) and [published source](../adr/0011-cursor-cloud-published-source.md).
