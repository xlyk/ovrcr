# Pi / Oh My Pi Reporting, Phase 1 Implementation Plan (tickets #99, #100, #89)

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Workers implement; a different reviewer checks each committed unit. Push, PR and merge require Kyle's authorization.

**Goal:** Land the two prefactors and the first Pi provider slice so that a Pi session launched from the Dashboard picker or `ovrcr agent run pi` reports Busy, Ready (Confirmed), Unread, explicit review and the existing Ready alerts through the managed route, with Codex and Claude behavior unchanged.

**Architecture:** Ticket #99 moves the four Codex-pinned readiness checks onto one predicate in `ovrcr-protocol`, appends an `Omp` provider variant and bumps the protocol version. Ticket #100 adds a provider table to the CLI, teaches the picker to wrap detected `pi`/`omp` executables in `ovrcr agent run <provider> --`, and adds argv eligibility for both harnesses with a reporting-unavailable receiver. Ticket #89 adds the Pi extension (Node built-ins only, compiled into the binary and materialized per invocation), the `report pi --stdin` helper, and the `src/report/pi.rs` receiver that binds on `session_start`, publishes Busy on `agent_start`, and publishes Ready/Confirmed, Error or Idle on `agent_settled`. Later tickets (#101, #90, #91, #94…) are planned after this phase lands.

**Tech Stack:** Rust 2024 workspace (`anyhow`, `serde_json`, `bincode` frames, `libc`), Node 22 ESM for the extension and its tests (`node --test`), existing fixtures in `tests/server_lifecycle.rs`.

**Spec:** GitHub issue #88 (`gh issue view 88 --repo xlyk/ovrcr`), tickets #99, #100, #89. Glossary: `CONTEXT.md`. Guides: `docs/development/{architecture,runtime,tui,testing,delivery}.md`.

## Global Constraints

- One server owner, one active dashboard, fifty sessions. Synchronous runtime; no async runtime, no new service, no new crate, no new dependency.
- Supported readiness providers are exactly Codex, Pi, Oh My Pi. Claude, Grok and Hermes stay excluded from Ready observations, Unread, review and alerts.
- `PROTOCOL_VERSION` becomes 8 in ticket #99 (new `AgentProvider::Omp` variant, appended last). Any later wire change in this phase is forbidden; #90 carries the Input-request snapshot change.
- Codex keeps its accepted contract: exact 0.153.0, `SampleQuality::Observed`, `(session_id, turn_id)` identity. No Codex test assertion is weakened; the one intentional change is in Task 2 (a Confirmed Ready now creates Unread) and must be cited in its commit message.
- Helper input and the private envelope stay within `HOOK_INPUT_LIMIT` = 65,536 bytes; the channel's 800 ms handler deadline and 200 ms auth budget are untouched. One helper in flight, at most 256 queued events, at most 1 MiB queued bytes in the extension.
- The extension never sends prompts, responses, tool arguments, titles or credentials. Payloads carry only: schema, event, mode, owner_pid, instance, sequence, session_id, run, outcome, reason.
- Pi extension loading passes `-e <path>` only. Never pass `--no-extensions` on the interactive route; the scheduled runner (`task_runner.rs`) is untouched.
- Never modify the user's `~/.pi`, `~/.omp`, or any live provider configuration. Tests give every fixture its own `OVRCR_CONFIG`, `OVRCR_SOCKET`, tempdir, and (for installed-Pi opt-in tests) `PI_CODING_AGENT_DIR`.
- Build environment: disk is nearly full and other worktrees share the target dir. Every cargo command is prefixed `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0` and scoped (`-p <crate>` / `--test <name>`); never `cargo test --workspace` inside a ticket. Shell commands are rewritten through `rtk`; that is expected.
- Every ticket ends with `cargo fmt --all -- --check`, `cargo clippy -p <touched crates> --all-targets --all-features -- -D warnings`, `git diff --check`.
- Work happens in `/Users/xlyk/Code/ovrcr/.worktrees/pi-omp-reporting` (branch `feature/pi-omp-reporting`) and per-ticket worktrees `/Users/xlyk/Code/ovrcr/.worktrees/ticket-<n>-<slug>` on branches `ticket/<n>-<slug>` cut from the integration branch. One commit per task; commit messages end with a blank line then `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Commit only; never push.

## Execution order

| Order | Task | Ticket | Depends on |
| --- | --- | --- | --- |
| 1 | Readiness predicate, `Omp` variant, protocol 8, wire snapshot | #99 | — |
| 2 | Runtime Unread creation uses the predicate (Observed or Confirmed) | #99 | 1 |
| 3 | Dashboard alert gate, hints, label color use the predicate | #99 | 1 |
| 4 | Glossary, guides, Pi variant of the unread test, Codex suites | #99 | 2, 3 |
| 5 | CLI provider table, `pi`/`omp` eligibility, unavailable receiver, setup/doctor stubs | #100 | 4 |
| 6 | Picker wraps detected `pi`/`omp` in the managed route | #100 | 5 |
| 7 | Managed-launch integration tests and docs for #100 | #100 | 6 |
| 8 | Pi extension (`.mjs`) and Node event host + `node --test` suite | #89 | 7 |
| 9 | `report pi --stdin`, shared `InvocationLease::bind`, `src/report/pi.rs` receiver | #89 | 8 |
| 10 | Managed lifecycle test: fake Pi host drives the real extension through server/PTY/socket | #89 | 9 |
| 11 | Unread, review and Ready alert through the shipped Dashboard for Pi | #89 | 10 |
| 12 | Docs, support record, CI Node step, opt-in installed-Pi test, final verification | #89 | 11 |

Tickets are sequential (each branch cuts from the integration branch after the previous ticket merges). Tasks inside a ticket are sequential.

## Dispatch protocol (controller)

Same as the deep-modules session (PR #98): for each ticket create the worktree, dispatch one implementer with the ticket's tasks from this plan plus the ticket body, then one spec-and-quality reviewer on the committed diff (`git diff <base>..<head>` saved to a file), then re-review fix rounds; merge the ticket branch into `feature/pi-omp-reporting` with `--no-ff`; after the last ticket run the workspace gates once and a whole-branch review. Models: Sonnet for tasks 1–7 and 12, Opus for 8–11; reviewers match the implementer's model; re-reviews Haiku or Sonnet.

---

## Ticket #99 — one readiness owner, `Omp` wire identity, protocol 8

Worktree `.worktrees/ticket-99-readiness`, branch `ticket/99-readiness`.

### Task 1: `AgentProvider::supports_readiness`, `Omp` variant, protocol version 8

**Files:**
- Modify: `crates/ovrcr-protocol/src/agent.rs:6-13` (enum), `:113-137` (`ReadyObservation`), tests at `:331-384`
- Modify: `crates/ovrcr-protocol/src/codec.rs:18` (`PROTOCOL_VERSION`)
- Modify: `crates/ovrcr-protocol/src/wire.rs` `wire_snapshot` module (`actual()` near `:698`, `EXPECTED` near `:757`)

**Interfaces:**
- Produces: `impl AgentProvider { pub const fn supports_readiness(self) -> bool }` — true for `Codex | Pi | Omp`, false otherwise. `AgentProvider::Omp` appended as the last variant. `PROTOCOL_VERSION = 8`.
- `ReadyObservation::validate` error text becomes `"review requires a Ready observation from a supported readiness provider with a turn and positive activity revision"`.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests` in `agent.rs`)

```rust
    #[test]
    fn readiness_providers_are_codex_pi_and_omp_only() {
        for (provider, expected) in [
            (AgentProvider::Codex, true),
            (AgentProvider::Pi, true),
            (AgentProvider::Omp, true),
            (AgentProvider::Claude, false),
            (AgentProvider::Grok, false),
            (AgentProvider::Hermes, false),
        ] {
            assert_eq!(provider.supports_readiness(), expected, "{provider:?}");
        }
    }

    #[test]
    fn ready_observation_validates_for_every_readiness_provider() {
        let base = ReadyObservation {
            binding: AgentBinding {
                provider: AgentProvider::Codex,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            turn: Some("turn".into()),
            activity_revision: 2,
        };
        for provider in [AgentProvider::Codex, AgentProvider::Pi, AgentProvider::Omp] {
            let ready = ReadyObservation {
                binding: AgentBinding { provider, ..base.binding.clone() },
                ..base.clone()
            };
            ready.validate().unwrap();
        }
        for provider in [AgentProvider::Claude, AgentProvider::Grok, AgentProvider::Hermes] {
            let ready = ReadyObservation {
                binding: AgentBinding { provider, ..base.binding.clone() },
                ..base.clone()
            };
            let error = ready.validate().unwrap_err().to_string();
            assert!(error.contains("supported readiness provider"), "{provider:?}: {error}");
        }
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-protocol readiness_providers` — Expected: compile error, no `supports_readiness`, no `Omp`.

- [ ] **Step 3: Implement**

In `agent.rs` replace the enum and the validator:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentProvider {
    Claude,
    Codex,
    Grok,
    Pi,
    Hermes,
    /// Oh My Pi. Appended last: bincode numbers variants by declaration order.
    Omp,
}

impl AgentProvider {
    /// The providers whose root response cycles may become Ready, Unread, a review
    /// target, and an alert. One owner for what used to be four Codex-pinned checks.
    pub const fn supports_readiness(self) -> bool {
        matches!(self, AgentProvider::Codex | AgentProvider::Pi | AgentProvider::Omp)
    }
}
```

```rust
/// The first accepted Ready observation for a response cycle of a supported readiness
/// provider. Used as an exact acknowledgement target; a later activity revision does
/// not change this identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyObservation {
    pub binding: AgentBinding,
    pub turn: Option<String>,
    pub activity_revision: u64,
}

impl ReadyObservation {
    pub fn validate(&self) -> Result<()> {
        self.binding.validate()?;
        optional_id(&self.turn)?;
        if !self.binding.provider.supports_readiness()
            || self.activity_revision == 0
            || self.turn.is_none()
        {
            bail!(
                "review requires a Ready observation from a supported readiness provider with a turn and positive activity revision"
            );
        }
        Ok(())
    }
}
```

In `codec.rs`: `pub const PROTOCOL_VERSION: u32 = 8;`

In `wire.rs` `actual()`, after the `AgentActivity` block, pin the provider order:

```rust
        all.extend(
            [
                crate::AgentProvider::Claude,
                crate::AgentProvider::Codex,
                crate::AgentProvider::Grok,
                crate::AgentProvider::Pi,
                crate::AgentProvider::Hermes,
                crate::AgentProvider::Omp,
            ]
            .iter()
            .map(|provider| (format!("AgentProvider::{provider:?}"), encode(provider))),
        );
```

- [ ] **Step 4: Regenerate the snapshot**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-protocol wire_snapshot -- --nocapture` — Expected: FAIL printing the new `EXPECTED` block (six new `AgentProvider::*` rows `00`…`05`; every existing row unchanged). Replace `EXPECTED` with the printed block. Re-run: PASS. If any pre-existing row changed, stop: the variant was not appended last.

