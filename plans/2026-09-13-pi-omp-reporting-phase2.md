# Pi / Oh My Pi Reporting, Phase 2 Implementation Plan (tickets #101, #94)

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Workers implement; a different reviewer checks each committed unit. Push, PR and merge require Kyle's authorization.

**Goal:** Finish Pi's operator surface (outcome coverage, setup and doctor with tested-version evidence and session lifecycle diagnostics) and deliver the first Oh My Pi provider slice (managed launch already exists; add the extension, the receiver, Observed Ready, Unread, review and alerts) on top of Phase 1 (PR #102, head b40e2fe).

**Architecture:** Ticket #101 replaces the `managed_doctor` stub with a real doctor for Pi and Oh My Pi (`src/cli/managed.rs`): bounded `--version` probe classified as tested / unverified / unavailable (never an allowlist), optional `--session` inspection through the existing `inspect()` that reports binding, delivery, activity, health and unread, plus docs. Ticket #94 extracts the Pi extension's transport into `src/ovrcr-reporting-transport.mjs` (materialized beside each provider extension), generalizes the Pi receiver into `src/report/extension.rs` parameterized by a `Harness` (provider, origin, file, settled quality), and adds the Oh My Pi extension, which normalizes OMP's events onto the same vocabulary: `agent_end` without a continuation flag is followed by a synthetic `agent_settled` (Observed), and an in-place `session_switch` re-announces the conversation with `session_start`. No new crates or dependencies.

**Tech Stack:** as Phase 1. OMP 18.1.19 is a Bun-compiled binary; its extension loader imports `.mjs` with relative sibling imports; `-e <path>` merges with discovered extensions unless `--no-extensions` (never passed).

**Spec:** GitHub issue #88; tickets #101 and #94. Facts about OMP's events come from the installed binary (recorded in #88's Further Notes): `agent_start`; `agent_end { messages, willContinue }` (no settled event; the `session_stop` hook may continue after a clean end); bare `session_start` / `session_shutdown`; `session_switch { reason: "new"|"resume"|"fork", previousSessionFile }` in place on the same extension instance; `ctx.mode` is `"tui"` only for the interactive terminal UI (task and advisor children are `"print"`, ACP reports `"rpc"`); ordinary handlers are awaited sequentially under a 30 s budget, shutdown handlers in parallel under 2 s.

## Global Constraints

