# Client startup implementation plan

> **For agentic workers:** Use subagent-driven-development or executing-plans to implement these bounded tasks. Root coordinates integration and independent review.

**Goal:** Offer local hook installation/repair, Bridge installation and controlled stale-server replacement before Dashboard attachment.

**Architecture:** Reuse native hook composers and the existing Bridge installer. Keep all startup prompts on the interactive no-command CLI branch. Capture server executable identity at startup and tie any shutdown approval to the checked connection.

**Tech Stack:** Synchronous Rust, existing serde_json/toml_edit/tempfile, Unix sockets, existing shell/Python/Swift Bridge packaging.

**Spec:** `docs/superpowers/specs/2026-10-05-client-startup.md`

## Global constraints

- Preserve one server owner, one active Dashboard and fifty-session support.
- No release tracking, credentials in output, broad process signaling or changes to native trust/approval.
- Default no for every y/n offer; noninteractive execution performs no installation or restart.
- Tests use isolated profiles, config, sockets and owned processes.

### Task 1: Hook composition and guarded repair

Files: `src/cli/agent_setup.rs`, `src/cli/codex_setup.rs`, new `src/cli/startup_hooks.rs`, `tests/agent_setup.rs`; root adds existing dependencies and module declaration.
Interface: `startup_hooks::run(confirm: &mut impl FnMut(&str) -> anyhow::Result<bool>) -> anyhow::Result<()>` inspects active installed Claude/Codex profiles and offers provider-specific writes. Composition helpers accept native text and the current executable path and return validated repaired text; print setup reuses them.

- [x] Add regression coverage for missing hooks, stale owned commands, idempotence, unrelated handler/trust/comment preservation, decline, dangling links and changed files.
- [x] Run `cargo test -p ovrcr --test agent_setup` and retain the original behavioral failure.
- [x] Factor existing composers, use toml_edit for Codex comments, and implement bounded guarded atomic repair. Do not add hooks to inactive historical profiles.
- [x] Run meaningful unit tests plus actual CLI/PTY startup integration cases.
- [ ] Review and commit the coherent hook change after root integration.

### Task 2: Build freshness and controlled restart

Files: `src/client.rs`, `src/service.rs`, `crates/ovrcr-runtime/src/server/startup.rs` and its narrow build-identity helper, `tests/server_lifecycle.rs` or new `tests/client_startup.rs`.
Interface: `ovrcr::client::connect_dashboard(paths: &ServerPaths, confirm: &mut impl FnMut(&str) -> anyhow::Result<bool>) -> anyhow::Result<UnixStream>` returns the checked usable connection. Root calls it before Dashboard attachment.

- [x] Add actual binary/socket regressions: unchanged build attaches, unknown/stale build prompts, decline preserves sessions, acceptance shuts down only the checked connection, same-protocol changed binary is detected, failures do not fall back to signals.
- [x] Run focused tests on the original behavior and save the failure.
- [x] Capture executable bytes at server startup, compare intended server build and validate discovery against connected peer. Prefer a bounded startup-owned identity file over changing the framed wire protocol only for discovery; never treat a found PID as permission to signal it.
- [x] Reuse service ownership checks. Refuse incompatible protocol or fixed outdated service executable with an actionable message rather than an unsafe restart.
- [ ] Run focused unit/integration tests, review and commit after integration.

### Task 3: Asset packaging and startup integration

Files: new `src/cli/startup.rs`, new `src/cli/startup_bridge.rs`, `src/cli/mod.rs`, `justfile`, Bridge packaging/validator scripts, `README.md`, `native/bridge/README.md`, `tests/client_startup.rs` and Bridge packaging checks.
Interfaces: `startup::run(paths: &ServerPaths) -> anyhow::Result<UnixStream>` owns terminal detection and default-no prompts. It calls hook offers, Bridge offers, then checked server connection. Installed payload is adjacent to the CLI under the existing local prefix.

- [x] Add real CLI/PTTY tests asserting prompt text and visible outcomes, and absence of bootstrap on callbacks/help/inspection.
- [x] Add isolated packaging/installer tests proving the payload works after moving away from a checkout and retains signature/contract guards.
- [x] Build/stage assets during `just run`; atomically publish the CLI, preserve executable mode/arguments/CARGO_TARGET_DIR, and remove shell hook mutation and broad stale-server cleanup.
- [x] Invoke the client routine only from the no-command Dashboard path. Optional repair errors produce sanitized actionable warnings and continue.
- [ ] Run focused gates, workspace regression/lint, native CUA on the reviewed checkout, then independent final review, PR/CI and work diary closeout.

2026-10-05 checkpoint: hook suite18/18, CLI units23/23, PTY startup3/3, server startup13/13, service14/14, build units4/4. Relocated payload3/3, packaging15/15, compiled guards22/22. Thread-hosted DesktopAlert fixture now explicitly names its test executable as intended server; its failed startup case passes1/1. Final review, workspace/CI and native acceptance remain. macOS discovery captures startup-path bytes; Linux reads /proc/self/exe. A macOS replacement before the initial capture is not a loaded-image proof.