- [ ] **Step 5: Run the crate tests**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-protocol` — Expected: all pass (28 + 2 new). Then `cargo check --workspace --all-targets --all-features` with the same prefix — Expected: clean (no exhaustive `match` on `AgentProvider` exists outside tests; if one appears, add an `Omp` arm mirroring `Pi`).

- [ ] **Step 6: Commit**

```bash
git add crates/ovrcr-protocol/src/agent.rs crates/ovrcr-protocol/src/codec.rs crates/ovrcr-protocol/src/wire.rs
git commit -m "feat(protocol): one readiness-provider predicate; add Omp; protocol 8"
```

### Task 2: runtime Unread creation consults the predicate

**Files:**
- Modify: `crates/ovrcr-runtime/src/session/reporting.rs:93-110` (`apply`), tests at `:456-501`

**Interfaces:**
- Consumes: `AgentProvider::supports_readiness()` from Task 1.
- Behavior: a `ResponseReady` activity with `quality` `Observed` **or** `Confirmed`, a `Some(turn)`, `Connected` health and a supported provider creates the `ReadyObservation` once per `(binding, turn)`. `Estimated` never does.

- [ ] **Step 1: Update the failing test table** — rename `unread_requires_observed_codex_ready_with_identified_turn` to `unread_requires_supported_provider_ready_with_identified_turn`; change the `Codex, Confirmed` row's expectation from `false` to `true`; add rows:

```rust
            (AgentProvider::Pi, SampleQuality::Confirmed, Some("one"), ReporterHealth::Connected, true),
            (AgentProvider::Pi, SampleQuality::Observed, Some("one"), ReporterHealth::Connected, true),
            (AgentProvider::Omp, SampleQuality::Observed, Some("one"), ReporterHealth::Connected, true),
            (AgentProvider::Grok, SampleQuality::Observed, Some("one"), ReporterHealth::Connected, false),
            (AgentProvider::Hermes, SampleQuality::Confirmed, Some("one"), ReporterHealth::Connected, false),
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-runtime unread_requires_supported` — Expected: FAIL on the Pi and Codex-Confirmed rows.

- [ ] **Step 3: Implement** — replace the condition in `apply`:

```rust
                if report.binding.provider.supports_readiness()
                    && activity.state == AgentActivity::ResponseReady
                    && matches!(activity.quality, SampleQuality::Observed | SampleQuality::Confirmed)
                    && activity.turn.is_some()
                    && snapshot.health.state == ReporterHealth::Connected
                    && self.ready.as_ref().is_none_or(|(ready, _)| {
                        ready.binding != report.binding || ready.turn != activity.turn
                    })
```

- [ ] **Step 4: Run**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-runtime --lib` — Expected: PASS (109 before).

- [ ] **Step 5: Commit** — message must cite the expectation change:

```bash
git add crates/ovrcr-runtime/src/session/reporting.rs
git commit -m "feat(runtime): Unread for every readiness provider, Observed or Confirmed

Behavior change: a Confirmed ResponseReady now creates Unread (Pi reports Confirmed at
its settled boundary). Codex still reports Observed only, so Codex behavior is unchanged."
```