All Phase 1 constraints apply (see `plans/2026-09-13-pi-omp-reporting-phase1.md` "Global Constraints"). In addition:
- No wire change in this phase. `PROTOCOL_VERSION` stays 8. (The Input-request snapshot change belongs to #90.)
- Payload contract gains exactly one optional field, `will_continue` (boolean, OMP only). Nothing else is added.
- Ready quality: Pi `Confirmed` at settled; Oh My Pi `Observed` at its synthetic settled. Never Confirmed for OMP.
- The Codex path is untouched. Pi behavior is unchanged by the receiver extraction: every Phase 1 Pi test keeps its count.
- Worktrees: integration `.worktrees/pi-omp-phase2` (branch `feature/pi-omp-phase2`, from b40e2fe); tickets `.worktrees/ticket-101-pi-doctor` (`ticket/101-pi-doctor`) and `.worktrees/ticket-94-omp-responses` (`ticket/94-omp-responses`). The two tickets run in parallel; #101 merges first, #94 rebases onto the merge before its review.
- Build prefix as before: `CARGO_TARGET_DIR=/Users/xlyk/Code/ovrcr/target CARGO_INCREMENTAL=0`, scoped commands only. Node tests: `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs`.

## Execution order

| Order | Task | Ticket | Depends on |
| --- | --- | --- | --- |
| 1 | `src/cli/managed.rs`: setup and doctor with version evidence and session lifecycle | #101 | — |
| 2 | Pi outcome coverage in the Node suite (tool error then success; resume announces no work) | #101 | — |
| 3 | `docs/pi-reporting-setup.md`, CLI reference, support record | #101 | 1 |
| 4 | Shared transport module; Pi extension imports it; materialization writes two files | #94 | — |
| 5 | `src/report/extension.rs` with `Harness`; `pi.rs`/`omp.rs` thin; `report omp --stdin`; CLI arm | #94 | 4 |
| 6 | OMP extension, host grammar, `tests/omp_reporting_extension.mjs` | #94 | 5 |
| 7 | OMP lifecycle and alert tests, docs, CI, final gates | #94 | 6 |

---

## Ticket #101 — Pi outcomes and diagnostics

### Task 1: `src/cli/managed.rs` — setup and doctor for Pi and Oh My Pi

**Files:**
- Create: `src/cli/managed.rs`
- Modify: `src/cli/mod.rs` (add `mod managed;`), `src/cli/agent.rs` (delete `display_name`, `managed_setup`, `managed_doctor`; dispatch through `managed::`)
- Modify: `tests/pi_reporting.rs` (replace `pi_and_omp_doctor_report_managed_launch_and_reporting_availability`), `tests/server_lifecycle.rs` (new test)

**Interfaces:**
- Produces: `pub(super) struct Managed { name, display, tested_versions: &'static [&'static str], reporting, input_requests, recovery }`, `pub(super) const PI: Managed`, `pub(super) const OMP: Managed`, `pub(super) fn managed(name: &str) -> Option<&'static Managed>`, `pub(super) fn setup(provider: &Managed) -> AppResult<()>`, `pub(super) fn doctor(provider: &Managed, session: Option<u64>, executable: &OsStr) -> AppResult<()>`.
- Doctor JSON: `provider`, `executable`, `probe_status` (`probed`|`unavailable`), `version`, `version_status` (`tested`|`unverified_compatible_until_proven_otherwise`|`unknown`), `tested_versions`, `supported_versions` (the sentence), `capabilities { managed_launch, reporting, input_requests, recovery, metrics: "absent" }`, `session_status` (`not_requested`|`inspection_unavailable`|`session_not_found`|`unbound`|`bound`), `binding { provider, generation }`, `lifecycle { extension, delivery, activity, health, unread }`, `remediation: [..]`.
- `#94` later flips `OMP.reporting` to `"available"`; nothing else in this file changes for it.

- [ ] **Step 1: Write the failing tests**

Replace the doctor test in `tests/pi_reporting.rs`:

```rust
fn fake_harness(dir: &std::path::Path, name: &str, version_line: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then printf '{version_line}\\n'; exit; fi\nexit 3\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn pi_and_omp_doctor_classify_versions_as_evidence_not_an_allowlist() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("pi", fake_harness(dir.path(), "pi-tested", "0.85.1"), "probed", Some("0.85.1"), "tested", "\"available\""),
        ("pi", fake_harness(dir.path(), "pi-newer", "0.86.0"), "probed", Some("0.86.0"), "unverified_compatible_until_proven_otherwise", "\"available\""),
        ("pi", std::path::PathBuf::from("/nonexistent/harness"), "unavailable", None, "unknown", "\"available\""),
        ("omp", fake_harness(dir.path(), "omp-tested", "omp/18.1.19"), "probed", Some("18.1.19"), "tested", "\"pending #94\""),
    ];
    for (provider, executable, probe, version, status, reporting) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_ovrcr"))
            .args(["agent", "doctor", provider, "--json", "--executable"])
            .arg(&executable)
            .env("OVRCR_CONFIG", dir.path().join("registry.toml"))
            .env("OVRCR_SOCKET", dir.path().join("socket"))
            .env_remove("OVRCR_SESSION_ID")
            .output()
            .unwrap();
        assert!(output.status.success(), "{provider} {executable:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["provider"], provider);
        assert_eq!(report["probe_status"], probe, "{executable:?}");
        assert_eq!(report["version"], version.map_or(serde_json::Value::Null, |v| v.into()));
        assert_eq!(report["version_status"], status, "{executable:?}");
        assert_eq!(report["capabilities"]["managed_launch"], true);
        assert_eq!(report["capabilities"]["reporting"].to_string(), reporting);
        assert_eq!(report["capabilities"]["metrics"], "absent");
        assert_eq!(report["session_status"], "not_requested");
        assert!(report["tested_versions"].as_array().unwrap().iter().any(|v| v == if provider == "pi" { "0.85.1" } else { "18.1.19" }));
        assert!(!dir.path().join("socket").exists(), "doctor must not start a server");
    }
}
```

Append to `tests/server_lifecycle.rs` (next to `pi_ready_alerts_once…`):

```rust
#[test]
fn pi_doctor_reports_bound_lifecycle_activity_and_transport_loss() {
    let _guard = env_lock();
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "pi-setup");
    let (summary, _probe) = pi_session_named(&fixture, &fixture.socket, "pi-hooks");
    let bin = env!("CARGO_BIN_EXE_ovrcr");
    let config = fixture._root.path().join("config.toml");
    let id = summary.id.0.to_string();
    let native = fixture._root.path().join("pi");
    let doctor = |fixture: &ControlFixture| -> serde_json::Value {
        let output = cli_with_output(bin, &config, &fixture.socket, &["agent", "doctor", "pi", "--json", "--session", &id, "--executable", native.to_str().unwrap()]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let before = doctor(&fixture);
    assert_eq!(before["session_status"], "unbound");
    assert!(before["remediation"].to_string().contains("agent run pi"));
    let mut index = 0;
    pi_callback(&fixture, summary.id, &mut index, "session_start:sess-a");
    let bound = doctor(&fixture);
    assert_eq!(bound["session_status"], "bound");
    assert_eq!(bound["binding"]["provider"], "Pi");
    assert_eq!(bound["lifecycle"]["extension"], "loaded_and_bound");
    assert_eq!(bound["lifecycle"]["delivery"], "observed");
    assert_eq!(bound["lifecycle"]["activity"], "Idle");
    assert_eq!(bound["lifecycle"]["health"], "Connected");
    assert_eq!(bound["lifecycle"]["unread"], false);
    assert_eq!(bound["version_status"], "tested");
    for command in ["agent_start", "agent_end:ok", "agent_settled"] {
        pi_callback(&fixture, summary.id, &mut index, command);
    }
    let ready = doctor(&fixture);
    assert_eq!(ready["lifecycle"]["activity"], "ResponseReady");
    assert_eq!(ready["lifecycle"]["unread"], true);
    pi_callback(&fixture, summary.id, &mut index, "session_shutdown:quit");
    let lost = doctor(&fixture);
    assert_eq!(lost["lifecycle"]["health"], "Unavailable");
    assert!(lost["remediation"].to_string().contains("fresh managed launch"), "{lost}");
    assert_eq!(lost["lifecycle"]["unread"], true, "reporter loss keeps Unread");
}
```

- [ ] **Step 2: Run to verify they fail**

Run (prefix as always): `cargo test -p ovrcr --test pi_reporting doctor` — Expected: FAIL on `version_status` / `session_status` (fields absent). `cargo test -p ovrcr --test server_lifecycle pi_doctor` — Expected: FAIL on `session_status`.

- [ ] **Step 3: Implement** — `src/cli/managed.rs`:

```rust
//! Setup and doctor for the providers whose managed launch needs no settings: Pi and
//! Oh My Pi load their owned reporting extension from the launch itself. Doctor reports
//! evidence (tested versions, what the live session has actually delivered), never an
//! allowlist.
use super::{AppResult, RuntimeError};
use ovrcr::protocol::{AgentActivity, ReporterHealth};
use serde_json::{Value, json};
use std::ffi::OsStr;

pub(super) struct Managed {
    pub name: &'static str,
    pub display: &'static str,
    /// Releases exercised by this repository's recorded evidence. Not an allowlist.
    pub tested_versions: &'static [&'static str],
    pub reporting: &'static str,
    pub input_requests: &'static str,
    pub recovery: &'static str,
}

pub(super) const PI: Managed = Managed {
    name: "pi",
    display: "Pi",
    tested_versions: &["0.85.1"],
    reporting: "available",
    input_requests: "pending #90",
    recovery: "pending #91",
};

pub(super) const OMP: Managed = Managed {
    name: "omp",
    display: "Oh My Pi",
    tested_versions: &["18.1.19"],
    reporting: "pending #94",
    input_requests: "pending #95",
    recovery: "pending #96",
};

pub(super) fn managed(name: &str) -> Option<&'static Managed> {
    match name {
        "pi" => Some(&PI),
        "omp" => Some(&OMP),
        _ => None,
    }
}

pub(super) fn setup(provider: &Managed) -> AppResult<()> {
    println!(
        "{} reporting needs no settings changes. Launch through `ovrcr agent run {name} -- {name} [ARGS...]` inside an OVRCR terminal, or pick {name} in the Dashboard agent picker. A plain `{name}` launch stays untracked.",
        provider.display,
        name = provider.name
    );
    Ok(())
}

/// The version as the harness prints it: Pi prints `0.85.1`, Oh My Pi prints `omp/18.1.19`.
/// Anything that is not a short version token is treated as no version at all.
fn version_of(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let first = text.lines().next()?.trim();
    let version = first.rsplit('/').next().unwrap_or(first).trim();
    let plausible = !version.is_empty()
        && version.len() <= 64
        && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        && version.chars().next().is_some_and(|c| c.is_ascii_digit());
    plausible.then(|| version.to_owned())
}

pub(super) fn doctor(provider: &Managed, session: Option<u64>, executable: &OsStr) -> AppResult<()> {
    let version = ovrcr::report::admission::probe_version(executable).and_then(|raw| version_of(&raw));
    let (probe_status, version_status) = match &version {
        None => ("unavailable", "unknown"),
        Some(found) if provider.tested_versions.contains(&found.as_str()) => ("probed", "tested"),
        Some(_) => ("probed", "unverified_compatible_until_proven_otherwise"),
    };
    let mut remediation = Vec::<String>::new();
    if version.is_none() {
        remediation.push(format!(
            "The bounded --version probe of {} failed; check the executable path, then rerun doctor.",
            executable.to_string_lossy()
        ));
    }
    let requested = session.or_else(|| std::env::var("OVRCR_SESSION_ID").ok()?.parse().ok());
    let mut binding = Value::Null;
    let mut lifecycle = Value::Null;
    let session_status = match requested {
        None => "not_requested",
        Some(id) => match super::inspect() {
            Err(_) => {
                remediation.push("Session inspection was unavailable; check the configured OVRCR socket and server, then rerun doctor.".into());
                "inspection_unavailable"
            }
            Ok((_, sessions)) => match sessions.iter().find(|session| session.id.0 == id) {
                None => {
                    remediation.push("The requested session is not live; select a current session ID before checking its binding.".into());
                    "session_not_found"
                }
                Some(session) => match &session.agent {
                    None => {
                        remediation.push(format!(
                            "Start {} through `ovrcr agent run {name} -- {name} [ARGS...]` or the Dashboard picker; a plain launch stays untracked.",
                            provider.display,
                            name = provider.name
                        ));
                        "unbound"
                    }
                    Some(agent) => {
                        binding = json!({"provider": agent.binding.provider, "generation": agent.binding.generation});
                        let activity = agent.activity.as_ref().map(|sample| sample.state);
                        lifecycle = json!({
                            // A binding exists only after the extension delivered session_start.
                            "extension": "loaded_and_bound",
                            "delivery": if activity.is_some() { "observed" } else { "bound_without_activity" },
                            "activity": activity,
                            "work_seen": matches!(activity, Some(AgentActivity::Busy | AgentActivity::ResponseReady | AgentActivity::Error | AgentActivity::WaitingInput)),
                            "health": agent.health.state,
                            "unread": session.unread.is_some(),
                        });
                        if agent.health.state == ReporterHealth::Unavailable {
                            remediation.push("Reporting is unavailable for this invocation: the transport was lost or the producer retired. Reporter recovery arrives with a later ticket; start a fresh managed launch to restore reporting.".into());
                        }
                        "bound"
                    }
                },
            },
        },
    };
    let report = json!({
        "provider": provider.name,
        "executable": executable.to_string_lossy(),
        "probe_status": probe_status,
        "version": version,
        "version_status": version_status,
        "tested_versions": provider.tested_versions,
        "supported_versions": "any compatible release; tested versions are evidence, not an allowlist",
        "known_incompatibilities": [],
        "capabilities": {
            "managed_launch": true,
            "reporting": provider.reporting,
            "input_requests": provider.input_requests,
            "recovery": provider.recovery,
            "metrics": "absent",
        },
        "session_status": session_status,
        "binding": binding,
        "lifecycle": lifecycle,
        "remediation": remediation,
    });
    println!("{}", serde_json::to_string_pretty(&report).map_err(RuntimeError::internal)?);
    Ok(())
}
```

`src/cli/agent.rs`: delete `display_name`, `managed_setup`, `managed_doctor`; the Setup arm becomes `name if let Some(provider) = super::managed::managed(name) => super::managed::setup(provider)` (write it as a `match` guard: `"pi" | "omp" => super::managed::setup(super::managed::managed(&provider).expect("managed provider"))`), the Doctor arm likewise `super::managed::doctor(provider, session, &executable)`. `src/cli/mod.rs`: `mod managed;` beside `mod agent;`. If `RuntimeError::internal` does not accept `serde_json::Error`, use `.expect("a JSON value serializes")` as before.

- [ ] **Step 4: Run**

`cargo test -p ovrcr --test pi_reporting`, `cargo test -p ovrcr --test server_lifecycle pi_doctor`, `cargo test -p ovrcr --test agent_setup`, `cargo test -p ovrcr --test cli` — Expected: PASS. `cargo clippy -p ovrcr --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`.

- [ ] **Step 5: Commit**

```bash
git add src/cli/managed.rs src/cli/mod.rs src/cli/agent.rs tests/pi_reporting.rs tests/server_lifecycle.rs
git commit -m "feat(cli): real doctor for pi and omp: version evidence, session lifecycle, remediation"
```

### Task 2: Pi outcome coverage in the Node suite

**Files:** Modify `tests/pi_reporting_extension.mjs`.

- [ ] **Step 1: Add the tests** (both must pass against the current extension; if one fails, the extension's `outcomeOf` is wrong and must be fixed, not the test):

```js
test("a tool error followed by a successful assistant response is a successful cycle", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "agent_start" });
  await host.emit({
    type: "agent_end",
    messages: [
      assistant("toolUse"),
      { role: "toolResult", isError: true, content: [{ type: "text", text: "secret tool output" }] },
      assistant("stop"),
    ],
  });
  await host.emit({ type: "agent_settled" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "ok"], ["agent_settled", 1, "ok"],
  ]);
  assert.ok(!readFileSync(record, "utf8").includes("secret tool output"));
});

test("resume and fork announce the conversation without a cycle; a run whose last assistant errored is an error", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-resumed" });
  await host.emit({ type: "session_start", reason: "resume", previousSessionFile: "/private/old.jsonl" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop"), assistant("error")] });
  await host.emit({ type: "agent_settled" });
  const sent = frames(record);
  assert.deepEqual(sent[0].event, "session_start");
  assert.equal(sent[0].reason, "resume");
  assert.equal(sent[0].run, null);
  assert.ok(!("previousSessionFile" in sent[0]) && !JSON.stringify(sent[0]).includes("old.jsonl"));
  assert.deepEqual(sent.slice(1).map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "error"], ["agent_settled", 1, "error"],
  ]);
});
```

- [ ] **Step 2: Run** `node --test tests/pi_reporting_extension.mjs` — Expected: 9 pass.

- [ ] **Step 3: Commit**

```bash
git add tests/pi_reporting_extension.mjs
git commit -m "test(pi): tool error then success is ok; resume announces without a cycle; no content leaks"
```

### Task 3: docs

**Files:** Create `docs/pi-reporting-setup.md`; modify `docs/cli-reference.md` (doctor row), `docs/agent-reporting-support.md` (Pi section: tested versions list and doctor vocabulary), `docs/agent-reporting.md` (link to the setup guide from the Pi section).

- [ ] **Step 1: Write `docs/pi-reporting-setup.md`**

```markdown
# Pi reporting setup

Pi needs no settings changes. `ovrcr agent run pi -- pi [ARGS...]` inside an OVRCR
terminal, or picking `pi` in the Dashboard, supervises the launch, materializes the
owned reporting extension into a private per-invocation directory, and adds
`-e <path>` beside your own extensions. A plain `pi` launch stays untracked. Nothing
under `~/.pi` is read or written by OVRCR.

## What is reported

| Pi event | OVRCR |
| --- | --- |
| `session_start` (startup, resume, fork, new, reload) | Idle; the conversation is bound to Pi's session id. Historical responses never become Ready. |
| `agent_start` | Busy. Retries, automatic compaction and queued follow-ups stay in the same response cycle. |
| `agent_end` | The cycle's outcome is recorded from the last assistant message (a tool error alone is not a failed run). |
| `agent_settled` | Ready `· confirmed` for a successful cycle, Error for an assistant error, Idle for an aborted or empty cycle. One Unread per Ready. |
| `session_shutdown` | `quit` ends reporting; replacement or reload retires the producer and the next `session_start` re-admits. |

Payloads carry identifiers and discriminants only: never prompts, responses, tool
arguments or results, titles or credentials.

## Doctor

`ovrcr agent doctor pi --json [--session ID] [--executable PATH]` reports:

- `version` and `version_status`: `tested` when the release is one this repository
  has exercised (`tested_versions`), `unverified_compatible_until_proven_otherwise`
  for any other release, `unknown` when the bounded `--version` probe fails.
  Ordinary upgrades stay enabled; there is no allowlist.
- `session_status`: `unbound` until the extension delivered `session_start`;
  `bound` afterwards with `binding` and `lifecycle`.
- `lifecycle.delivery` (`observed` once any activity arrived), `activity`,
  `work_seen`, `health` (`Unavailable` after transport loss or a retired producer),
  `unread`.
- `capabilities`: `reporting` is available; `input_requests` (#90), `recovery`
  (#91) and `metrics` (absent by design) are stated explicitly.

## Removal

There is nothing to remove: the extension lives only in the invocation's private
directory and is deleted when the invocation ends. Stop using the managed launch to
stop reporting.

## Boundaries

Extension dialogs (`WaitingInput`), session switches and reporter recovery are later
tickets. A long-running response is never declared failed because no end event has
arrived. Native acceptance evidence is recorded in
[agent-reporting-support.md](agent-reporting-support.md).
```

- [ ] **Step 2: Other docs** — `docs/cli-reference.md` doctor row: "Probe `--version` (tested / unverified / unknown), inspect an optional `--session` (binding, delivery, activity, health, unread) and print remediation; no server is started." `docs/agent-reporting-support.md` Pi section: add "Tested versions: 0.85.1 (opt-in installed-Pi test, 2026-09-13). Doctor vocabulary is defined in [pi-reporting-setup.md](pi-reporting-setup.md)." `docs/agent-reporting.md` Pi section: add "See [Pi reporting setup](pi-reporting-setup.md)."

- [ ] **Step 3: Gates and commit** — `cargo fmt --all -- --check`, `cargo clippy -p ovrcr --all-targets --all-features -- -D warnings`, `git diff --check`; the Task 1 and Task 2 suites once more.

```bash
git add docs/pi-reporting-setup.md docs/cli-reference.md docs/agent-reporting-support.md docs/agent-reporting.md
git commit -m "docs(pi): setup and doctor guide; tested versions as evidence"
```

---

## Ticket #94 — Oh My Pi responses through the managed route

### Task 4: shared transport module; Pi imports it; materialization writes two files

**Files:**
- Create: `src/ovrcr-reporting-transport.mjs`
- Modify: `src/pi-reporting-extension.mjs` (import the transport; keep the handlers)
- Modify: `src/report/pi.rs` (`materialize_extension` writes the transport file first; `TRANSPORT_SOURCE`, `TRANSPORT_FILE`)
- Modify: `tests/pi_reporting_extension.mjs` (`materialize()` also writes the transport beside the extension)

**Interfaces:**
- Produces: `export function createReporter({ binary, helperArgs, deadlineMs = 900, maxEvents = 256, maxBytes = 1048576 }) -> { producer, report(event, ctx, extra) }` and `export function outcomeOf(messages)`. `producer` is `{ instance, sequence, run, open, outcome, disabled }`, exactly the object the Pi extension owns today.
- File names: `ovrcr-reporting-transport.mjs` beside `ovrcr-pi-reporting.mjs` (and later `ovrcr-omp-reporting.mjs`) in the materialized directory.

- [ ] **Step 1: Write `src/ovrcr-reporting-transport.mjs`** — move `producer`, `frame`, `deliver`, `report`, `outcomeOf` out of the Pi extension verbatim, wrapped:

```js
// Shared bounded delivery for OVRCR reporting extensions (Pi, Oh My Pi). Materialized
// beside each provider extension for one managed invocation. Captures identity and
// ordering synchronously, spawns the helper directly (no shell) so the channel's
// native-parent check holds, keeps one helper in flight, and never carries content.
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";

export function outcomeOf(messages) {
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

export function createReporter({
  binary,
  helperArgs,
  deadlineMs = 900, // inside the channel's one-second callback budget
  maxEvents = 256,
  maxBytes = 1024 * 1024,
}) {
  const producer = {
    instance: randomBytes(16).toString("hex"),
    sequence: 0,
    run: 0,
    open: false,
    outcome: "none",
    disabled: false,
  };
  const queue = [];
  let queuedBytes = 0;
  let draining = Promise.resolve(false);

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
        child = spawn(binary, helperArgs, { stdio: ["pipe", "ignore", "ignore"] });
      } catch {
        resolve(false);
        return;
      }
      const timer = setTimeout(() => child.kill("SIGKILL"), deadlineMs);
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

  function report(event, ctx, extra = {}) {
    if (producer.disabled || ctx.mode !== "tui") return Promise.resolve(false);
    const body = frame(event, ctx, extra);
    if (queue.length >= maxEvents || queuedBytes + body.length > maxBytes) {
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

  return { producer, report };
}
```

`src/pi-reporting-extension.mjs` becomes the header comment plus:

```js
import { createReporter, outcomeOf } from "./ovrcr-reporting-transport.mjs";

// Replaced with the JSON-encoded absolute path of the supervising ovrcr binary when the
// receiver materializes this file for one invocation.
const OVRCR_BINARY = __OVRCR_BINARY__;

export default function (pi) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const { producer, report } = createReporter({ binary: OVRCR_BINARY, helperArgs: ["report", "pi", "--stdin"] });
  // … the five pi.on(...) handlers exactly as today …
}
```

`src/report/pi.rs`: add `pub const TRANSPORT_SOURCE: &str = include_str!("../ovrcr-reporting-transport.mjs");` and `const TRANSPORT_FILE: &str = "ovrcr-reporting-transport.mjs";`; in `materialize_extension`, after creating the directory, write the transport file with the same `create_new` + 0600 options, then the extension file. (`extension_source` is unchanged: only the extension file carries the binary token.)

`tests/pi_reporting_extension.mjs` `materialize()`: `writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(resolve("src/ovrcr-reporting-transport.mjs")));` before writing the extension.

- [ ] **Step 2: Run** `node --test tests/pi_reporting_extension.mjs` — Expected: all pass (7 here, 9 after #101 merges). Then (prefix) `cargo test -p ovrcr --lib report::`, `cargo test -p ovrcr --test pi_reporting`, `cargo test -p ovrcr --test server_lifecycle pi_managed` — Expected: PASS with Phase 1 counts (the materialized directory now holds two files; the removal assertion still holds).

- [ ] **Step 3: Commit**

```bash
git add src/ovrcr-reporting-transport.mjs src/pi-reporting-extension.mjs src/report/pi.rs tests/pi_reporting_extension.mjs
git commit -m "refactor(pi): shared reporting transport module, materialized beside the extension"
```

### Task 5: `src/report/extension.rs` with `Harness`; thin `pi.rs`/`omp.rs`; `report omp --stdin`

**Files:**
- Create: `src/report/extension.rs` (the `Receiver`, `materialize_extension`, `receiver`, `extension_source`, unit tests — moved from `pi.rs` and parameterized)
- Modify: `src/report/pi.rs` (keep `eligible_argv`; add `pub static HARNESS` and `pub fn receiver`/`extension_source` delegates), `src/report/omp.rs` (add its `HARNESS`, `receiver`, `extension_source`), `src/report.rs` (`pub mod extension;`, `send_omp_event`), `src/cli/args.rs` (`ReportCommand::Omp { stdin }`), `src/cli/report.rs` (arm), `src/cli/agent.rs` (`"omp" => ovrcr::report::omp::receiver(lease, native_argv)`), `src/cli/managed.rs` (`OMP.reporting = "available"`)
- Modify: `tests/pi_reporting.rs` (doctor expectation for omp reporting → `"available"`; add the omp helper-inert test)

**Interfaces:**

```rust
pub struct Harness {
    pub provider: AgentProvider,
    pub name: &'static str,
    pub display: &'static str,
    pub origin: &'static str,
    pub extension_file: &'static str,
    pub extension_source: &'static str,
    pub eligible_argv: fn(&[OsString]) -> bool,
    /// Quality of the Ready published at the extension's settled event.
    pub settled_quality: SampleQuality,
}
pub fn receiver(harness: &'static Harness, lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler;
pub fn extension_source(harness: &Harness, binary: &Path) -> Option<String>;
```

`pi.rs`: `pub static HARNESS: Harness = Harness { provider: AgentProvider::Pi, name: "pi", display: "Pi", origin: "pi-extension", extension_file: "ovrcr-pi-reporting.mjs", extension_source: include_str!("../pi-reporting-extension.mjs"), eligible_argv, settled_quality: SampleQuality::Confirmed };` `pub fn receiver(lease, argv) -> HookHandler { super::extension::receiver(&HARNESS, lease, argv) }` `pub fn extension_source(binary: &Path) -> Option<String> { super::extension::extension_source(&HARNESS, binary) }`.
`omp.rs`: the same with `AgentProvider::Omp`, `"omp"`, `"Oh My Pi"`, `"omp-extension"`, `"ovrcr-omp-reporting.mjs"`, `include_str!("../omp-reporting-extension.mjs")` (created in Task 6; create the file in this task with a one-line placeholder comment so the crate builds, then fill it in Task 6), `SampleQuality::Observed`.

- [ ] **Step 1: Move and parameterize** — in `extension.rs` the moved code changes only these lines: envelope check `value["provider"] != harness.name || value["origin"] != harness.origin`; the `agent_settled` `"ok"` arm publishes `harness.settled_quality`; `bind` uses `harness.provider`; `materialize_extension(harness)` names the directory `ovrcr-{name}-{id}` and writes `TRANSPORT_FILE` then `harness.extension_file`; `receiver` prints `"{display} reporting unavailable ({reason}); running native command"` and calls `(harness.eligible_argv)(argv)`; the `session_start` arm additionally sets `self.current = None` before binding (a conversation switch invalidates the open cycle; for a fresh instance it is already `None`). Unit tests move with the code and take `&pi::HARNESS`; add one test that an `omp-extension` envelope is ignored by the Pi harness and vice versa.

- [ ] **Step 2: CLI** — `send_omp_event(input, deadline) -> send_payload(input, "omp", "omp-extension", deadline)`; `ReportCommand::Omp { #[arg(long, required = true)] stdin: bool }`; the `report.rs` arm mirrors `Pi`; `agent.rs` `"omp"` arm; `managed.rs` `OMP.reporting: "available"`; update the omp expectation in `tests/pi_reporting.rs` to `"\"available\""` and add:

```rust
#[test]
fn omp_helper_outside_managed_invocation_is_silent_without_reading_stdin() {
    // identical to the pi helper test with ["report", "omp", "--stdin"]
}
```

- [ ] **Step 3: Run** (prefix) `cargo test -p ovrcr --lib report::`, `cargo test -p ovrcr --test pi_reporting`, `cargo test -p ovrcr --test codex_reporting`, `cargo test -p ovrcr --test server_lifecycle pi_` — Expected: PASS with Phase 1 counts plus the new tests; `cargo clippy -p ovrcr --all-targets --all-features -- -D warnings` clean.

- [ ] **Step 4: Commit**

```bash
git add src/report/extension.rs src/report/pi.rs src/report/omp.rs src/omp-reporting-extension.mjs src/report.rs src/cli/args.rs src/cli/report.rs src/cli/agent.rs src/cli/managed.rs tests/pi_reporting.rs
git commit -m "refactor(report): one extension receiver parameterized by Harness; omp wired through report omp --stdin"
```

### Task 6: OMP extension, host grammar, Node tests

**Files:**
- Create/fill: `src/omp-reporting-extension.mjs`
- Modify: `tests/fixtures/pi/pi_host.mjs` (grammar: `agent_end:<outcome>[:continue]`, `session_switch:<id>:<reason>`; document that the host serves both harnesses)
- Create: `tests/omp_reporting_extension.mjs`

- [ ] **Step 1: Write the failing Node tests** — `tests/omp_reporting_extension.mjs` reuses the Pi test's helpers (copy `materialize`/`frames`/`managed` with `SOURCE = resolve("src/omp-reporting-extension.mjs")`, extension file name `ovrcr-omp-reporting.mjs`, helper args `report omp --stdin`):

```js
test("inert outside a managed invocation and outside the terminal UI", async () => { /* as Pi: registered() is []; then with env, mode "print": no frames */ });

test("registers OMP's lifecycle and switch events", async () => {
  managed();
  const host = await createHost(materialize().extension);
  assert.deepEqual(host.registered(), ["agent_end", "agent_start", "session_shutdown", "session_start", "session_switch"]);
});

test("an end that will continue keeps the cycle open; the final end settles it as observed work", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "session_start" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop")], willContinue: true });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop")] });
  assert.deepEqual(frames(record).map((f) => [f.event, f.run, f.outcome, f.will_continue ?? null]), [
    ["session_start", null, "none", null],
    ["agent_start", 1, "none", null],
    ["agent_end", 1, "ok", true],
    ["agent_start", 1, "ok", null],
    ["agent_end", 1, "ok", false],
    ["agent_settled", 1, "ok", null],
  ]);
  // A later start after the settled end is a new cycle (a stop-hook continuation).
  await host.emit({ type: "agent_start" });
  assert.equal(frames(record).at(-1).run, 2);
});

test("error and aborted ends settle as error and aborted; no assistant message is none", async () => { /* three cycles, assert outcomes */ });

test("an in-place session switch re-announces the conversation and closes any open cycle", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "session_start" });
  await host.emit({ type: "agent_start" });
  host.state.session = "sess-b";
  await host.emit({ type: "session_switch", reason: "resume", previousSessionFile: "/private/a.jsonl" });
  await host.emit({ type: "agent_start" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.session_id, f.run, f.reason ?? null]), [
    ["session_start", "sess-a", null, "startup"],
    ["agent_start", "sess-a", 1, null],
    ["session_start", "sess-b", null, "resume"],
    ["agent_start", "sess-b", 1, null],
  ]);
  assert.ok(!JSON.stringify(sent).includes("a.jsonl"));
});

test("shutdown reports once with reason quit and disables", async () => { /* as Pi */ });
```

- [ ] **Step 2: Write the extension** — `src/omp-reporting-extension.mjs`:

```js
// OVRCR reporting for a managed interactive Oh My Pi session. Loaded only by
// `ovrcr agent run omp` through an explicit -e path beside the user's own extensions;
// inert without the private invocation channel and outside the terminal UI (task and
// advisor sessions run in this same process with mode "print"). Oh My Pi has no settled
// event: an agent_end without the continuation flag closes the cycle, and OVRCR treats
// that Ready as Observed because the stop hook may still continue — a later agent_start
// then opens a new cycle. Session switches happen in place on this same instance, so a
// switch re-announces the conversation with session_start.
import { createReporter, outcomeOf } from "./ovrcr-reporting-transport.mjs";

const OVRCR_BINARY = __OVRCR_BINARY__;

export default function (omp) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const { producer, report } = createReporter({ binary: OVRCR_BINARY, helperArgs: ["report", "omp", "--stdin"] });

  function announce(ctx, reason) {
    producer.run = 0;
    producer.open = false;
    producer.outcome = "none";
    return report("session_start", ctx, { reason });
  }

  omp.on("session_start", (_event, ctx) => announce(ctx, "startup"));
  omp.on("session_switch", (event, ctx) => announce(ctx, event.reason));
  omp.on("agent_start", (_event, ctx) => {
    if (!producer.open) {
      producer.run += 1;
      producer.open = true;
      producer.outcome = "none";
    }
    return report("agent_start", ctx);
  });
  omp.on("agent_end", async (event, ctx) => {
    producer.outcome = outcomeOf(event.messages ?? []);
    const willContinue = Boolean(event.willContinue);
    const end = report("agent_end", ctx, { will_continue: willContinue });
    if (willContinue) return end;
    await end;
    const settled = report("agent_settled", ctx);
    producer.open = false;
    return settled;
  });
  omp.on("session_shutdown", (_event, ctx) => {
    const sent = report("session_shutdown", ctx, { reason: "quit" });
    producer.disabled = true;
    return sent;
  });
}
```

Host grammar additions in `tests/fixtures/pi/pi_host.mjs` `main()`:

```js
      } else if (command === "agent_end") {
        const messages = a === "none" ? [] : [assistant(a === "ok" ? "stop" : a)];
        await host.emit({ type: "agent_end", messages, ...(b === "continue" ? { willContinue: true } : {}) });
      } else if (command === "session_switch") {
        if (a) host.state.session = a;
        await host.emit({ type: "session_switch", reason: b ?? "resume" });
```

and update the header comment: the host serves both the Pi and the Oh My Pi extension.

- [ ] **Step 3: Run** `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs` — Expected: all pass, no leftover tempdirs.

- [ ] **Step 4: Commit**

```bash
git add src/omp-reporting-extension.mjs tests/fixtures/pi/pi_host.mjs tests/omp_reporting_extension.mjs
git commit -m "feat(omp): reporting extension with observed settling and in-place switch announcement; Node tests"
```

### Task 7: OMP lifecycle and alert tests, docs, CI, final gates

**Files:** Modify `tests/server_lifecycle.rs`, `docs/agent-reporting.md`, `docs/agent-reporting-support.md`, `docs/cli-reference.md`, `docs/dashboard.md`, `.github/workflows/ci.yml`.

- [ ] **Step 1: Fixtures** — `omp_session_named(fixture, socket, name)` is `pi_session_named` with the fake script answering `--version` with `omp/18.1.19`, the executable named `omp`, the session command `agent run omp -- "$2"`, and the same Node host. Factor the shared body into `fn harness_session_named(fixture, socket, name, harness: &str, version_line: &str)` and make both wrappers one-liners.

- [ ] **Step 2: Lifecycle test**

```rust
#[test]
fn omp_managed_extension_reports_observed_ready_continuations_switches_and_child_inertness() {
    let _guard = env_lock();
    use ovrcr::protocol::{AgentActivity, ReporterHealth, SampleQuality};
    let fixture = ControlFixture::new_bounded();
    fixture.create_hook_child("setup", "omp-setup");
    let (summary, _probe) = omp_session_named(&fixture, &fixture.socket, "omp-hooks");
    let mut index = 0;
    let mut first_unread = None;
    let mut continued_turn = None;
    let steps: Vec<(&str, Option<(AgentActivity, SampleQuality, &str, u64)>)> = vec![
        ("session_start:sess-a", Some((AgentActivity::Idle, SampleQuality::Observed, "sess-a", 1))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed, "sess-a", 1))),
        ("agent_end:ok:continue", Some((AgentActivity::Busy, SampleQuality::Observed, "sess-a", 1))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed, "sess-a", 1))),           // continuation: same turn
        ("agent_end:ok", Some((AgentActivity::ResponseReady, SampleQuality::Observed, "sess-a", 1))), // observed Ready, Unread
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed, "sess-a", 1))),           // stop-hook continuation: new cycle
        ("agent_end:error", Some((AgentActivity::Error, SampleQuality::Observed, "sess-a", 1))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed, "sess-a", 1))),
        ("agent_end:aborted", Some((AgentActivity::Idle, SampleQuality::Observed, "sess-a", 1))),
        ("mode:print", None),
        ("agent_start", Some((AgentActivity::Idle, SampleQuality::Observed, "sess-a", 1))),           // in-process child: inert
        ("mode:tui", None),
        ("session_switch:sess-b:resume", Some((AgentActivity::Idle, SampleQuality::Observed, "sess-b", 2))),
        ("agent_start", Some((AgentActivity::Busy, SampleQuality::Observed, "sess-b", 2))),
        ("agent_end:ok", Some((AgentActivity::ResponseReady, SampleQuality::Observed, "sess-b", 2))),
    ];
    for (step, (command, expected)) in steps.into_iter().enumerate() {
        pi_callback(&fixture, summary.id, &mut index, command);
        let current = fixture.session_summary(summary.id);
        let Some((state, quality, conversation, generation)) = expected else { continue };
        let agent = current.agent.clone().expect(command);
        let sample = agent.activity.clone().expect(command);
        assert_eq!((sample.state, sample.quality), (state, quality), "step {step}: {command}");
        assert_eq!(agent.binding.provider, ovrcr::protocol::AgentProvider::Omp);
        assert_eq!((agent.binding.conversation.as_str(), agent.binding.generation), (conversation, generation), "step {step}");
        match step {
            1 => continued_turn = sample.turn.clone(),
            3 => assert_eq!(sample.turn, continued_turn, "continuation keeps the cycle"),
            4 => {
                assert_eq!(sample.turn, continued_turn);
                first_unread = Some(current.unread.clone().expect("observed Ready is unread"));
            }
            5..=12 => assert_eq!(current.unread, first_unread, "step {step}: later work keeps the first Unread"),
            14 => assert_ne!(current.unread, first_unread, "a new cycle after the switch replaces the Unread"),
            _ => {}
        }
    }
    pi_callback(&fixture, summary.id, &mut index, "session_shutdown:quit");
    assert_eq!(fixture.session_summary(summary.id).agent.unwrap().health.state, ReporterHealth::Unavailable);
    assert!(fixture.session_summary(summary.id).unread.is_some());
    assert_eq!(fixture.request(Request::SendTerminal { session: summary.id, text: "exit".into(), submit: true }), Response::Ok);
    fixture.wait_terminal_contains(summary.id, "PI_NATIVE_EXIT=17");
}
```

(Verify against the runtime whether a Bind to a new conversation keeps `unread`; Phase 1's report says Unread is retained across reporting resets for the server lifetime. If the runtime clears it, the step 12 expectation is wrong: stop and report rather than weakening.)

- [ ] **Step 3: Alert test** — `omp_ready_alerts_once_and_creates_unread`: the first half of `pi_ready_alerts_once…` (start the Dashboard, hide the session, `session_start:sess-a`, `agent_start`, `agent_end:ok`, `wait_named_calls(1, id, "omp-hooks")`, CLI `terminal list --json` shows `unread`, a repeated `agent_end:ok` (no open cycle → ignored) adds no alert). Also the picker shape: extend `pi_managed_launch_preserves_argv…` with an `omp` managed session asserting `PI_ARGS=-e ` + `ovrcr-omp-reporting.mjs` and that the extension directory is removed, or add `omp_managed_launch_inserts_its_extension_and_removes_it` reusing the same fake-script pattern.

- [ ] **Step 4: Docs and CI** — `docs/agent-reporting.md`: an `## Oh My Pi` section mirroring Pi's: Observed Ready at an end without continuation, stop-hook continuation opens a new cycle, in-place switches re-announce, children inert; `docs/agent-reporting-support.md`: `## Oh My Pi 18.1.19 evidence, 2026-09-13` (bundled binary inspected; no extension type declarations survive compilation; approvals and questions are #95); `docs/cli-reference.md`: the `agent run omp` row loses "(reporting arrives with #94)"; `docs/dashboard.md`: "Pi and Oh My Pi need no setup beyond the managed launch."; `.github/workflows/ci.yml`: the Node step runs both test files.

- [ ] **Step 5: Final gates for the ticket** — (prefix) `node --test tests/pi_reporting_extension.mjs tests/omp_reporting_extension.mjs`; `cargo test -p ovrcr --lib`; `cargo test -p ovrcr --test pi_reporting`; `cargo test -p ovrcr --test codex_reporting`; `cargo test -p ovrcr --test server_lifecycle pi_`; `cargo test -p ovrcr --test server_lifecycle omp_`; `cargo test -p ovrcr --test server_lifecycle codex_`; `cargo test -p ovrcr --test agent_setup`; `cargo test -p ovrcr --test cli`; `cargo fmt --all -- --check`; `cargo clippy -p ovrcr --all-targets --all-features -- -D warnings`; `git diff --check`. Record executed counts.

- [ ] **Step 6: Commit**

```bash
git add tests/server_lifecycle.rs docs/agent-reporting.md docs/agent-reporting-support.md docs/cli-reference.md docs/dashboard.md .github/workflows/ci.yml
git commit -m "test(omp): observed Ready, continuations, switches and child inertness through the managed route; docs; CI"
```

---

## Whole-branch gates (controller, after both tickets merge into `feature/pi-omp-phase2`)

`.superpowers/sdd/2026-09-13-pi-omp-phase2/branch-gates.sh`, then the whole-branch review. Push, PR (base `feature/pi-omp-reporting` while #102 is open, else `main`), CI and merge wait for Kyle.
