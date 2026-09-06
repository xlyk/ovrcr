# Scheduled Pi tasks

Approved implementation contract, from the conversational plan.

## Product behavior

Deliver the task engine and CLI, background service, then a dedicated Tasks TUI view. Preserve existing terminal commands and dashboard behavior. Blocking Rust I/O and threads; no Tokio or SQLite.

- Persistent task definitions and history. Three concurrent runs by default, configurable; no concurrent executions of the same task. One active and at most one pending occurrence per task. One hour default timeout.
- Cron (five fields, explicit IANA timezone), anchored intervals, and one-time RFC3339 timestamps. Catch up missed times once; advancing a schedule and enqueueing are one atomic persistence operation. Manual runs do not alter the cadence. Pause cancels scheduled pending work only; resume uses the next future occurrence. Immutable queued/active task snapshots. Delete refuses active work, cancels pending work, preserves history.
- Task targets: registered project + remote + branch fetched for a fresh worktree, or fresh persistent scratch directory. Retain all working files until explicit guarded cleanup. Preserve branches on Git cleanup; scratch cleanup requires explicit confirmation.
- Pi 0.84.4 RPC baseline, one process and fresh conversation per run; explicit provider/model, default thinking off. Disable discovered extensions, skills, prompt templates, and project settings; continue loading project instruction files. Existing global Pi authentication/model configuration remains available.
- Prompt acknowledgement is not completion. Wait for agent_settled and inspect final assistant stopReason; provider failure, length truncation, and abort are not success. Pi internal retries stay within timeout. Clear queue, abort, close stdin, bounded process cleanup before final status. Record JSONL events and separate stderr.
- Run supervisor in the same binary holds per-run lifetime lock, consumes a parent control pipe, and interrupts Pi on parent loss. Restart reconciles old supervisors before admission. Interrupted executions are not replayed automatically. Cleanup failure must remain visible.
- CLI groups task/tasks, run/runs, service; global --json convention. Task CRUD/pause/resume/run/concurrency; run list/get/logs --follow/cancel/clean. Offline reads must not start server. Cursor-based log reads bounded below protocol frame cap; slow/disconnected viewers do not block durable output capture.
- User LaunchAgent on macOS; systemd user service on Linux. Login startup, restart on failure, clean shutdown stays stopped. Absolute paths and captured nonsecret environment; optional environment-file path. Never take over unmanaged running server, delete tasks on uninstall, or store credentials in task definitions.
- Dedicated Tasks TUI via Ctrl-t in browse mode: task list, all-field in-TUI creation/editing including multiline prompt, pause/resume/run/delete/concurrency, run history/live scrollable transcript/cancel/confirmed cleanup. Existing terminal-mode forwarding remains literal. Task output uses a separate control connection.

## Implementation ownership and shared interfaces

Task model owner: src/tasks.rs and tests/task_schedule.rs. Service owner: src/service.rs and tests/service.rs. Runner owner: src/task_runner.rs and tests/task_runner.rs. Integration owner: Cargo.toml, src/lib.rs, src/task_manager.rs, src/task_cli.rs, src/main.rs, src/protocol.rs, src/server.rs. TUI later has separate ownership.

Use these public types in tasks.rs (derive Clone, Debug, PartialEq, Eq, Serialize, Deserialize; ordinary externally tagged enums for bincode):

```rust
pub struct TaskId(pub u64);
pub struct RunId(pub u64);
pub enum Schedule { Cron { expression: String, timezone: String }, Interval { seconds: u64 }, Once { at: i64 } }
pub enum TaskTarget { Git { project: String, remote: String, branch: String }, Scratch }
pub struct TaskSpec { pub name: String, pub prompt: String, pub target: TaskTarget, pub schedule: Schedule, pub model: String, pub thinking: String, pub timeout_seconds: u64 }
pub struct Task { pub id: TaskId, pub spec: TaskSpec, pub enabled: bool, pub next_due_at: Option<i64> }
pub enum RunTrigger { Scheduled, Manual }
pub enum RunStatus { Queued, Preparing, Running, Cancelling, Succeeded, Failed, Cancelled, TimedOut, Interrupted, CleanupFailed }
pub struct Run { pub id: RunId, pub task_id: TaskId, pub spec: TaskSpec, pub trigger: RunTrigger, pub scheduled_at: i64, pub created_at: i64, pub started_at: Option<i64>, pub finished_at: Option<i64>, pub status: RunStatus, pub directory: Option<std::path::PathBuf>, pub workspace: Option<String>, pub base_commit: Option<String>, pub pi_version: Option<String>, pub session_file: Option<String>, pub error: Option<String> }
pub struct TaskStore { pub tasks: Vec<Task>, pub runs: Vec<Run>, pub max_concurrent: usize, /* private persisted ID counters allowed */ }
```

TaskStore API: default max_concurrent=3; load(path), save_atomic(path); create(spec, now)->Result<Task>; update(id,spec,now)->Result<Task>; pause(id,now)->Result<()>; resume(id,now)->Result<()>; delete(id,now)->Result<()>; enqueue(id,trigger,now)->Result<Run> coalesces existing queued; advance_due(now)->Result<bool>; admit(now)->Vec<Run> marks Preparing. Schedule::validate(), Schedule::next_after(timestamp)->Result<Option<i64>> (interval returns timestamp+seconds; admission advances intervals from previous due using arithmetic). TaskSpec::validate(). RunStatus::is_terminal(), RunStatus::occupies_slot(). All timestamps Unix seconds. ID newtypes Copy + Ord + Hash.

Free tasks helpers: tasks_dir(registry_path)->PathBuf uses registry_path.with_extension("tasks"); run_dir(tasks_dir,RunId)->PathBuf uses runs/<id>; read_run(run_dir)->Result<Run>; write_run(run_dir,&Run)->Result<()> atomically writes run.json; now()->i64. write_run creates run directory. Store state.toml tracks queue and cursor atomically; run.json is authoritative execution metadata once admitted. Server merges it into persisted run inventory. Do not add serde flatten/tagged enums incompatible with bincode.

Runner public API: supervise(run_dir:&Path,pi_executable:&Path)->anyhow::Result<()>; reads prepared run.json with directory populated, consumes stdin lines "cancel" or EOF (parent loss), owns run.lock flock, writes run.json/events.jsonl/stderr.log. A normal completion exits 0 after persisting status (run failure is metadata, supervisor failure nonzero). Add read_log(run_dir,offset:u64,max_bytes:usize)->Result<(Vec<u8>,u64)> bounded byte cursor. Pi binary override via OVRCR_PI_EXECUTABLE at manager spawn; otherwise resolve pi from PATH. Supervisor subcommand __task-runner RUN_DIR PI_EXECUTABLE is wired by integration owner.

Service public API should be self-contained, with clap Args/Subcommand types and run(command,json)->Result<()> callable by main. Add start_if_installed()->Result<bool> for connect_or_start before detached fallback; status must not start anything. Resolve RegistryPath and ServerPaths from existing modules. Service tests should use isolated paths and executable hooks, without controlling the user's actual service. Coordinate exact names before integration.

## Validation

TDD: meaningful failing behavior test first. Fake clock schedule tests; real temporary persistence; actual child processes and Git worktrees. RPC fixtures may replace external provider, never the production runner. Three parallel runs with independent cancellation and a fourth queued. Crash/EOF cleanup with process-group evidence. CLI parity and offline reads. TUI buffer/input tests and real GUI acceptance. Keep blocked OS/auth checks explicit. No unrequested commits, push, merge, or real production schedule installation. Write required work diary at closeout.