### Task 3: Dashboard alert gate, hints and label color

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/ready.rs:1-4` (doc), `:26-35` (`delivery_live`), test table `:99-159`
- Modify: `crates/ovrcr-tui/src/dashboard/hints.rs:416`, `:423`, `:424`
- Modify: `crates/ovrcr-tui/src/dashboard/render.rs:1286-1301` (`label_color`)

- [ ] **Step 1: Extend the readiness table** (in `ready.rs` tests, after the `Claude` row):

```rust
            (Running, Pi, Connected, ResponseReady, Some("t"), true, true),
            (Running, Omp, Connected, ResponseReady, Some("t"), true, true),
            (Running, Grok, Connected, ResponseReady, Some("t"), true, false),
            (Running, Pi, Unavailable, ResponseReady, Some("t"), true, false),
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-tui --lib readiness_table` — Expected: FAIL on the Pi row (`live` false).

- [ ] **Step 3: Implement**

`ready.rs` header and gate:

```rust
//! Ready (CONTEXT.md): the accepted root observation of a completed response cycle from a
//! supported readiness provider. This is the only place the Dashboard decides whether a
//! session is ready and whether that readiness may be delivered as an alert. The sidebar
//! glyph, the metadata line, and desktop delivery all ask here, so they cannot disagree.
use ovrcr_protocol::{ActivitySample, AgentActivity, ReporterHealth, SessionPhase, SessionSummary};
```

```rust
pub(super) fn delivery_live(session: &SessionSummary) -> bool {
    session.phase == SessionPhase::Running
        && session.agent.as_ref().is_some_and(|agent| {
            agent.binding.provider.supports_readiness()
                && agent.health.state == ReporterHealth::Connected
        })
        && ready(session)
            .and_then(|sample| sample.turn.as_deref())
            .is_some_and(|turn| !turn.is_empty())
}
```

`hints.rs`: the three strings become `"Mark the displayed unread agent response from {target} reviewed; activity and reporting health stay unchanged"`, `"Toggle notifications for new background agent responses in this dashboard; no replay"`, `"Toggle a sound for new background agent responses in this dashboard, independent of desktop notifications; no replay"`.

`render.rs` `label_color`: add before the `grok` arm

```rust
    } else if explicit_label.eq_ignore_ascii_case("omp") {
        SKY
```

(`SKY` is an existing palette constant in `render.rs`; never add a new constant.)

- [ ] **Step 4: Run**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-tui --lib` and `cargo test -p ovrcr --test tui` (same prefix) — Expected: PASS (103 / 177 before). If a tui test pins the old hint wording, update the string and say so in the commit message.

- [ ] **Step 5: Commit**

```bash
git add crates/ovrcr-tui/src/dashboard/ready.rs crates/ovrcr-tui/src/dashboard/hints.rs crates/ovrcr-tui/src/dashboard/render.rs
git commit -m "feat(tui): alert gate and hints speak for every readiness provider; omp label color"
```

### Task 4: glossary, guides, Pi variant of the Dashboard unread test, Codex regressions

**Files:**
- Modify: `CONTEXT.md` (Ready entry), `docs/agent-reporting.md:81-86`, `docs/dashboard.md:53`, `:422-427`, `:465`, `docs/development/architecture.md` (one sentence on `supports_readiness`)
- Modify: `tests/tui/unread.rs`

- [ ] **Step 1: Add the Pi test** — in `tests/tui/unread.rs` make `observation` take the provider and add a test that runs the existing review-key flow for Pi:

```rust
fn observation(turn: &str, revision: u64) -> ReadyObservation {
    observation_for(AgentProvider::Codex, turn, revision)
}

fn observation_for(provider: AgentProvider, turn: &str, revision: u64) -> ReadyObservation {
    ReadyObservation {
        binding: AgentBinding {
            provider,
            invocation: "private-invocation".into(),
            conversation: "private-conversation".into(),
            generation: 1,
        },
        turn: Some(turn.into()),
        activity_revision: revision,
    }
}

#[test]
fn pi_unread_is_reviewable_with_the_same_key_and_identity() {
    let mut dashboard = dashboard_fixture();
    let mut summary = focused_summary(&dashboard);
    summary.unread = Some(observation_for(AgentProvider::Pi, "cycle-1", 3));
    publish_session(&mut dashboard, summary.clone());
    draw(&mut dashboard, 120, 30);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Char('R')) else {
        panic!("R did not mint a request");
    };
    assert_eq!(
        message.request,
        Request::MarkReviewed { session: summary.id, expected: summary.unread.clone().unwrap() }
    );
}
```

(Reuse the `draw`, `focused_summary`, `publish_session` helpers already in the file; `KeyCode`, `Request`, `DashboardAction` are imported via `use super::*` and the file's own `use`.)

- [ ] **Step 2: Run**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr --test tui pi_unread` — Expected: PASS (the gates from Tasks 1–3 make it pass; if it fails, the Dashboard still pins Codex somewhere: find it with `rg 'AgentProvider::Codex' crates/ovrcr-tui/src` and route it through `supports_readiness`).

- [ ] **Step 3: Docs**

`CONTEXT.md` Ready entry:

```markdown
**Ready**:
The accepted root observation of a completed response cycle from a supported readiness provider (Codex, Pi, Oh My Pi), identified by binding, cycle identity (the `turn` field), and activity revision.
_Avoid_: Confirmed activity as a success claim, Claude observations, a desktop alert
```

`docs/agent-reporting.md` lines 81–86: replace "Managed Codex also retains one unread identity" with "Managed sessions of a supported readiness provider (Codex, Pi, Oh My Pi) retain one unread identity", and the last sentence with "Manual activity reports and other providers do not create unread observations."

`docs/dashboard.md`: line 53 "A managed Codex terminal" → "A managed Codex, Pi or Oh My Pi terminal"; line 422 "a managed Codex root response" → "a managed root response from a supported readiness provider"; line 465 "one accepted managed Codex root response" → "one accepted managed root response".

`docs/development/architecture.md`: append to the shared-types paragraph: "Which providers may produce Ready, Unread, review targets and alerts is decided once by `AgentProvider::supports_readiness`; never compare a provider literal at a consumer."

- [ ] **Step 4: Codex regressions**

Run with the prefix: `cargo test -p ovrcr --test codex_reporting`, `cargo test -p ovrcr --test server_lifecycle codex_`, `cargo test -p ovrcr --test server_lifecycle desktop_notifications`, `cargo test -p ovrcr --test server_lifecycle ready_sound`, `cargo test -p ovrcr --test cli codex`, `cargo test -p ovrcr --test agent_setup codex` — Expected: every filter executes ≥1 test and passes. Then `cargo fmt --all -- --check`, `cargo clippy -p ovrcr-protocol -p ovrcr-runtime -p ovrcr-tui -p ovrcr --all-targets --all-features -- -D warnings`, `git diff --check`.

- [ ] **Step 5: Commit**

```bash
git add CONTEXT.md docs/agent-reporting.md docs/dashboard.md docs/development/architecture.md tests/tui/unread.rs
git commit -m "docs(readiness): Ready is provider-neutral; Pi unread review test"
```

## Ticket #100 — managed launch route for `pi` and `omp`

Worktree `.worktrees/ticket-100-launch`, branch `ticket/100-launch`, cut after #99 merges.

### Task 5: CLI provider table, eligibility grammars, unavailable receiver, setup/doctor stubs

**Files:**
- Modify: `src/cli/args.rs:294`, `:303`, `:316`, `:318` (value parsers), `:313` (`long_about`)
- Modify: `src/cli/agent.rs` (whole file)
- Modify: `src/report.rs` (add `unavailable_receiver`; `pub mod pi; pub mod omp;`)
- Create: `src/report/pi.rs` (eligibility only in this task), `src/report/omp.rs`
- Create: `tests/pi_reporting.rs`
- Modify: `tests/cli.rs:2196-2209`

**Interfaces:**
- Produces in `src/cli/args.rs`: `pub(super) const PROVIDERS: [&str; 4] = ["claude", "codex", "pi", "omp"];`
- Produces in `src/cli/agent.rs`: `fn provider(name: &str) -> ovrcr::protocol::AgentProvider`
- Produces in `src/report.rs`: `pub fn unavailable_receiver(lease: Option<InvocationLease>, provider: &str, reason: &str) -> HookHandler`
- Produces in `src/report/pi.rs`: `pub fn eligible_argv(argv: &[OsString]) -> bool`; in `src/report/omp.rs`: `pub fn eligible_argv(argv: &[OsString]) -> bool`

- [ ] **Step 1: Write the failing tests** — `tests/pi_reporting.rs`:

```rust
use std::{
    ffi::OsString,
    process::{Command, Stdio},
};

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn pi_interactive_grammar_rejects_headless_help_version_and_package_modes() {
    for values in [
        vec!["pi"],
        vec!["pi", "--model", "anthropic/claude-sonnet-4", "hello"],
        vec!["pi", "-e", "/tmp/user-extension.ts", "--resume"],
        vec!["pi", "--session", "abc123", "--thinking", "high"],
        vec!["/usr/local/bin/pi", "--", "install this"],
    ] {
        assert!(ovrcr::report::pi::eligible_argv(&args(&values)), "{values:?}");
    }
    for values in [
        vec!["pi", "--mode", "rpc"],
        vec!["pi", "--mode", "json"],
        vec!["pi", "-p", "hello"],
        vec!["pi", "--print", "hello"],
        vec!["pi", "--help"],
        vec!["pi", "-h"],
        vec!["pi", "--version"],
        vec!["pi", "-v"],
        vec!["pi", "--export", "out.html"],
        vec!["pi", "--list-models"],
        vec!["pi", "install", "some-package"],
        vec!["pi", "auth", "print-api-key"],
        vec!["other"],
    ] {
        assert!(!ovrcr::report::pi::eligible_argv(&args(&values)), "{values:?}");
    }
}

#[test]
fn omp_interactive_grammar_rejects_headless_help_and_version_modes() {
    for values in [vec!["omp"], vec!["omp", "--model", "x", "hello"], vec!["omp", "--yolo"]] {
        assert!(ovrcr::report::omp::eligible_argv(&args(&values)), "{values:?}");
    }
    for values in [
        vec!["omp", "--mode", "rpc"],
        vec!["omp", "--mode", "acp"],
        vec!["omp", "--mode", "rpc-ui"],
        vec!["omp", "-p"],
        vec!["omp", "--print"],
        vec!["omp", "--help"],
        vec!["omp", "--version"],
        vec!["pi"],
    ] {
        assert!(!ovrcr::report::omp::eligible_argv(&args(&values)), "{values:?}");
    }
}

#[test]
fn pi_and_omp_native_argv_stdout_and_exit_survive_missing_reporting() {
    for provider in ["pi", "omp"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(["agent", "run", provider, "--", "/bin/sh", "-c", "printf '%s|%s' \"$0\" \"$1\"; exit 23", "native-zero", "kept arg"])
            .env_remove("OVRCR_HOOK_SOCKET")
            .env_remove("OVRCR_SESSION_ID")
            .env_remove("OVRCR_HOOK_TOKEN")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(23), "{provider}");
        assert_eq!(output.stdout, b"native-zero|kept arg", "{provider}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("reporting unavailable"), "{provider}: {stderr}");
        assert!(!stderr.contains("OVRCR_AGENT_TOKEN"), "{provider}: {stderr}");
    }
}

#[test]
fn pi_setup_prints_managed_launch_contract_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
        .args(["agent", "setup", "pi", "--print"])
        .env("OVRCR_CONFIG", dir.path().join("registry.toml"))
        .env("OVRCR_SOCKET", dir.path().join("socket"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("agent run pi -- pi"), "{stdout}");
    assert!(stdout.contains("no settings"), "{stdout}");
    assert!(!dir.path().join("socket").exists());
    assert!(!dir.path().join("registry.toml").exists());
}
```

And in `tests/cli.rs` change the help assertion to `assert!(help.contains("claude, codex, pi, omp"), "{help}");`.

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr --test pi_reporting` — Expected: compile error (`ovrcr::report::pi` missing).

- [ ] **Step 3: Implement**

`src/cli/args.rs`: add near the top `pub(super) const PROVIDERS: [&str; 4] = ["claude", "codex", "pi", "omp"];` and replace each `value_parser = ["claude", "codex"]` with `value_parser = PROVIDERS`. Append to the `Run` `long_about`: ` Pi and Oh My Pi: interactive terminal launches are supervised and load the owned OVRCR reporting extension beside the user's own extensions; help, version, print, RPC, JSON, ACP, export and package commands run native with reporting unavailable.`

`src/report/pi.rs` (this task's content; Task 9 adds the receiver below it):

```rust
//! Managed interactive Pi: argv eligibility (this module grows the receiver in #89).
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// Interactive terminal launches only. Provider arguments are never rewritten here.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("pi")) {
        return false;
    }
    let Some(rest) = argv.get(1..) else { return false };
    if let Some(first) = rest.first().and_then(|s| s.to_str())
        && !first.starts_with('-')
        && matches!(
            first,
            "auth" | "install" | "uninstall" | "remove" | "list" | "update" | "upgrade" | "config" | "help"
        )
    {
        return false;
    }
    rest.iter().all(|arg| {
        let Some(arg) = arg.to_str() else { return false };
        !matches!(
            arg,
            "--mode" | "-p" | "--print" | "-h" | "--help" | "-v" | "--version" | "--export" | "--list-models"
        )
    })
}
```

`src/report/omp.rs`:

```rust
//! Managed interactive Oh My Pi: argv eligibility (the receiver arrives with #94).
use std::ffi::{OsStr, OsString};
use std::path::Path;

pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("omp")) {
        return false;
    }
    argv[1..].iter().all(|arg| {
        let Some(arg) = arg.to_str() else { return false };
        !matches!(arg, "--mode" | "-p" | "--print" | "-h" | "--help" | "-v" | "-V" | "--version")
    })
}
```

`src/report.rs`: add `pub mod omp; pub mod pi;` to the module list and

```rust
/// A provider whose reporting cannot run for this invocation: release the reservation
/// immediately, tell the operator once on stderr, and answer every callback unavailable.
pub fn unavailable_receiver(
    lease: Option<InvocationLease>,
    provider: &str,
    reason: &str,
) -> ovrcr_runtime::agent_runner::HookHandler {
    if let Some(lease) = lease {
        let _ = lease.stream.shutdown(std::net::Shutdown::Both);
        drop(lease);
    }
    eprintln!("{provider} reporting unavailable ({reason}); running native command");
    Box::new(|_| b"admission-unavailable\n".to_vec())
}
```

`src/cli/agent.rs` full replacement:

```rust
use super::{AppResult, RuntimeError, args::AgentCommand};
use ovrcr::protocol::AgentProvider;
use std::os::unix::process::ExitStatusExt;

fn provider(name: &str) -> AgentProvider {
    match name {
        "codex" => AgentProvider::Codex,
        "pi" => AgentProvider::Pi,
        "omp" => AgentProvider::Omp,
        _ => AgentProvider::Claude,
    }
}

fn display_name(name: &str) -> &'static str {
    match name {
        "codex" => "Codex",
        "pi" => "Pi",
        "omp" => "Oh My Pi",
        _ => "Claude",
    }
}

/// Pi and Oh My Pi need no settings: the managed launch loads the owned extension itself.
fn managed_setup(name: &str) -> AppResult<()> {
    println!(
        "{} reporting needs no settings changes. Launch through `ovrcr agent run {name} -- {name} [ARGS...]` inside an OVRCR terminal, or pick {name} in the Dashboard agent picker. A plain `{name}` launch stays untracked.",
        display_name(name)
    );
    Ok(())
}

fn managed_doctor(name: &str, executable: &std::ffi::OsStr) -> AppResult<()> {
    let version = ovrcr::report::admission::probe_version(executable)
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned());
    let report = serde_json::json!({
        "provider": name,
        "probe_status": if version.is_some() { "probed" } else { "unavailable" },
        "version": version,
        "supported_versions": "any compatible release; tested versions are evidence, not an allowlist",
        "capabilities": { "managed_launch": true, "reporting": if name == "pi" { "pending #89" } else { "pending #94" } },
    });
    println!("{}", serde_json::to_string_pretty(&report).expect("a JSON value serializes"));
    Ok(())
}

pub(super) fn run(command: AgentCommand) -> AppResult<()> {
    let (name, argv) = match command {
        AgentCommand::Setup { provider, print: _, settings } => {
            return match provider.as_str() {
                "codex" => super::codex_setup::setup(settings.as_deref()),
                "pi" | "omp" => managed_setup(&provider),
                _ => super::agent_setup::setup(settings.as_deref()),
            };
        }
        AgentCommand::Doctor { provider, settings, session, executable } => {
            let executable = executable.unwrap_or_else(|| provider.clone().into());
            return match provider.as_str() {
                "codex" => super::codex_setup::doctor(settings.as_deref(), session, &executable),
                "pi" | "omp" => managed_doctor(&provider, &executable),
                _ => super::agent_setup::doctor(settings.as_deref(), session, &executable),
            };
        }
        AgentCommand::Run { provider, legacy_provider, argv } => {
            (provider.or(legacy_provider).expect("required provider"), argv)
        }
    };
    let swallow_conflict = name != "claude";
    let mut lease = ovrcr::report::reserve_invocation_for(provider(&name))
        .or_else(|error| if swallow_conflict { Ok(None) } else { Err(error) })
        .map_err(|error| {
            if let Some(report) = error.downcast_ref::<ovrcr::report::ReportError>() {
                RuntimeError::new(report.code(), report.to_string())
            } else {
                RuntimeError::internal(error)
            }
        })?;
    let status = ovrcr::agent_runner::run_native(&argv, move |available, native_argv| {
        if !available {
            drop(lease.take());
            eprintln!("agent reporting unavailable; running native command");
            return None;
        }
        Some(match name.as_str() {
            "codex" => ovrcr::report::codex::receiver(lease, native_argv),
            "pi" => ovrcr::report::unavailable_receiver(lease, "Pi", "provider reporting not implemented yet"),
            "omp" => ovrcr::report::unavailable_receiver(lease, "Oh My Pi", "provider reporting not implemented yet"),
            _ => ovrcr::report::admission::receiver(lease, native_argv),
        })
    })
    .map_err(RuntimeError::internal)?;
    if let Some(signal) = status.signal() {
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
        std::process::exit(128 + signal);
    }
    std::process::exit(status.code().unwrap_or(1));
}

```

- [ ] **Step 4: Run**

Run with the prefix: `cargo test -p ovrcr --test pi_reporting`, `cargo test -p ovrcr --test cli codex_setup_and_doctor_help`, `cargo test -p ovrcr --test codex_reporting`, `cargo test -p ovrcr --test agent_setup` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli/args.rs src/cli/agent.rs src/report.rs src/report/pi.rs src/report/omp.rs tests/pi_reporting.rs tests/cli.rs
git commit -m "feat(cli): pi and omp providers: eligibility, unavailable receiver, setup/doctor stubs"
```

### Task 6: the picker wraps detected `pi` and `omp` in the managed route

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/agents.rs:6-16` (`KNOWN_AGENTS`), `:33-58` (`detect_agents`), tests at the bottom
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs:1104-1111` (`detected_agents`)
- Modify: `tests/tui/palette.rs:1292-1293` (the `detect_agents` call gains the launcher argument)

**Interfaces:**
- Produces: `pub fn detect_agents(path: &OsStr, shell: Option<&OsStr>, launcher: Option<&Path>) -> Vec<AgentEntry>`; `pub const MANAGED_AGENTS: [&str; 2] = ["pi", "omp"];`
- An entry for a managed agent has `argv = [launcher, "agent", "run", name, "--", <detected executable>]` when `launcher` is `Some`; otherwise the bare executable (unchanged behavior). Overrides applied afterwards by `apply_overrides` replace `argv` wholesale, so a settings override still wins.

- [ ] **Step 1: Write the failing test** (in `agents.rs` tests):

```rust
    #[test]
    fn managed_agents_launch_through_the_agent_run_route() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["pi", "omp", "codex"] {
            write_stub(dir.path(), name, 0o755);
        }
        let launcher = Path::new("/opt/ovrcr/bin/ovrcr");
        let entries = detect_agents(dir.path().as_os_str(), None, Some(launcher));
        let argv = |name: &str| entries.iter().find(|e| e.name == name).unwrap().argv.clone();
        for name in MANAGED_AGENTS {
            assert_eq!(
                argv(name),
                vec![
                    OsString::from(launcher),
                    "agent".into(),
                    "run".into(),
                    name.into(),
                    "--".into(),
                    dir.path().join(name).into_os_string(),
                ],
                "{name}"
            );
        }
        assert_eq!(argv("codex"), vec![dir.path().join("codex").into_os_string()]);
        let bare = detect_agents(dir.path().as_os_str(), None, None);
        assert_eq!(bare.iter().find(|e| e.name == "pi").unwrap().argv, vec![dir.path().join("pi").into_os_string()]);
        let overridden = apply_overrides(entries, &[AgentOverride { name: "pi".into(), argv: vec!["/custom/pi".into()] }]);
        assert_eq!(overridden.iter().find(|e| e.name == "pi").unwrap().argv, vec![OsString::from("/custom/pi")]);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr-tui --lib managed_agents_launch` — Expected: compile error (arity, `MANAGED_AGENTS`).

- [ ] **Step 3: Implement**

```rust
const KNOWN_AGENTS: [&str; 10] = [
    "claude", "codex", "gemini", "aider", "opencode", "pi", "omp", "goose", "amp", "cursor-agent",
];
/// Detected agents OVRCR launches through `agent run <name> --` so the owned reporting
/// extension loads beside the user's own. Codex and Claude keep their explicit routes.
pub const MANAGED_AGENTS: [&str; 2] = ["pi", "omp"];

pub fn detect_agents(path: &OsStr, shell: Option<&OsStr>, launcher: Option<&Path>) -> Vec<AgentEntry> {
    let mut entries = Vec::new();
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    for name in KNOWN_AGENTS {
        if let Some(found) = dirs.iter().find_map(|dir| executable_in(dir, name)) {
            let argv = match launcher {
                Some(launcher) if MANAGED_AGENTS.contains(&name) => vec![
                    launcher.as_os_str().to_owned(),
                    "agent".into(),
                    "run".into(),
                    name.into(),
                    "--".into(),
                    found.into(),
                ],
                _ => vec![found.into()],
            };
            entries.push(AgentEntry { name: name.into(), argv, source: AgentSource::Detected });
        }
    }
    // … shell and Custom entries unchanged …
```

`palette.rs` `detected_agents`: `let launcher = std::env::current_exe().ok(); apply_overrides(detect_agents(&path, shell.as_deref(), launcher.as_deref()), &self.settings.agents)`.

`tests/tui/palette.rs:1292`: `ovrcr::tui::detect_agents(&path, shell.as_deref(), None)`.

Fix the existing `detects_executables_then_shell_then_custom` test's call (add `None`).

- [ ] **Step 4: Run**

Run with the prefix: `cargo test -p ovrcr-tui --lib agents`, `cargo test -p ovrcr --test tui` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ovrcr-tui/src/dashboard/agents.rs crates/ovrcr-tui/src/dashboard/palette.rs tests/tui/palette.rs
git commit -m "feat(tui): picker launches detected pi and omp through agent run"
```

### Task 7: managed-launch integration tests and docs for #100

**Files:**
- Modify: `tests/server_lifecycle.rs` (append near the Codex fixtures, after `codex_session_named`)
- Modify: `docs/cli-reference.md:103-108` (rows), `docs/dashboard.md:350-352` (picker list)

- [ ] **Step 1: Write the test** — the argv is exactly what `detect_agents` produces for a managed agent, run inside a real PTY session:

```rust
#[test]
fn pi_managed_launch_preserves_argv_exit_and_environment_and_plain_launch_is_untracked() {
    let _guard = env_lock();
    use std::os::unix::fs::PermissionsExt;
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "pi-setup");
    let native = fixture._root.path().join("pi");
    std::fs::write(
        &native,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '0.85.1\\n'; exit; fi\nprintf 'PI_ARGS=%s\\n' \"$*\"\nprintf 'PI_HOOK=[%s%s%s]\\n' \"${OVRCR_HOOK_SOCKET:-}\" \"${OVRCR_HOOK_TOKEN:-}\" \"${OVRCR_SESSION_ID:-}\"\nprintf 'PI_CHANNEL=[%s]\\n' \"${OVRCR_AGENT_SOCKET:+set}\"\nexit 17\n",
    )
    .unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    // Managed: the picker's argv shape.
    let managed = fixture.create_session_summary(
        "pi-managed",
        vec![
            env!("CARGO_BIN_EXE_ovrcr").into(), "agent".into(), "run".into(), "pi".into(), "--".into(),
            native.clone().into_os_string(), "--model".into(), "x/y".into(), "hello world".into(),
        ],
    );
    fixture.record_process_group(&managed);
    fixture.wait_terminal_contains(managed.id, "PI_ARGS=--model x/y hello world");
    fixture.wait_terminal_contains(managed.id, "PI_HOOK=[]");
    fixture.wait_terminal_contains(managed.id, "PI_CHANNEL=[set]");
    // Reporting is not implemented yet in this ticket: the supervisor says so once and runs native.
    fixture.wait_terminal_contains(managed.id, "reporting unavailable");
    assert!(fixture.session_summary(managed.id).agent.is_none());
    // Plain launch: no supervision, no channel, no binding.
    let plain = fixture.create_session_summary("pi-plain", vec![native.into_os_string(), "hi".into()]);
    fixture.record_process_group(&plain);
    fixture.wait_terminal_contains(plain.id, "PI_ARGS=hi");
    fixture.wait_terminal_contains(plain.id, "PI_CHANNEL=[]");
    assert!(fixture.session_summary(plain.id).agent.is_none());
}
```

Note for the implementer: in this ticket the `pi` receiver is the unavailable stub, so no `-e` is inserted yet. Task 9 changes this assertion to `PI_ARGS=-e ` followed by the materialized path and must cite that in its commit message. Ineligible modes: add a second session `pi-rpc` with `"--mode", "rpc"` and assert `reporting unavailable` plus `PI_ARGS=--mode rpc`.

- [ ] **Step 2: Run**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle pi_managed_launch` — Expected: PASS; the filter executes 1 test.

- [ ] **Step 3: Docs**

`docs/cli-reference.md`: add rows
`| agent run pi -- pi [ARGS...] | Supervise an interactive Pi launch inside an OVRCR PTY; the owned reporting extension is loaded beside the user's own (#89). Help, version, print, RPC, JSON, export and package commands run native with reporting unavailable. |`
`| agent run omp -- omp [ARGS...] | Supervise an interactive Oh My Pi launch (reporting arrives with #94); headless modes run native. |`
`| agent setup pi|omp --print | Print the managed-launch contract; nothing to write. |`
`| agent doctor pi|omp --json [--executable PATH] | Probe --version and report capabilities; no allowlist. |`

`docs/dashboard.md` picker paragraph: list `omp` after `pi`, then add: "Detected `pi` and `omp` entries launch through `ovrcr agent run <name> --` so OVRCR can report their activity; an `agents` override for the same name replaces the command entirely. A shell command that runs `pi` or `omp` directly stays untracked."

- [ ] **Step 4: Gates and commit**

Run with the prefix: `cargo fmt --all -- --check`, `cargo clippy -p ovrcr -p ovrcr-tui --all-targets --all-features -- -D warnings`, `git diff --check`.

```bash
git add tests/server_lifecycle.rs docs/cli-reference.md docs/dashboard.md
git commit -m "test(lifecycle): managed pi launch preserves argv and exit; plain launch untracked"
```

## Ticket #89 — Pi responses through the managed route

Worktree `.worktrees/ticket-89-pi-responses`, branch `ticket/89-pi-responses`, cut after #100 merges.

### Task 8: the Pi extension and its Node event host

**Files:**
- Create: `src/pi-reporting-extension.mjs`
- Create: `tests/fixtures/pi/pi_host.mjs`
- Create: `tests/pi_reporting_extension.mjs`

**Interfaces:**
- The extension is an ESM default-export factory `(pi) => void`. It registers handlers for `session_start`, `agent_start`, `agent_end`, `agent_settled`, `session_shutdown` **only when** `OVRCR_AGENT_SOCKET` and `OVRCR_AGENT_TOKEN` are set; otherwise the factory returns before registering anything. Every handler returns a promise that resolves when its helper has exited or been killed at the 900 ms deadline, so the host (and Pi) can await it.
- Helper: `spawn(OVRCR_BINARY, ["report", "pi", "--stdin"], { stdio: ["pipe", "ignore", "ignore"] })` from the extension's own process (a direct child of the native process; no shell). `OVRCR_BINARY` is the literal token `__OVRCR_BINARY__` replaced at materialization (Task 9) with the JSON-encoded absolute path of the supervising `ovrcr`.
- Payload written to the helper's stdin (one JSON object, no content fields):
  `{ "schema": 1, "event": <string>, "mode": <ctx.mode>, "owner_pid": <number>, "instance": <32 hex>, "sequence": <n≥1>, "session_id": <string>, "run": <n|null>, "outcome": "none"|"ok"|"error"|"aborted", "reason"?: <string> }`
- Host: `createHost(extensionPath, { mode = "tui", session = "session-a" }) -> { state, registered(), emit(event) }`; `assistant(stopReason)`.

- [ ] **Step 1: Write the failing Node test** — `tests/pi_reporting_extension.mjs`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, chmodSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHost, assistant } from "./fixtures/pi/pi_host.mjs";

const SOURCE = resolve("src/pi-reporting-extension.mjs");

function materialize({ hang = false } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-pi-ext-"));
  const record = join(dir, "record.jsonl");
  const helper = join(dir, "fake-ovrcr");
  writeFileSync(
    helper,
    `#!/bin/sh\n[ "$1" = report ] && [ "$2" = pi ] && [ "$3" = --stdin ] || exit 9\ncat >> "${record}"; printf '\\n' >> "${record}"\n${hang ? "sleep 5\n" : ""}`,
  );
  chmodSync(helper, 0o700);
  const extension = join(dir, "ovrcr-pi-reporting.mjs");
  writeFileSync(extension, readFileSync(SOURCE, "utf8").replace("__OVRCR_BINARY__", JSON.stringify(helper)));
  return { extension, record };
}

function frames(record) {
  if (!existsSync(record)) return [];
  return readFileSync(record, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

function managed() {
  process.env.OVRCR_AGENT_SOCKET = "/private/ovrcr/socket";
  process.env.OVRCR_AGENT_TOKEN = "f".repeat(64);
}

test("inert outside a managed invocation: registers nothing", async () => {
  delete process.env.OVRCR_AGENT_SOCKET;
  delete process.env.OVRCR_AGENT_TOKEN;
  const { extension } = materialize();
  const host = await createHost(extension);
  assert.deepEqual(host.registered(), []);
});

test("one response cycle: start, end, settled carry run, outcome and ordered sequence", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  assert.deepEqual(host.registered(), ["agent_end", "agent_settled", "agent_start", "session_shutdown", "session_start"]);
  await host.emit({ type: "session_start", reason: "startup" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [{ role: "user", content: "secret prompt" }, assistant("stop")] });
  await host.emit({ type: "agent_settled" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.sequence, f.run, f.outcome]), [
    ["session_start", 1, null, "none"],
    ["agent_start", 2, 1, "none"],
    ["agent_end", 3, 1, "ok"],
    ["agent_settled", 4, 1, "ok"],
  ]);
  for (const frame of sent) {
    assert.equal(frame.schema, 1);
    assert.equal(frame.mode, "tui");
    assert.equal(frame.owner_pid, process.pid);
    assert.equal(frame.session_id, "sess-a");
    assert.match(frame.instance, /^[0-9a-f]{32}$/);
    assert.ok(!JSON.stringify(frame).includes("secret prompt"));
  }
  assert.equal(sent[0].reason, "startup");
});

test("error and aborted outcomes come from the last assistant message; a second run increments", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop"), assistant("error")] });
  await host.emit({ type: "agent_settled" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("aborted")] });
  await host.emit({ type: "agent_settled" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [] });
  await host.emit({ type: "agent_settled" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "error"], ["agent_settled", 1, "error"],
    ["agent_start", 2, "none"], ["agent_end", 2, "aborted"], ["agent_settled", 2, "aborted"],
    ["agent_start", 3, "none"], ["agent_end", 3, "none"], ["agent_settled", 3, "none"],
  ]);
});

