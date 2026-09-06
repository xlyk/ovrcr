# Task 1: Serialize process-group controls and protect lifecycle state

Status: DONE_WITH_CONCERNS

## RED

Command:

```text
rtk proxy cargo test --lib session::tests::pause_resume_phase_and_input_admission -- --exact --nocapture
```

The test failed to compile with the expected missing `SessionPhase::Paused` variant and `Session::set_paused` method errors (8 unresolved references).

## GREEN

Command:

```text
rtk proxy cargo test --lib session::tests::pause_resume_phase_and_input_admission -- --exact --nocapture
```

Result: 1 passed, 0 failed.

Command:

```text
rtk proxy cargo test --lib pause_resume_ -- --nocapture
```

Result: 3 passed, 0 failed.

Additional verification:

- `rtk proxy cargo check --all-targets --all-features`: exit 0.
- `rtk proxy cargo fmt -- --check`: exit 0.
- `rtk proxy git diff --check`: exit 0.

## Files

- `src/session.rs`: added `SessionPhase::Paused`, serialized pause/resume controls under `terminate_lock`, ESRCH-aware signaling results, shared write/send admission checks, and the three requested PTY/state tests with cleanup guards.
- `src/main.rs`: added the required `Paused` arm to the terminal phase serialization match.

## Concerns

- Task 4 owns acceptance evidence for descendants actually stopping and resuming. These Task 1 tests validate lifecycle state, input admission, race ordering, and process-group ownership boundaries without claiming syscall-level descendant-stop proof.
- Server and UI pause commands remain intentionally absent per the Task 1 scope.

## Commit

`0fe6ed9 feat: add session pause and resume controls`
