# Standards review

**Verdict: PASS — no blocking documented-standard breaches found.**

Reviewed the committed branch `07b792fb9854e38db671ec51fc81ecee25d70907...4cb02011febebf7b2da46620d80acd309a294ffa` (one commit, `4cb02011 refactor(tui): route formed Dashboard requests through Outbox`), its production callers, migrated assertions, and supplied acceptance evidence. HEAD matches the requested revision; the working tree is clean.

The change respects `AGENTS.md`'s scope/coverage rules and `docs/development/architecture.md`'s owning-crate and shared-caller requirements: no new owner, crate, transport, dependency, or wire type appears. `DashboardAction` loses request payloads, while the existing Outbox remains the queue. `docs/development/tui.md` and `CONTEXT.md` describe the revised ownership and exceptions.

The source and assertions satisfy the relevant testing rules: `event_loop.rs:918` drives the real dispatcher/socket writer and checks captured session/run/control epoch/view revision, exact release bytes, History/request order, and no replay; `event_loop.rs:978` covers the ignored-response no-drain boundary. `tests/terminal_acceptance.rs:2670` uses the compiled outer-PTY Dashboard, checks replacement child output, and checks the old child's captured release exactly once. Existing request assertions generally become exact public-drain assertions; helpers reject extra messages. Native evidence separates observed input/History/retarget outcomes from unexercised clipboard/provider/mouse gestures.

**Nonblocking judgment call — P3, possible Mysterious Name:** `crates/ovrcr-tui/src/dashboard/state.rs:956`, `enqueue_request`, executes `self.queue_history_request(); self.push_request(message);`, whereas `push_request` at line 936 only queues. These near-synonyms conceal the ordering distinction at callers. Consider a name such as `enqueue_after_due_history`. The comment, documented ordering, and tests make this a maintainability suggestion, not a documented-standard violation or current defect. The private palette ordering boolean represents an existing required distinction, so I do not flag speculative generality or demand another type.

Supplied logs show macOS owning-crate 345+10, Linux owning-crate 321+10, and root TUI 330 passing cases. These counts do not certify the pending full regression/lint gates.

Skipped: rerunning tests/builds/UI, the separate Spec axis, and tooling-enforced style checks. Full macOS/Linux regression and clippy gates remain root-owned requirements before delivery; no merge-readiness claim is made.