test("non-tui modes never spawn a helper; shutdown reports once then disables", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { mode: "rpc" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_settled" });
  assert.deepEqual(frames(record), []);
  host.state.mode = "tui";
  await host.emit({ type: "session_shutdown", reason: "reload" });
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.reason]), [["session_shutdown", "reload"]]);
});

test("a hung helper is killed at the deadline and the handler still resolves", async () => {
  managed();
  const { extension } = materialize({ hang: true });
  const host = await createHost(extension);
  const started = Date.now();
  await host.emit({ type: "agent_start" });
  const elapsed = Date.now() - started;
  assert.ok(elapsed >= 800 && elapsed < 2500, `elapsed ${elapsed}`);
});

test("queue overflow disables reporting and sends one content-free unavailable notice", async () => {
  managed();
  const { extension, record } = materialize({ hang: true });
  const host = await createHost(extension);
  const pending = [];
  for (let i = 0; i < 258; i += 1) pending.push(host.emit({ type: "agent_start" }));
  await Promise.all(pending);
  const sent = frames(record);
  assert.equal(sent.at(-1).event, "unavailable");
  assert.equal(sent.at(-1).reason, "overflow");
  assert.ok(sent.length <= 2, `helpers actually completed: ${sent.length}`);
  await host.emit({ type: "agent_settled" });
  assert.equal(frames(record).length, sent.length, "disabled after overflow");
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `node --test tests/pi_reporting_extension.mjs` — Expected: FAIL (cannot import `./fixtures/pi/pi_host.mjs`).

- [ ] **Step 3: Write the host** — `tests/fixtures/pi/pi_host.mjs`:

```js
// Test host for the OVRCR Pi reporting extension. As a library it fires the real
// registered handlers with a fake ExtensionContext (node --test). Run directly it is a
// fake `pi` executable for the Rust lifecycle tests: it loads the `-e` extension and
// turns stdin lines into extension events, printing PI_CALLBACK=<n> after each one.
import { pathToFileURL } from "node:url";
import { createInterface } from "node:readline";
import { writeFileSync } from "node:fs";

export function assistant(stopReason) {
  return { role: "assistant", stopReason, content: [] };
}

export async function createHost(extensionPath, { mode = "tui", session = "session-a" } = {}) {
  const handlers = new Map();
  const api = {
    on(event, handler) {
      if (!handlers.has(event)) handlers.set(event, []);
      handlers.get(event).push(handler);
    },
  };
  const module = await import(pathToFileURL(extensionPath).href);
  await module.default(api);
  const state = { mode, session };
  const ctx = {
    get mode() { return state.mode; },
    hasUI: true,
    sessionManager: { getSessionId: () => state.session, getSessionFile: () => undefined },
  };
  return {
    state,
    registered: () => [...handlers.keys()].sort(),
    async emit(event) {
      const results = [];
      for (const handler of handlers.get(event.type) ?? []) results.push(await handler(event, ctx));
      return results;
    },
  };
}

// stdin grammar: session_start[:<id>[:<reason>]] | agent_start | agent_end:<ok|error|aborted|none>
//   | agent_settled | session_shutdown:<reason> | mode:<tui|rpc|print> | exit
async function main() {
  const args = process.argv.slice(2);
  const at = args.indexOf("-e");
  const host = at >= 0 ? await createHost(args[at + 1]) : null;
  if (process.env.OVRCR_TEST_PROBE) {
    writeFileSync(
      process.env.OVRCR_TEST_PROBE,
      `${process.env.OVRCR_AGENT_SOCKET ?? ""}\n${process.env.OVRCR_AGENT_TOKEN ?? ""}\n${process.pid}\n`,
    );
  }
  process.stdout.write("PI_NATIVE_READY\n");
  let index = 0;
  for await (const line of createInterface({ input: process.stdin })) {
    const [command, a, b] = line.split(":");
    if (command === "exit") process.exit(17);
    if (host) {
      if (command === "mode") host.state.mode = a;
      else if (command === "session_start") {
        if (a) host.state.session = a;
        await host.emit({ type: "session_start", reason: b ?? "startup" });
      } else if (command === "agent_start") await host.emit({ type: "agent_start" });
      else if (command === "agent_end") {
        const messages = a === "none" ? [] : [assistant(a === "ok" ? "stop" : a)];
        await host.emit({ type: "agent_end", messages });
      } else if (command === "agent_settled") await host.emit({ type: "agent_settled" });
      else if (command === "session_shutdown") await host.emit({ type: "session_shutdown", reason: a ?? "quit" });
    }
    process.stdout.write(`PI_CALLBACK=${index}\n`);
    index += 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`${error}\n`);
    process.exit(1);
  });
}
```

- [ ] **Step 4: Write the extension** — `src/pi-reporting-extension.mjs`:

```js
// OVRCR reporting for a managed interactive Pi session. Loaded only by
// `ovrcr agent run pi` through an explicit -e path beside the user's own extensions;
// inert without the private invocation channel. Sends bounded identifiers and
// discriminants to a directly spawned helper, never prompts, responses or tool data.
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";

// Replaced with the JSON-encoded absolute path of the supervising ovrcr binary when the
// receiver materializes this file for one invocation.
const OVRCR_BINARY = __OVRCR_BINARY__;
const HELPER_ARGS = ["report", "pi", "--stdin"];
const HELPER_DEADLINE_MS = 900; // inside the channel's one-second callback budget
const MAX_QUEUED_EVENTS = 256;
const MAX_QUEUED_BYTES = 1024 * 1024;

export default function (pi) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const producer = {
    instance: randomBytes(16).toString("hex"),
    sequence: 0,
    run: 0,
    // One cycle spans every continuation Pi runs (retry, automatic compaction, a queued
    // follow-up): Pi emits agent_start at the top of both its agent loop and its continue
    // loop, and settles once. The extension owns that span.
    open: false,
    outcome: "none",
    disabled: false,
  };
  const queue = [];
  let queuedBytes = 0;
  let draining = Promise.resolve(false);

  // Identity and ordering are captured here, synchronously, before any delivery.
  function frame(event, ctx, extra) {
    producer.sequence += 1;
    return JSON.stringify({
      schema: 1,
      event,
      mode: ctx.mode,
      owner_pid: process.pid,
      instance: producer.instance,
      sequence: producer.sequence,
      session_id: ctx.sessionManager.getSessionId(),
      run: producer.run > 0 ? producer.run : null,
      outcome: producer.outcome,
      ...extra,
    });
  }

  function deliver(body) {
    return new Promise((resolve) => {
      let child;
      try {
        child = spawn(OVRCR_BINARY, HELPER_ARGS, { stdio: ["pipe", "ignore", "ignore"] });
      } catch {
        resolve(false);
        return;
      }
      const timer = setTimeout(() => child.kill("SIGKILL"), HELPER_DEADLINE_MS);
      child.once("error", () => {
        clearTimeout(timer);
        resolve(false);
      });
      child.once("close", (code) => {
        clearTimeout(timer);
        resolve(code === 0);
      });
      child.stdin.once("error", () => {});
      child.stdin.end(body);
    });
  }

  // One helper in flight; FIFO; bounded. Overflow disables this producer after one
  // content-free notice (recovery is a later ticket).
  function report(event, ctx, extra = {}) {
    if (producer.disabled || ctx.mode !== "tui") return Promise.resolve(false);
    const body = frame(event, ctx, extra);
    if (queue.length >= MAX_QUEUED_EVENTS || queuedBytes + body.length > MAX_QUEUED_BYTES) {
      producer.disabled = true;
      queue.length = 0;
      queuedBytes = 0;
      const notice = frame("unavailable", ctx, { reason: "overflow" });
      draining = draining.then(() => deliver(notice));
      return draining;
    }
    queue.push(body);
    queuedBytes += body.length;
    const mine = draining.then(() => {
      const next = queue.shift();
      if (next === undefined) return false;
      queuedBytes -= next.length;
      return deliver(next);
    });
    draining = mine.catch(() => false);
    return mine;
  }

  function outcomeOf(messages) {
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      const message = messages[i];
      if (message && message.role === "assistant") {
        if (message.stopReason === "error") return "error";
        if (message.stopReason === "aborted") return "aborted";
        return "ok";
      }
    }
    return "none";
  }

  pi.on("session_start", (event, ctx) => {
    producer.run = 0;
    producer.open = false;
    producer.outcome = "none";
    return report("session_start", ctx, { reason: event.reason });
  });
  pi.on("agent_start", (_event, ctx) => {
    // A second start inside an open cycle is a continuation: same run, outcome so far kept.
    if (!producer.open) {
      producer.run += 1;
      producer.open = true;
      producer.outcome = "none";
    }
    return report("agent_start", ctx);
  });
  pi.on("agent_end", (event, ctx) => {
    producer.outcome = outcomeOf(event.messages ?? []);
    return report("agent_end", ctx);
  });
  pi.on("agent_settled", (_event, ctx) => {
    const sent = report("agent_settled", ctx);
    producer.open = false; // the cycle closes here, whatever continuations it contained
    return sent;
  });
  pi.on("session_shutdown", (event, ctx) => {
    const sent = report("session_shutdown", ctx, { reason: event.reason });
    producer.disabled = true; // Pi re-runs factories after replacement or reload
    return sent;
  });
}
```

- [ ] **Step 5: Run**

Run: `node --test tests/pi_reporting_extension.mjs` — Expected: 6 tests pass (the overflow test takes about two seconds because the first hung helper is killed at the deadline).

- [ ] **Step 6: Commit**

```bash
git add src/pi-reporting-extension.mjs tests/fixtures/pi/pi_host.mjs tests/pi_reporting_extension.mjs
git commit -m "feat(pi): reporting extension with bounded direct helper delivery; Node event host and tests"
```

### Task 9: `report pi --stdin`, shared `InvocationLease::bind`, the Pi receiver

**Files:**
- Modify: `src/report.rs` (add `send_pi_event`, `InvocationLease::bind`)
- Modify: `src/report/codex.rs:248-284` (`Receiver::bind` delegates)
- Modify: `src/report/pi.rs` (append the receiver)
- Modify: `src/cli/args.rs:98-129` (`ReportCommand::Pi`), `src/cli/report.rs:11-21`
- Modify: `src/cli/agent.rs` (`"pi"` arm)
- Modify: `tests/pi_reporting.rs` (append), `tests/server_lifecycle.rs` (Task 7 assertion)

**Interfaces:**
- Produces: `pub fn send_pi_event(input: &[u8], deadline: Instant) -> Result<()>` = `send_payload(input, "pi", "pi-extension", deadline)`.
- Produces: `impl InvocationLease { pub(crate) fn bind(&mut self, provider: AgentProvider, conversation: &str, deadline: Instant) -> Option<bool> }` — `Some(true)` newly bound (caller resets its revision), `Some(false)` already bound to that conversation, `None` refused or unreachable.
- Produces: `pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler` in `src/report/pi.rs`; `pub const EXTENSION_SOURCE: &str`; `pub fn extension_source(binary: &Path) -> Option<String>`.
- CLI: `ovrcr report pi --stdin` reads stdin under a one-second deadline and forwards it; silent `Ok(())` without reading stdin when `OVRCR_AGENT_SOCKET` is absent.

- [ ] **Step 1: Write the failing tests** — append to `tests/pi_reporting.rs`:

```rust
#[test]
fn pi_helper_outside_managed_invocation_is_silent_without_reading_stdin() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ovrcr"));
    command
        .args(["report", "pi", "--stdin"])
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let _held_stdin = child.stdin.take().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("unmanaged pi helper waited on stdin");
        }
        std::thread::yield_now();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn pi_extension_source_embeds_the_json_escaped_binary_path() {
    let source = ovrcr::report::pi::extension_source(std::path::Path::new("/opt/o v r/\"ovrcr\"")).unwrap();
    assert!(source.contains(r#"const OVRCR_BINARY = "/opt/o v r/\"ovrcr\"";"#), "{source}");
    assert!(!source.contains("__OVRCR_BINARY__"));
    assert!(source.contains("[\"report\", \"pi\", \"--stdin\"]"));
}
```

And unit tests at the bottom of `src/report/pi.rs` (they drive `Receiver::handle` without a lease, so only the paths that never publish are exercised):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn unbound() -> Receiver {
        Receiver::new(None)
    }
    fn event(instance: &str, sequence: u64, event: &str, run: Option<u64>, outcome: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"provider":"pi","origin":"pi-extension","payload":{
            "schema":1,"event":event,"mode":"tui","owner_pid":1,"instance":instance,"sequence":sequence,
            "session_id":"sess-a","run":run,"outcome":outcome}}))
        .unwrap()
    }
    #[test]
    fn unknown_producer_is_admitted_only_by_session_start() {
        let mut receiver = unbound();
        assert_eq!(receiver.handle(&event("a", 1, "agent_start", Some(1), "none"), true, Instant::now()), b"admission-ignored\n");
        assert!(receiver.producer.is_none());
        // session_start with no lease cannot bind: the receiver disables itself honestly.
        assert_eq!(receiver.handle(&event("a", 2, "session_start", None, "none"), true, Instant::now()), b"admission-unavailable\n");
        assert!(receiver.disabled);
    }
    #[test]
    fn stale_and_replayed_sequences_are_ignored_before_any_mutation() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 5));
        for sequence in [5, 4, 1] {
            assert_eq!(receiver.handle(&event("a", sequence, "agent_end", Some(1), "ok"), true, Instant::now()), b"admission-ignored\n");
        }
        assert_eq!(receiver.producer, Some(("a".into(), 5)));
        assert!(!receiver.disabled);
        assert_eq!(receiver.revision, 0);
    }
    #[test]
    fn foreign_provider_non_tui_and_grandchild_payloads_are_ignored() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        let mut rpc: serde_json::Value = serde_json::from_slice(&event("a", 2, "agent_start", Some(1), "none")).unwrap();
        rpc["payload"]["mode"] = "rpc".into();
        assert_eq!(receiver.handle(&serde_json::to_vec(&rpc).unwrap(), true, Instant::now()), b"admission-ignored\n");
        let codex = br#"{"provider":"codex","origin":"codex-hook","payload":{"hook_event_name":"Stop"}}"#;
        assert_eq!(receiver.handle(codex, true, Instant::now()), b"admission-ignored\n");
        assert_eq!(receiver.handle(&event("a", 3, "agent_start", Some(1), "none"), false, Instant::now()), b"admission-ignored\n");
        assert_eq!(receiver.producer, Some(("a".into(), 1)), "ignored payloads never advance the sequence");
    }
    #[test]
    fn settled_for_a_run_that_is_not_current_is_ignored() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.current = Some((2, "a:2".into()));
        assert_eq!(receiver.handle(&event("a", 2, "agent_settled", Some(1), "ok"), true, Instant::now()), b"admission-ignored\n");
        assert_eq!(receiver.current, Some((2, "a:2".into())));
    }
    #[test]
    fn duplicate_agent_start_is_checked_before_capacity() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.seen.insert("a:1".into());
        receiver.charged_bytes = MAX_IDENTITY_BYTES;
        assert_eq!(receiver.handle(&event("a", 2, "agent_start", Some(1), "none"), true, Instant::now()), b"admission-ignored\n");
        assert!(!receiver.disabled);
    }
    #[test]
    fn shutdown_for_replacement_retires_the_producer_without_disabling() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.current = Some((1, "a:1".into()));
        let mut shutdown: serde_json::Value = serde_json::from_slice(&event("a", 2, "session_shutdown", Some(1), "ok")).unwrap();
        shutdown["payload"]["reason"] = "reload".into();
        assert_eq!(receiver.handle(&serde_json::to_vec(&shutdown).unwrap(), true, Instant::now()), b"admission-ignored\n");
        assert!(receiver.producer.is_none() && receiver.current.is_none() && !receiver.disabled);
        shutdown["payload"]["reason"] = "quit".into();
        shutdown["payload"]["sequence"] = 3.into();
        receiver.producer = Some(("a".into(), 2));
        receiver.handle(&serde_json::to_vec(&shutdown).unwrap(), true, Instant::now());
        assert!(receiver.disabled);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib report::pi` — Expected: compile error (`Receiver` missing).

- [ ] **Step 3: Implement**

`src/report.rs` — add after `send_codex_hook`:

```rust
pub fn send_pi_event(input: &[u8], deadline: Instant) -> Result<()> {
    send_payload(input, "pi", "pi-extension", deadline)
}
```

and inside `impl InvocationLease`:

```rust
    /// Bind this reservation to `conversation` for `provider`. `Some(true)`: newly bound
    /// (callers reset their revision); `Some(false)`: already bound to it; `None`: refused
    /// or unreachable. Half the remaining budget goes to the command, the rest to the
    /// receipt re-read that recovers a lost reply.
    pub(crate) fn bind(
        &mut self,
        provider: ovrcr_protocol::AgentProvider,
        conversation: &str,
        deadline: Instant,
    ) -> Option<bool> {
        use ovrcr_protocol::{AgentCommand, AgentOperationResult};
        if self.binding.as_ref().is_some_and(|b| b.conversation == conversation) {
            return Some(false);
        }
        let operation = ovrcr_runtime::agent_runner::private_identifier().ok()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let response = self.command(
            operation.clone(),
            AgentCommand::Bind {
                expected_binding: self.binding.clone(),
                conversation: conversation.to_owned(),
            },
            Instant::now() + remaining / 2,
        );
        let response = response.or_else(|_| self.operation_status(operation, deadline));
        match response {
            Ok(Response::AgentOperation(AgentOperationResult::Bound(binding)))
                if binding.conversation == conversation && binding.provider == provider =>
            {
                self.binding = Some(binding);
                Some(true)
            }
            _ => None,
        }
    }
