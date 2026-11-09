# Plan: cut fixture cost without weakening any claim

Scope: the three fixture-level optimizations. No assertion changes, no deadline changes, no test deletions, no CI selection changes. This is the companion to `implementation-plan.md`; that document covers scheduling, this one covers the work inside the tests.

Evidence base: hosted run 42429838269 at revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`. Every number below is either measured from that run or counted from source at this revision. No saving here is measured.

## What is actually repeated

Three source facts set the ceiling for this plan:

| Fact | Source | Consequence |
|---|---|---|
| `Live::idle()` runs **5 git subprocesses** and writes 2 fixture files before any test does anything | `tests/support/live.rs:111-145`, `:653-664` | Every fixture construction pays 5 process spawns for identical content |
| **59** `yield_now()` loops exist; **6** of them spawn a process inside the loop body | counted across `tests/` | Poll-replacement is narrower than the 25.4% polling share suggests |
| `init_repo` content is constant: branch `main`, one commit, one `README` | `tests/support/live.rs:653-664` | It is a pure candidate for seeding |

The 25.4% polling share in `accounting.md` includes in-process waits. Only the six spawn loops are addressable by this plan.

## Gate 0 — measure, or do not touch anything

Every item below is gated on a phase measurement that does not exist yet. Instrument `tests/support/live.rs` with opt-in monotonic spans around: `init_repo`, `ClaudeAuth::install`, server spawn, socket readiness, workspace creation, managed session launch, each wait, and teardown. Time teardown **from the caller** — a span closed inside `Live::drop` misses the field destruction after it. Count git and CLI spawns per test.

Decision gates, applied in order:

1. If `init_repo` plus `ClaudeAuth::install` is under ~50 ms per construction, skip items 1 and 3 entirely; the remaining cost is server and session work, and there is nothing to win.
2. If a spawn-per-poll helper averages fewer than ~20 iterations per call, skip item 2 for that helper.
3. Re-measure after each item. Do not stack three changes and attribute the difference to one.

Run instrumentation behind a feature or env flag that is off by default, so normal local runs and CI do not pay for it.

## Item 1 — seed the fixture repository (do this first)

`tests/support/live.rs:653-664` shells out five times per construction: `init`, `config user.name`, `config user.email`, `add`, `commit`. The result is byte-identical every time.

**Change:** build the repository once per test process into a `std::sync::OnceLock<PathBuf>`, then copy it into `live.repo` for each construction. Five process spawns become one directory copy.

**Must preserve:** branch name `main`; the committer identity `OVRCR Tests <tests@example.invalid>`; exactly one commit with message `initial`; a `README` containing `fixture\n`; a real `.git` directory with normal file modes; `repo` still canonicalizes to a real path with no symlinks.

**Traps:**

- The copy must not share writable objects with the seed. Copy the files; do not hardlink `.git/objects`. A test that commits would otherwise corrupt the seed for every later fixture in that process.
- The seed must live in a temp directory owned for the process lifetime, removed when the process exits. Do not put it under `target/` or the repository.
- Do not pin or assert commit hashes. Existing tests read the SHA dynamically via `rev-parse`, which this preserves; a fixed timestamp in the seed would otherwise change identities.
- Keep `init_repo` exported for the tests that call it deliberately to build a second repository.

**Acceptance:** `git_lifecycle` identity and branch assertions still pass; `rev-parse HEAD` still resolves; the full affected binaries pass; spawn counts drop from 5 to 0 per construction in the instrumented record.

**Expected range:** 5 spawns of roughly 25–40 ms each, so ~100–200 ms per construction removed. Against the whole suite that is seconds, not minutes. Do it because it is nearly free and it compounds inside matrix loops, not because it is the answer.

## Item 2 — replace spawn-per-poll waits with in-protocol waits

The six loops that shell out inside their body are the target. The highest-leverage one is a shared helper, so fixing it fixes every caller:

`tests/retained_sessions.rs:1105-1119` `wait_output` spawns `terminal read` on **every iteration**, asserts success, checks for the marker, and yields. The CLI itself performs the same protocol request the test could make directly (`src/cli/resources.rs:189-210`).

**Change:** poll `Request::ReadTerminal` for the same session and the exact marker using bounded requests under the existing `live::wait_deadline()`, with a short bounded pause between attempts. After the marker appears, perform **one** original CLI read and assert success plus the same marker, so the CLI path stays covered.

**Must preserve:** the deadline value, the marker text, the failure message, and the requirement that a missing marker fails rather than passes. Mutations and recovery logic stay untouched in this item.

**Then** apply the same judgement to the other five sites individually — `automatic_titles.rs`, `claude_reporting.rs`, `codex_reporting.rs`, `pi_reporting.rs`, `server_lifecycle.rs`. Do not convert a loop that polls in-process state; there is no spawn there to remove.

**Explicitly out of scope:** negative observation windows. A test that proves "nothing happened for N ms" is not polling for readiness and must not be converted to a notification.

**Acceptance:** spawn count per call drops to one; the marker-missing path still fails inside the deadline (test it by pointing at a marker that never appears); every original assertion still runs.

## Item 3 — stop constructing what a test never touches

`Live::idle()` always builds the repository and installs the `claude` stand-in, whether or not the test uses either. Only **11 files** reference `live.repo`, but that count understates the dependency: any test that creates a workspace or session reaches the repository indirectly. So the guard is not a grep — it is whether the test starts a server.

**Change:** make the repository and the `claude` stand-in lazy behind `OnceLock`, exposed as accessors instead of public fields. Construction then happens on first use: server start calls it, a test that only inspects settings or builds a command does not.

**Why this shape:** the compiler finds every call site, so nothing is missed silently. No test needs migrating to a new variant, and no test's behaviour changes when it does use the repository.

**Acceptance:** full suite green. Instrumented counts show the number of `Live` constructions that never trigger repository construction — that number is the item's entire value, and it is currently unknown. If it is near zero, because nearly every `Live` exists to start a server, abandon this item and keep the lazy accessors only if they read better.

**Risk to watch:** `Live::isolated_home()` and the comment at `tests/support/live.rs:114-115` depend on specific directories *not existing yet*. Laziness must not create anything early, and must not change what exists at assertion time.

## Delivery order

1. Gate 0 instrumentation, and publish the phase numbers.
2. Item 1, seeded repo — one PR, no behaviour change.
3. Item 2, the `wait_output` helper first, then the remaining five sites — one PR each, or one PR if the diff stays small.
4. Item 3, lazy construction — only if Gate 0 shows constructions that skip the server.

Each PR states the measured before and after for the affected binary. Do not combine items; a combined diff cannot attribute its own improvement.

## Guardrails

- No assertion, deadline, retry or selection changes. No test deleted.
- Never share a live server, database, PTY or mutable fixture between nextest processes. Process isolation is what makes this suite trustworthy.
- Never share a mutable fixture across cases within one test either; share the immutable seed, and keep per-case sessions, bridges and evidence slices.
- Crash, recovery, corruption and storage-fault cases keep private servers and storage. Hoisting construction out of their loops is out of scope.
- Fail-closed behaviour must keep failing through the real mechanism. A mocked failure proves the mock.

## What this plan cannot deliver

Together these items model to seconds-to-low-minutes of wall time, not fifteen. The measured ceiling for the whole fixture category is ~95 s even under a loose assumption that all repeated construction were removable, which it is not. The scheduling plan in `implementation-plan.md` is the only path to a materially shorter job. This plan exists because it also reduces flake surface and makes every future run cheaper, and because a suite that constructs five subprocesses to make one file is a defect worth fixing on its own terms.
