# Issue #58 installation preparation

Status: **prepared, not installed or accepted for normal use**. The user requested
work to begin in an isolated worktree. Merge, permanent installation, live config
and trust changes, and a transition of the existing server await authorization.
No product code changed; this work reuses PR #56.

## Revision and review preflight

- Source: `86ef46b8eff3128566f76022c2cfef49714031ac`, PR #56.
- Worktree: `/Users/xlyk/Code/ovrcr/.worktrees/issue-58-ready-install`, branch
  `codex/issue-58-ready-install`. The original checkout and other worktrees were
  left unchanged.
- Refreshed main: `983dd42a05690b15db073e6c1d8454d5a889db63`, an ancestor of
  this source (0 behind, 19 ahead). No merge conflict requires a correction.
- [PR metadata](pr56-preflight.json): open, ready, mergeable/clean; all four
  [current-head CI jobs](https://github.com/xlyk/ovrcr/actions/runs/34572184009)
  passed at the source SHA above. This is existing hosted evidence, not a new run.
- [Review requirements](review-requirements.json): no review threads or branch
  protection rules returned. GitHub has no submitted reviews; retained PR #56
  evidence records independent agent review. The repository still requires
  independent review and authorization to merge.

## Host and configuration preflight

[Machine-readable preflight](preflight.json) records binary/config hashes and the
private candidate draft location. The full user config and its native trust
values are not committed here.

- Installed Codex: `/opt/homebrew/bin/codex`, resolving to
  `/opt/homebrew/Caskroom/codex/0.153.0/bin/codex`; actual version `0.153.0`.
- No `ovrcr` on this shell's PATH. The documented destination
  `/Users/xlyk/.local/bin/ovrcr`, plus `.cargo/bin/ovrcr` and
  `/opt/homebrew/bin/ovrcr`, were absent. The proposed permanent destination is
  `/Users/xlyk/.local/bin/ovrcr`; using its absolute path avoids assuming PATH setup.
- The existing dashboard PID `43226` and server PID `43232` use
  `/Users/xlyk/Code/ovrcr/target/debug/ovrcr`. That binary reports
  `ovrcr 0.1.0 (protocol 4)`; the server socket is
  `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-501/ovrcr/server.sock`.
  No matching user LaunchAgent file was found. These observations do not authorize
  termination; the live processes and socket were left intact.
- Candidate: `target/release/ovrcr`, `ovrcr 0.1.0 (protocol 6)`, SHA-256
  `663ca41654b694acc7cfb232c4fd8a0bb18d3169c07e4eeff4b649739d5585ae`.
  Building or installing it cannot replace the protocol-4 server or transfer PTYs.
- Print-only setup against `/Users/xlyk/.codex/config.toml` adds one group each
  for SessionStart, UserPromptSubmit, Stop, Interrupt and SessionEnd. A parsed
  comparison removed only those additions and recovered the entire original
  value. Existing handler arrays retained their order; existing trust values
  stayed identical. Repeated composition was semantically identical. The live
  file's bytes and SHA-256 remained unchanged.
- [Candidate doctor](doctor-candidate.json) accepts the exact installed Codex
  version and supplied candidate file. [Live read-only doctor](doctor-live-read-only.json)
  reports the five reporters missing. Both leave effective configuration, trust
  and delivery unverified. Neither command created the isolated server socket.
- [Candidate hook additions](candidate-hook-additions.toml) name the built
  executable. [Proposed permanent additions](proposed-installed-hooks.toml)
  change only that command path to the proposed install destination. They are
  review material, not an applied or trusted configuration. Regenerate with the
  permanent binary before applying, as described in the
  [installation and rollback guide](../../docs/codex-ready-installation.md).

## Validation

Each attempt has a command, tested revision, environment, timestamps and exit
status in `attempts/*.json`, with complete output in its matching log. These are
existing regressions; no new behavior required a new test seam.

| Check | Current result |
| --- | --- |
| All-target, all-feature workspace typecheck | Passed |
| Codex setup/doctor public CLI regressions | 3 passed, 0 failed |
| Codex launch/version/native-exit regressions | 4 passed, 0 failed |
| Locked offline release build | Passed |
| Full all-target, all-feature workspace suite | Failed: 212 passed, 2 failed, 11 ignored across 12 reached binaries; later binaries unrun |
| Each failed lifecycle case, isolated follow-up | 1 passed each; full attempt remains failed |
| All-target, all-feature Clippy / formatting | Passed |
| Installed managed native macOS acceptance | Not run; authorization gate |
| New Linux native/provider acceptance | Not run; no such claim |

No native provider was started in this preparation. The prior PR's macOS fixture,
Linux CI, capacity and memory evidence remains at its recorded revision and does
not establish a normal installation on this host.

## Failed preparation attempts

- The full suite failed `agent_admission_failed_unavailable_publication_disconnects_watch`
  and `agent_admission_lost_bind_then_clear_resolves_without_another_startup`.
  Both stopped at `tests/server_lifecycle.rs:8807`: `supervisor must select a fresh
  UUID`, length 12 rather than 36. Source inspection places this in shared Claude
  fixture admission, before either intended fault assertion. The fixture passes
  `--agent fixture-root`; an unchanged native argument list would give its helper
  that 12-character value. That is a possible fallback explanation, not a proven
  reason the version/admission path was unavailable. Each exact case passed once
  in isolation afterward. No source, deadline, assertion or test threading was
  changed. The failed full run remains failed; later binaries were not executed.
- [Cleanup observation](cleanup-observation.json) found no executable processes
  from this task checkout, both failed fixture directories absent, and the original
  server/socket present. This is not a complete per-process-group native manifest.
  The private configuration draft and reviewed build remain for continuation.
- Initial GitHub access failed in the restricted network sandbox; the authorized
  network retry succeeded. The original checkout was behind main, so the new
  worktree uses the fetched exact PR source rather than that checkout.
- A sandboxed process inventory returned `Operation not permitted`. The
  permission-reviewed read-only retry identified the live server; no process was
  signaled.
- The first attempt to save review metadata omitted `query=` in the `gh api -f`
  argument and failed with `invalid key`. Corrected read-only query succeeded.

## Remaining issue gates

1. Obtain applicable merge/install/configuration authorization; refresh PR #56
   again immediately before any merge. No release is included.
2. Resolve the protocol-4 live server transition with its session owners. Retain
   the compatible old executable and do not force a shutdown.
3. Install the approved binary at the agreed path, verify its hash/version and
   actual server executable, compose from that permanent path, and apply only the
   reviewed configuration change. Preserve any intervening unrelated edits.
4. Complete hook review through native Codex, then capture actual assistant
   output, Ready, next Busy, interruption and exit health through the installed
   managed launch. Keep isolated fixture config, socket and workspace and retain
   screenshots/accessibility, snapshot and owned cleanup evidence.
5. Record the installed/native outcomes before marking #58 complete. No metrics,
   accounting, Confirmed completion, notifications or unread behavior is added.