```

`src/report/codex.rs` `Receiver::bind` body becomes:

```rust
    fn bind(&mut self, conversation: &str, deadline: Instant) -> bool {
        let Some(lease) = &mut self.lease else {
            return false;
        };
        match lease.bind(ovrcr_protocol::AgentProvider::Codex, conversation, deadline) {
            Some(true) => {
                self.revision = 0;
                true
            }
            Some(false) => true,
            None => false,
        }
    }
```

(and drop the now-unused `AgentCommand`, `AgentOperationResult`, `Response` imports from `codex.rs`).

`src/cli/args.rs` — add to `ReportCommand`:

```rust
    Pi {
        #[arg(long, required = true)]
        stdin: bool,
    },
```

`src/cli/report.rs` — add the arm beside `Codex`:

```rust
        ReportCommand::Pi { stdin: _ } => {
            if std::env::var_os("OVRCR_AGENT_SOCKET").is_none() {
                return Ok(());
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            if let Ok(input) = app_report::read_hook_stdin(deadline) {
                let _ = app_report::send_pi_event(&input, deadline);
            }
            Ok(())
        }
```

`src/cli/agent.rs` — the `"pi"` arm becomes `"pi" => ovrcr::report::pi::receiver(lease, native_argv),`.

`src/report/pi.rs` — append below `eligible_argv`:

```rust
use super::{HOOK_INPUT_LIMIT, InvocationLease};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, ProviderReport, SampleQuality,
    validate_agent_id,
};
use ovrcr_runtime::agent_runner::{HookEvent, HookHandler, private_identifier};
use std::{collections::HashSet, io::Write, path::PathBuf, time::Instant};

