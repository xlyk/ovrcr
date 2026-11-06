# #404 retained Agent jump implementation

## Revision and scope

- Branch: `codex/sidebar-session-jump-404`
- Base/integration tip merged: `4829a72d286f833dab22baae513d34a6e7177c8a`
- Commit: `77616944a592c098f40848bcdb6490c3d125786a`
- Worktree: `/private/tmp/ovrcr-401-t404` (clean after commit)
- Target directory: `/private/tmp/ovrcr-401-build-404`

Retained `Exited`, `Stopped`, and `Interrupted` Agent rows now share the visible code order with running Agents. Paused rows remain excluded. Each captured target keeps its session, run, and running/retained class; a run replacement, archive, or a Running-to-retained phase change makes the old label unavailable. Retained selection reuses `select_session` and `SetView`, stays in Browse, arms no typing intent, and suppresses automatic recovery for the exact retained run. Running selection still uses the existing modifier-release, acknowledged-view, and input-drain path.

The new public Dashboard tests also verify the retained row's rendered status/title text remains stable, no Input or lifecycle request is emitted, recoverable Interrupted rows do not enqueue `RecoverSession`, and a later Running choice remains gated. The compiled acceptance launches only local fixture shell commands; no provider turn or user session was used.

## Verification

- Red: the new public render test failed on the original behavior because the retained title had no label.
- Red: the Interrupted-retained test exposed an unexpected `RecoverSession` queued beside `SetView`.
- Green: `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-404 CARGO_HOME=/Users/xlyk/.cargo HOME=/private/tmp/ovrcr-401-implementation/home-404 cargo test --offline -p ovrcr --test tui session_jump_retained:: -- --nocapture` — 4 passed, 387 filtered.
- Green: `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-404 CARGO_HOME=/Users/xlyk/.cargo HOME=/private/tmp/ovrcr-401-implementation/home-404 cargo test --offline -p ovrcr --test tui agent_search:: -- --nocapture` — 26 passed, 365 filtered.
- Green: `rtk proxy env CARGO_TARGET_DIR=/private/tmp/ovrcr-401-build-404 CARGO_HOME=/Users/xlyk/.cargo HOME=/private/tmp/ovrcr-401-implementation/home-404 cargo test --offline -p ovrcr --test terminal_acceptance session_jump_retained::retained_jump_inspects_without_restarting_then_running_jump_accepts_only_post_release_input -- --nocapture` — 1 passed, 25 filtered. This drove Kitty modifier events through the compiled Dashboard and isolated binary Server. The retained run stayed at the same run with no PID or new process group; held input did not reach it. A subsequent running selection reached Terminal after release/readiness, and only that fixture Agent acknowledged the explicit marker.
- Green: `rtk cargo fmt --all -- --check` and staged `rtk git diff --check`.

The compiled acceptance needed the authorized fixture-access retry for Unix sockets/process groups. Two failed startup fixture roots were checked for their reported Server PIDs, found inactive, and removed. The successful fixture shut down and cleaned itself. Native GUI/CUA evidence is still root-owned and remains required; this report makes no host-keyboard claim.

The terminal acceptance was run under the assigned `CARGO_TARGET_DIR`, with a private test `HOME` and the existing Cargo cache. The TUI and acceptance fixtures used private server sockets/workspaces/profiles. No other target directory was built or cleaned.