pub const EXTENSION_SOURCE: &str = include_str!("../pi-reporting-extension.mjs");
const EXTENSION_FILE: &str = "ovrcr-pi-reporting.mjs";
const BINARY_TOKEN: &str = "__OVRCR_BINARY__";
const MAX_IDENTITIES: usize = 65_536;
const MAX_IDENTITY_BYTES: usize = 16 * 1024 * 1024;
const ACCEPTED: &[u8] = b"admission-accepted\n";
const IGNORED: &[u8] = b"admission-ignored\n";
const UNAVAILABLE: &[u8] = b"admission-unavailable\n";

/// The extension text for one invocation: the helper path is embedded as a JSON string.
pub fn extension_source(binary: &std::path::Path) -> Option<String> {
    let encoded = serde_json::to_string(binary.to_str()?).ok()?;
    Some(EXTENSION_SOURCE.replacen(BINARY_TOKEN, &encoded, 1))
}

/// A task-owned 0700 directory holding the materialized extension; removed on disable.
fn materialize_extension() -> std::io::Result<(PathBuf, PathBuf)> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let binary = std::env::current_exe()?;
    let source = extension_source(&binary)
        .ok_or_else(|| std::io::Error::other("ovrcr path is not valid UTF-8"))?;
    let dir = std::env::temp_dir().join(format!("ovrcr-pi-{}", private_identifier()?));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let path = dir.join(EXTENSION_FILE);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(source.as_bytes())?;
    Ok((dir, path))
}

pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler {
    let unavailable = if lease.is_none() {
        Some("no managed reservation".to_owned())
    } else if !eligible_argv(argv) {
        Some("unsupported launch arguments".to_owned())
    } else if unsafe { libc::isatty(0) != 1 || libc::isatty(1) != 1 } {
        Some("interactive terminal required".to_owned())
    } else if super::admission::probe_version(&argv[0]).is_none() {
        Some("version probe unavailable".to_owned())
    } else {
        None
    };
    let mut receiver = Receiver::new(lease);
    match unavailable {
        Some(reason) => {
            receiver.disable();
            eprintln!("Pi reporting unavailable ({reason}); running native command");
        }
        None => match materialize_extension() {
            Ok((dir, path)) => {
                receiver.extension_dir = Some(dir);
                argv.insert(1, "-e".into());
                argv.insert(2, path.into_os_string());
            }
            Err(error) => {
                receiver.disable();
                eprintln!("Pi reporting unavailable (extension not materialized: {error}); running native command");
            }
        },
    }
    Box::new(move |event| match event {
        HookEvent::Request { input, native_root, deadline } => receiver.handle(input, native_root, deadline),
        HookEvent::NativeCompleted { .. } => {
            receiver.disable();
            Vec::new()
        }
        HookEvent::Poll { .. } => Vec::new(),
    })
}

struct Receiver {
    lease: Option<InvocationLease>,
    disabled: bool,
    extension_dir: Option<PathBuf>,
    /// The admitted extension instance and its last accepted source sequence.
    producer: Option<(String, u64)>,
    /// The open response cycle: Pi run counter and the cycle identity published as `turn`.
    current: Option<(u64, String)>,
    seen: HashSet<String>,
    charged_bytes: usize,
    revision: u64,
}

impl Receiver {
    fn new(lease: Option<InvocationLease>) -> Self {
        Receiver {
            lease,
            disabled: false,
            extension_dir: None,
            producer: None,
            current: None,
            seen: HashSet::new(),
            charged_bytes: 0,
            revision: 0,
        }
    }
    fn disable(&mut self) {
        self.disabled = true;
        self.current = None;
        if let Some(lease) = self.lease.take() {
            let _ = lease.stream.shutdown(std::net::Shutdown::Both);
            drop(lease);
        }
        if let Some(dir) = self.extension_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    fn handle(&mut self, input: &[u8], native_root: bool, deadline: Instant) -> Vec<u8> {
        if self.disabled {
            return UNAVAILABLE.to_vec();
        }
        if !native_root || input.len() > HOOK_INPUT_LIMIT {
            return IGNORED.to_vec();
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(input) else {
            return IGNORED.to_vec();
        };
        if value["provider"] != "pi" || value["origin"] != "pi-extension" {
            return IGNORED.to_vec();
        }
        let payload = &value["payload"];
        if payload["schema"] != 1 || payload["mode"] != "tui" {
            return IGNORED.to_vec();
        }
        let (Some(event), Some(instance), Some(sequence), Some(session)) = (
            payload["event"].as_str(),
            payload["instance"].as_str().filter(|s| validate_agent_id(s).is_ok()),
            payload["sequence"].as_u64().filter(|s| *s > 0),
            payload["session_id"].as_str().filter(|s| validate_agent_id(s).is_ok()),
        ) else {
            return IGNORED.to_vec();
        };
        // Producer fencing: a new instance is admitted only by its session_start; a stale,
        // replayed or foreign sequence never mutates state, freshness, or Unread.
        match &mut self.producer {
            Some((current, last)) if current.as_str() == instance => {
                if sequence <= *last {
                    return IGNORED.to_vec();
                }
                *last = sequence;
            }
            _ if event == "session_start" => {
                self.producer = Some((instance.to_owned(), sequence));
                self.current = None;
            }
            _ => return IGNORED.to_vec(),
        }
        let run = payload["run"].as_u64();
        let outcome = payload["outcome"].as_str().unwrap_or("none");
        let (state, quality, turn) = match event {
            "session_start" => {
                if !self.bind(session, deadline) {
                    self.disable();
                    return UNAVAILABLE.to_vec();
                }
                (AgentActivity::Idle, SampleQuality::Observed, None)
            }
            "agent_start" => {
                let Some(run) = run else {
                    return IGNORED.to_vec();
                };
                let turn = format!("{instance}:{run}");
                // A repeated start for the same run is the continuation Pi emits for a retry,
                // automatic compaction or a queued follow-up: the cycle is already open and
                // already published. Duplicates are checked before any capacity, binding, or
                // revision change.
                if self.seen.contains(&turn) {
                    return IGNORED.to_vec();
                }
                // A different run while a cycle is open replaces it and publishes the new
                // identity. Permanent disablement is reserved for unresolvable identity or
                // ordering ambiguity (#88); recovering a lost close is #91's job.
                let charge = turn.len() + std::mem::size_of::<String>();
                if self.seen.len() >= MAX_IDENTITIES
                    || self.charged_bytes + charge > MAX_IDENTITY_BYTES
                {
                    self.disable();
                    return UNAVAILABLE.to_vec();
                }
                if !self.bind(session, deadline) {
                    self.disable();
                    return UNAVAILABLE.to_vec();
                }
                self.seen.insert(turn.clone());
                self.charged_bytes += charge;
                self.current = Some((run, turn.clone()));
                (AgentActivity::Busy, SampleQuality::Observed, Some(turn))
            }
            "agent_end" => {
                // Records the outcome on the producer side; Ready waits for the settled boundary.
                return if self.current.as_ref().is_some_and(|(r, _)| Some(*r) == run) {
                    ACCEPTED.to_vec()
                } else {
                    IGNORED.to_vec()
                };
            }
            "agent_settled" => {
                let Some((current_run, turn)) = self.current.clone() else {
                    return IGNORED.to_vec();
                };
                if Some(current_run) != run {
                    return IGNORED.to_vec();
                }
                self.current = None;
                match outcome {
                    "ok" => (AgentActivity::ResponseReady, SampleQuality::Confirmed, Some(turn)),
                    "error" => (AgentActivity::Error, SampleQuality::Observed, Some(turn)),
                    _ => (AgentActivity::Idle, SampleQuality::Observed, Some(turn)),
                }
            }
            "session_shutdown" => {
                if payload["reason"] == "quit" {
                    self.disable();
                } else {
                    // Replacement or reload: Pi re-runs factories; the next session_start admits them.
                    self.producer = None;
                    self.current = None;
                }
                return IGNORED.to_vec();
            }
            "unavailable" => {
                self.disable();
                return UNAVAILABLE.to_vec();
            }
            _ => return IGNORED.to_vec(),
        };
        self.publish(state, quality, turn, deadline)
    }
    fn bind(&mut self, conversation: &str, deadline: Instant) -> bool {
        let Some(lease) = &mut self.lease else {
            return false;
        };
        match lease.bind(AgentProvider::Pi, conversation, deadline) {
            Some(true) => {
                self.revision = 0;
                true
            }
            Some(false) => true,
            None => false,
        }
    }
    fn publish(
        &mut self,
        state: AgentActivity,
        quality: SampleQuality,
        turn: Option<String>,
        deadline: Instant,
    ) -> Vec<u8> {
        let Some(lease) = &self.lease else {
            self.disable();
            return UNAVAILABLE.to_vec();
        };
        let Some(binding) = lease.binding.clone() else {
            self.disable();
            return UNAVAILABLE.to_vec();
        };
        self.revision += 1;
        let report = ProviderReport {
            binding,
            revision: self.revision,
            observation: AgentObservation::Activity(ActivitySample { state, quality, turn }),
        };
        if lease.publish_observation(report, deadline).is_err() {
            self.disable();
            return UNAVAILABLE.to_vec();
        }
        ACCEPTED.to_vec()
    }
}
```

(`InvocationLease::publish_observation`, `stream` and `binding` are private to `report.rs`; `pi.rs` is its child module, so they are reachable exactly as `codex.rs` reaches them. Merge the two `use` blocks at the top of `pi.rs` into one.)

`tests/server_lifecycle.rs` Task 7 test: change the managed assertion to `fixture.wait_terminal_contains(managed.id, "PI_ARGS=-e ");` followed by `fixture.wait_terminal_contains(managed.id, "ovrcr-pi-reporting.mjs --model x/y hello world");` and remove the `reporting unavailable` expectation for the managed session (the receiver is real now; the fake `pi` exits before reporting anything, so `agent` is still `None` after exit). Cite in the commit message.

- [ ] **Step 4: Run**

Run with the prefix: `cargo test -p ovrcr --lib report::`, `cargo test -p ovrcr --test pi_reporting`, `cargo test -p ovrcr --test codex_reporting`, `cargo test -p ovrcr --test server_lifecycle codex_managed`, `cargo test -p ovrcr --test server_lifecycle pi_managed_launch` — Expected: PASS; Codex counts unchanged.

- [ ] **Step 5: Commit**

```bash
git add src/report.rs src/report/codex.rs src/report/pi.rs src/cli/args.rs src/cli/report.rs src/cli/agent.rs tests/pi_reporting.rs tests/server_lifecycle.rs
git commit -m "feat(pi): report pi --stdin, shared lease bind, Pi receiver with producer fencing

Task 7's managed-launch assertion now expects the materialized -e path, since the real
receiver inserts it."
```

### Task 10: managed lifecycle through server, PTY, socket and the real extension

**Files:**
- Modify: `tests/server_lifecycle.rs` (append after `codex_session_named`)

**Interfaces:**
- Produces: `fn pi_session_named(fixture: &ControlFixture, socket: &Path, name: &str) -> (SessionSummary, PathBuf)` — a fake `pi` shell script that answers `--version` with `0.85.1` and otherwise `exec`s `node tests/fixtures/pi/pi_host.mjs "$@"`; `fn pi_callback(fixture, session, &mut index, command)` — sends a stdin line and waits for `PI_CALLBACK=<index>`.

- [ ] **Step 1: Write the fixture and the test**

```rust
fn pi_session_named(
    fixture: &ControlFixture,
    socket: &Path,
    name: &str,
) -> (ovrcr::session::SessionSummary, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let host = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pi/pi_host.mjs");
    let native = fixture._root.path().join("pi");
    std::fs::write(
        &native,
        format!("#!/bin/sh\nif [ \"$1\" = --version ]; then printf '0.85.1\\n'; exit; fi\nexec node '{host}' \"$@\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o700)).unwrap();
    let probe = fixture._root.path().join(format!("{name}-channel"));
    let summary = fixture.create_session_summary(name, vec![
        "/bin/sh".into(), "-c".into(),
        r#"stty -echo; printf '%s\n' "$OVRCR_HOOK_TOKEN" > "$3.capability"; export OVRCR_TEST_PROBE="$3" OVRCR_HOOK_SOCKET="$4"; "$1" agent run pi -- "$2"; printf 'PI_NATIVE_EXIT=%s\n' "$?"; IFS= read -r done"#.into(),
        "pi-fixture".into(), env!("CARGO_BIN_EXE_ovrcr").into(), native.into_os_string(), probe.clone().into_os_string(), socket.as_os_str().into(),
    ]);
    fixture.record_process_group(&summary);
    fixture.wait_terminal_contains_until(summary.id, "PI_NATIVE_READY", Instant::now() + Duration::from_secs(10));
    let terminal = fixture.request(Request::ReadTerminal { session: summary.id, max_lines: None });
    assert!(
        !matches!(&terminal, Response::TerminalText {text,..} if text.contains("reporting unavailable")),
        "unexpected initial admission gate: {terminal:?}"
    );
    (summary, probe)
}

fn pi_callback(fixture: &ControlFixture, session: SessionId, index: &mut usize, command: &str) {
    assert_eq!(
        fixture.request(Request::SendTerminal { session, text: command.into(), submit: true }),
        Response::Ok
    );
    fixture.wait_terminal_contains(session, &format!("PI_CALLBACK={index}"));
    *index += 1;
}

#[test]
fn pi_managed_extension_reports_busy_ready_error_idle_and_fences_producers() {
    let _guard = env_lock();
    use ovrcr::protocol::{AgentActivity, ReporterHealth, SampleQuality};
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "pi-setup");
    let (summary, probe) = pi_session_named(&fixture, &fixture.socket, "pi-hooks");
    let channel = std::fs::read_to_string(&probe).unwrap();
    let channel: Vec<_> = channel.lines().collect();
    // A helper with the right token but a foreign OS parent cannot bind.
    {
        use std::io::Write;
        let mut foreign = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(["report", "pi", "--stdin"])
            .env("OVRCR_AGENT_SOCKET", channel[0])
            .env("OVRCR_AGENT_TOKEN", channel[1])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        foreign.stdin.take().unwrap().write_all(br#"{"schema":1,"event":"session_start","mode":"tui","owner_pid":1,"instance":"ffffffffffffffffffffffffffffffff","sequence":1,"session_id":"foreign","run":null,"outcome":"none","reason":"startup"}"#).unwrap();
        let output = foreign.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(fixture.session_summary(summary.id).agent.is_none());
    }
    let mut index = 0;
    let mut unread_after_first = None;
    let steps: Vec<(&str, Option<(AgentActivity, SampleQuality)>)> = vec![
        ("session_start:sess-a", Some((AgentActivity::Idle, SampleQuality::Observed))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_end:ok", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_settled", Some((AgentActivity::ResponseReady, SampleQuality::Confirmed))),
        ("agent_settled", Some((AgentActivity::ResponseReady, SampleQuality::Confirmed))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_end:error", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_settled", Some((AgentActivity::Error, SampleQuality::Observed))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_end:aborted", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_settled", Some((AgentActivity::Idle, SampleQuality::Observed))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_end:none", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_settled", Some((AgentActivity::Idle, SampleQuality::Observed))),
        ("mode:rpc", None),
        ("agent_start", Some((AgentActivity::Idle, SampleQuality::Observed))),
        ("mode:tui", None),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_end:ok", Some((AgentActivity::Busy, SampleQuality::Observed))),
        ("agent_settled", Some((AgentActivity::ResponseReady, SampleQuality::Confirmed))),
    ];
    for (step, (command, expected)) in steps.into_iter().enumerate() {
        pi_callback(&fixture, summary.id, &mut index, command);
        let current = fixture.session_summary(summary.id);
        let Some((state, quality)) = expected else { continue };
        let agent = current.agent.clone().expect(command);
        let sample = agent.activity.clone().expect(command);
        assert_eq!((sample.state, sample.quality), (state, quality), "step {step}: {command}");
        assert_eq!(agent.binding.conversation, "sess-a");
        assert_eq!(agent.binding.provider, ovrcr::protocol::AgentProvider::Pi);
        if step == 3 {
            let unread = current.unread.clone().expect("first Ready is unread");
            assert_eq!(unread.turn.as_deref(), sample.turn.as_deref());
            unread_after_first = Some(unread);
        }
        if (4..19).contains(&step) {
            assert_eq!(current.unread, unread_after_first, "step {step}: error, cancel and new Busy keep the first Unread");
        }
        if step == 19 {
            let unread = current.unread.clone().expect("second Ready is unread");
            assert_ne!(Some(unread), unread_after_first, "a later cycle replaces the Unread identity");
        }
    }
    pi_callback(&fixture, summary.id, &mut index, "session_shutdown:quit");
    let after = fixture.session_summary(summary.id).agent.unwrap();
    assert_eq!(after.health.state, ReporterHealth::Unavailable, "quit releases the lease");
    assert!(fixture.session_summary(summary.id).unread.is_some(), "reporter loss keeps Unread");
    assert_eq!(
        fixture.request(Request::SendTerminal { session: summary.id, text: "exit".into(), submit: true }),
        Response::Ok
    );
    fixture.wait_terminal_contains(summary.id, "PI_NATIVE_EXIT=17");
}
```

- [ ] **Step 2: Run**

Run: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle pi_managed_extension -- --nocapture` — Expected: PASS. If the `mode:rpc` step publishes Busy, the extension's mode guard is broken; if step 4 (duplicate settled) changes the sample, the receiver's `current` clearing is broken. Also run `cargo test -p ovrcr --test server_lifecycle codex_managed` to prove the shared `bind` did not change Codex.

- [ ] **Step 3: Commit**

```bash
git add tests/server_lifecycle.rs
git commit -m "test(lifecycle): real Pi extension through managed PTY and socket: Busy, Ready, Error, Idle, fencing"
```

### Task 11: Unread, review and one Ready alert through the shipped Dashboard for Pi

**Files:**
- Modify: `tests/server_lifecycle.rs` (`DesktopAlertDashboard::wait_calls` gains a named variant; new test)

- [ ] **Step 1: Generalize the identity assertion** — in `impl DesktopAlertDashboard` add

```rust
    fn wait_named_calls(&mut self, expected: usize, session: SessionId, name: &str) {
        let body = format!("fixture / work / {name} (#{})", session.0);
        let record = self.record.clone();
        self.wait_record(&record, expected, |call| {
            assert!(call.contains(&body), "identity missing: {call}");
            assert!(call.contains("OVRCR · response ready"), "title missing: {call}");
            assert!(!call.contains("CALLBACK"));
            assert!(!call.contains("OVRCR_HOOK_TOKEN"));
            assert!(!call.contains("OVRCR_AGENT_TOKEN"));
            assert!(!call.contains("root.jsonl"));
        });
    }
```

and make `wait_calls` a one-line delegate: `self.wait_named_calls(expected, session, "codex-hooks")`.

- [ ] **Step 2: Write the test**

```rust
#[test]
fn pi_ready_alerts_once_creates_unread_and_explicit_review_clears_only_presented() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "pi-setup");
    let mut dashboard = DesktopAlertDashboard::start(&fixture, Some("desktop_notifications = true\n"));
    dashboard.wait_screen(|screen| screen.contains("fixture"));
    let (summary, _probe) = pi_session_named(&fixture, &fixture.socket, "pi-hooks");
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let config = fixture._root.path().join("config.toml");
    let mut index = 0;
    for command in ["session_start:sess-a", "agent_start", "agent_end:ok", "agent_settled"] {
        pi_callback(&fixture, summary.id, &mut index, command);
    }
    // The Pi session must not be shown in a pane for the alert to fire. If the shipped
    // Dashboard focuses a newly created session automatically, select the `setup` session
    // first with `dashboard.select("setup", "")`-style navigation as the managed-completion
    // visibility test does, and keep `pi-hooks` hidden until the alert is recorded.
    dashboard.wait_named_calls(1, summary.id, "pi-hooks");
    let first = fixture.session_summary(summary.id);
    let first_unread = first.unread.clone().expect("Confirmed Ready is unread");
    let listed = cli_with_output(bin, &config, &fixture.socket, &["terminal", "list", "--json"]);
    assert!(listed.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let row = listed.as_array().unwrap().iter().find(|s| s["id"] == summary.id.0).unwrap();
    assert_eq!(row["unread"], serde_json::to_value(&first_unread).unwrap());
    // Duplicate settled: no second alert, no new Unread.
    pi_callback(&fixture, summary.id, &mut index, "agent_settled");
    assert_eq!(fixture.session_summary(summary.id), first);
    dashboard.wait_named_calls(1, summary.id, "pi-hooks");
    // Opening the pane does not acknowledge; R reviews exactly the presented identity.
    dashboard.select("pi-hooks", "PI_CALLBACK=3");
    dashboard.wait_screen(|screen| screen.contains("Unread"));
    assert_eq!(fixture.session_summary(summary.id), first, "viewing does not acknowledge");
    dashboard.send(b"\x07R");
    dashboard.wait_screen(|screen| !screen.contains("Unread") && screen.contains("response ready"));
    let mut reviewed = first.clone();
    reviewed.unread = None;
    assert_eq!(fixture.session_summary(summary.id), reviewed);
    // A stale acknowledgement cannot clear a newer response.
    for command in ["agent_start", "agent_end:ok", "agent_settled"] {
        pi_callback(&fixture, summary.id, &mut index, command);
    }
    let second = fixture.session_summary(summary.id);
    let second_unread = second.unread.clone().expect("second cycle is unread");
    assert_ne!(second_unread, first_unread);
    let stale = cli_with_output(
        bin, &config, &fixture.socket,
        &["terminal", "mark-reviewed", &summary.id.0.to_string(), "--expected", &serde_json::to_string(&first_unread).unwrap()],
    );
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("Ready observation changed"));
    assert_eq!(fixture.session_summary(summary.id), second);
    // The response is visible in the focused pane: no alert for it.
    dashboard.wait_named_calls(1, summary.id, "pi-hooks");
    dashboard.detach();
}
```

- [ ] **Step 3: Run**

Run with the prefix: `cargo test -p ovrcr --test server_lifecycle pi_ready_alerts`, then `cargo test -p ovrcr --test server_lifecycle desktop_notifications`, `cargo test -p ovrcr --test server_lifecycle codex_unread`, `cargo test -p ovrcr --test server_lifecycle ready_sound` — Expected: all PASS; the Codex filters execute the same counts as before.

- [ ] **Step 4: Commit**

```bash
git add tests/server_lifecycle.rs
git commit -m "test(lifecycle): Pi Ready alerts once, Unread survives duplicates, review is exact"
```

### Task 12: docs, support record, CI Node step, opt-in installed-Pi test, final verification

**Files:**
- Modify: `docs/agent-reporting.md` (new section), `docs/agent-reporting-support.md` (new dated section), `docs/cli-reference.md` (the `agent run pi` row), `docs/dashboard.md` (alert paragraph), `docs/development/testing.md` (Node test command)
- Modify: `.github/workflows/ci.yml` (`checks` and `linux` jobs)
- Modify: `tests/server_lifecycle.rs` (one `#[ignore]` test)

- [ ] **Step 1: CI** — in both the `checks` and `linux` jobs, after the `Configure git identity` step add

```yaml
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - name: Pi extension tests
        run: node --test tests/pi_reporting_extension.mjs
```

- [ ] **Step 2: Docs**

`docs/agent-reporting.md`, new section after the Codex paragraph:

```markdown
## Pi

`ovrcr agent run pi -- pi [ARGS...]` (or picking `pi` in the Dashboard) materializes the
owned reporting extension into a private per-invocation directory and adds `-e <path>`
beside your own extensions; nothing under `~/.pi` changes. The extension is inert without
the private channel and in print, JSON and RPC modes. It reports `session_start` (Idle),
`agent_start` (Busy), and at `agent_settled` one of Ready (`response ready · confirmed`,
because Pi settles only after retries, automatic compaction and queued continuations),
Error (the last assistant message stopped with an error) or Idle (aborted, or no assistant
response). Ready creates one unread identity per response cycle; review and alerts work as
for Codex. Payloads carry identifiers and discriminants only: never prompts, responses or
tool data. Extension dialogs, session switches and reporter recovery are later tickets.
```

`docs/agent-reporting-support.md`, appended section `## Pi 0.85.1 evidence, 2026-09-13`: installed 0.85.1 and its extension declarations were the research anchor; tested versions are evidence, not an allowlist; the settled event's single emission point after retry, compaction and queued continuations is the basis for Confirmed; the Node event host and the managed lifecycle test are the automated proof; native acceptance is tracked by #92.

`docs/cli-reference.md`: replace the `agent run pi` row's "(#89)" with the sentence "Reports Busy, Ready (confirmed at Pi's settled boundary), Error and Idle; creates Unread and Ready alerts."

`docs/dashboard.md` alert paragraph: after "The accepted Codex reporting setup is still required" add "Pi needs no setup beyond the managed launch."

`docs/development/testing.md` commands list: add `node --test tests/pi_reporting_extension.mjs` with the note "Node 22; the Pi extension and its host run outside cargo."

- [ ] **Step 3: Opt-in installed-Pi test** — append to `tests/server_lifecycle.rs`:

```rust
#[test]
#[ignore = "requires OVRCR_TEST_PI_EXECUTABLE (installed pi) and an isolated PI_CODING_AGENT_DIR"]
fn installed_pi_managed_launch_binds_the_real_session_and_stays_idle() {
    let _guard = env_lock();
    use ovrcr::protocol::AgentActivity;
    let pi = std::env::var_os("OVRCR_TEST_PI_EXECUTABLE").expect("OVRCR_TEST_PI_EXECUTABLE");
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "pi-setup");
    let agent_dir = fixture._root.path().join("pi-agent-dir");
    std::fs::create_dir_all(&agent_dir).unwrap();
    let summary = fixture.create_session_summary("pi-native", vec![
        "/bin/sh".into(), "-c".into(),
        r#"export PI_CODING_AGENT_DIR="$3" PI_OFFLINE=1; "$1" agent run pi -- "$2" --no-session --offline; printf 'PI_NATIVE_EXIT=%s\n' "$?"; IFS= read -r done"#.into(),
        "pi-native".into(), env!("CARGO_BIN_EXE_ovrcr").into(), pi, agent_dir.into_os_string(),
    ]);
    fixture.record_process_group(&summary);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let current = fixture.session_summary(summary.id);
        if let Some(agent) = &current.agent
            && agent.activity.as_ref().is_some_and(|s| s.state == AgentActivity::Idle)
        {
            assert_eq!(agent.binding.provider, ovrcr::protocol::AgentProvider::Pi);
            assert!(!agent.binding.conversation.is_empty());
            break;
        }
        assert!(Instant::now() < deadline, "real Pi never reported session_start: {current:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        fixture.request(Request::SendTerminal { session: summary.id, text: "/exit".into(), submit: true }),
        Response::Ok
    );
    fixture.wait_terminal_contains_until(summary.id, "PI_NATIVE_EXIT=", Instant::now() + Duration::from_secs(10));
}
```

Run it once locally with `OVRCR_TEST_PI_EXECUTABLE=/Users/xlyk/.local/bin/pi` and record the outcome (pass or the exact failure) in the task report; do not modify `~/.pi`. If `/exit` is not Pi's quit command in this version, send `\x03\x03` instead and say so.

- [ ] **Step 4: Final verification for the ticket**

Run with the prefix: `node --test tests/pi_reporting_extension.mjs`; `cargo test -p ovrcr --lib`; `cargo test -p ovrcr --test pi_reporting`; `cargo test -p ovrcr --test codex_reporting`; `cargo test -p ovrcr --test server_lifecycle pi_`; `cargo test -p ovrcr --test server_lifecycle codex_`; `cargo test -p ovrcr --test agent_setup`; `cargo test -p ovrcr --test cli`; `cargo test -p ovrcr --test task_runner`; `cargo test -p ovrcr --test task_execution`; `cargo fmt --all -- --check`; `cargo clippy -p ovrcr --all-targets --all-features -- -D warnings`; `git diff --check`. Record executed counts per filter; a filter that runs zero tests is a failure.

- [ ] **Step 5: Commit**

```bash
git add docs/agent-reporting.md docs/agent-reporting-support.md docs/cli-reference.md docs/dashboard.md docs/development/testing.md .github/workflows/ci.yml tests/server_lifecycle.rs
git commit -m "docs(pi): reporting contract and evidence; CI runs the Node extension tests; opt-in installed-Pi test"
```

---

## Whole-branch gates (controller, after #89 merges into `feature/pi-omp-reporting`)

`cargo test --workspace --all-targets --all-features` (run as `--exclude ovrcr` then `-p ovrcr`), `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, `cargo test --workspace --doc`, `node --test tests/pi_reporting_extension.mjs`, then the whole-branch review. Push, PR, CI and merge wait for Kyle.
