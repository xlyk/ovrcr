# Operator slash commands & `/tree` branch summarization: native GUI check, 2026-11-15

Source: worktree `operator-slash-commands-490`, parent `0a6dfc0ef` (`operator-slash-commands`), covering GitHub issues `#481` and `#490` (`PROTOCOL_VERSION = 88`, `SCHEMA_VERSION = 35`). The worktree stayed clean except for the intended `#490` changes and this evidence directory.

The GUI (`target/OVRCR GUI.app`) was built from this checkout and launched via `scripts/gui.sh` against a task-owned `HOME` and a disposable `ovrcr-gui-*` root (`/private/tmp/ovrcr-cua-490-35759/tmp/ovrcr-gui-5cfoGz`). `OVRCR_PI_EXECUTABLE` selected installed Pi `0.87.1`, quota probes were disabled (`logs/quota-off.toml`), and the Operator models (`fixture/operator-fixture` and `fixture/operator-fixture-fast`) came from the loopback `operator_pi_gui_provider` test helper in `tests/operator_e2e.rs` with `OVRCR_OPERATOR_GUI_SLASH=1` (seeding compaction settings `reserveTokens: 1024, keepRecentTokens: 1` and deterministic responses for `GUI_SLASH_SEED`, `GUI_BRANCH_ALPHA`, `GUI_BRANCH_BETA`, compaction, and branch summarization). No personal instance, credentials, provider account, or shared state was used.

CUA drove real keyboard input through Codex CLI (`codex-cli 0.164.0`, `cua_repl`) with `getAXStateAndScreenshot({ disableDiffing: true, emit: false })`. No AppleScript or System Events was used. Prompts, launcher/provider logs, and cleanup proof are recorded in `logs/`.

## Attempt 01 — end-to-end native verification (`native/attempt-01/`)

All 16 captures are real 2200×1864 JPEGs paired with unshortened accessibility trees (`.ax.txt`), recorded in `manifest.json` and `cua-report.txt`:

| File | Bytes / JPEG SHA-256 prefix | Verified outcome |
| --- | --- | --- |
| `00-initial-state.jpg` / `.ax.txt` | 193,842 B (`ff5cb6bb…`) | Initial disposable `OVRCR GUI.app` startup prompt (`Connect agent status?`) before declining (`n`) |
| `01-slash-menu-filtered.jpg` / `.ax.txt` | 252,167 B (`3b1965e1…`) | Full Operator (`o`): typing `/mo` opens live `/` command menu above composer rule filtered to `/model` (`Tab` autocompletes `> /model`) |
| `02-typo-rejection-draft-retained.jpg` / `.ax.txt` | 250,829 B (`8c09b1cd…`) | Submitting `/modle` rejects locally (`Unknown slash command "/modle"`) and retains `> /modle` in the composer |
| `03-model-picker.jpg` / `.ax.txt` | 278,833 B (`974e3194…`) | `/model` searchable picker listing `Harness default (Pi profile default)`, `fixture/operator-fixture`, and `fixture/operator-fixture-fast` |
| `04-thinking-picker.jpg` / `.ax.txt` | 260,420 B (`1e6cf43a…`) | `/thinking` reasoning picker displaying `default (Pi's choice)` |
| `05-session-modal.jpg` / `.ax.txt` | 358,829 B (`977cb599…`) | `/session` read-only stats modal after `GUI_SLASH_SEED` + `GUI_BRANCH_ALPHA` (`epoch 0`, `checkpoint turn #2`, `fixture/operator-fixture-fast`, `2 total (2 completed)` turns, `4 total` messages) |
| `06-copy-confirmation.jpg` / `.ax.txt` | 270,370 B (`a530c9c7…`) | `/copy` confirmation (`Copied latest assistant reply to clipboard.`) |
| `07-compact-receipt.jpg` / `.ax.txt` | 281,092 B (`bf5e3841…`) | `/compact focus on protocol 88` settled with `Compacted from 5,026 tokens` receipt and `FIXTURE_COMPACTION_SUMMARY` |
| `08-tree-active-branch.jpg` / `.ax.txt` | 369,959 B (`6dc2eb44…`) | `/tree` open showing `●` active-path markers across `GUI_SLASH_SEED`, `GUI_BRANCH_ALPHA`, and `[compaction]` |
| `09-tree-summarize-modal.jpg` / `.ax.txt` | 331,536 B (`96608289…`) | Selecting `user: GUI_BRANCH_ALPHA` (via `Tab` × 2 `filter: user-only` + search) opens `Summarize branch? · /tree` with `No summary`, `Summarize`, and `Summarize with custom prompt` |
| `10-branch-summary-composer-restore.jpg` / `.ax.txt` | 300,841 B (`0708377d…`) | Choosing `Summarize` completes a managed turn (`Branch summary recorded` + structured Markdown summary) and restores unwrapped `> GUI_BRANCH_ALPHA` into the composer |
| `11-tree-diverging-branches.jpg` / `.ax.txt` | 400,809 B (`92b8b714…`) | After `GUI_BRANCH_BETA` completes on the sibling branch, `/tree` renders diverging alpha/beta branches with `├─` / `└─` connectors, `●` active markers, and `[branch summary]` |
| `12-branch-switched-feed.jpg` / `.ax.txt` | 266,857 B (`47a49523…`) | Selecting alpha's assistant leaf with `No summary` switches the visible Operator feed back to `GUI_BRANCH_ALPHA` (`Switched Operator branch.`) with `GUI_BRANCH_BETA` absent |
| `13-new-session-reset.jpg` / `.ax.txt` | 236,100 B (`e78db7b2…`) | `/new` clears the visible Operator feed and confirms `Started a new Operator session.` |
| `14-quick-message-slash-input.jpg` / `.ax.txt` | 259,016 B (`8ac975e5…`) | From Browse, `Ctrl-o` opens `Quick message · Ctrl-o` with `/session` typed before submission |
| `15-quick-message-slash-executed.jpg` / `.ax.txt` | 319,337 B (`3d67bf41…`) | Submitting `/session` from `Quick message` transitions into full Operator and opens `Operator session · /session` at `epoch 1 · pi none · checkpoint none` with `0 total` turns/messages and `fixture/operator-fixture-fast` preserved |

### Observation noted during CUA

- After `/copy` (`06-copy-confirmation`), the bottom-left Dashboard status banner `Clipboard request sent; paste to verify` stayed visible during subsequent in-Operator interactions until exiting to Browse (`Esc`). It did not block or interfere with any Operator command or modal.

## Cleanup

`OVRCR GUI.app` was closed cleanly via `Cmd-Q` (`cua-report.txt` confirms `isRunning: false` in CUA inventory). Both `scripts/gui.sh` and `operator_pi_gui_provider` exited `0` (`logs/attempt-01-gui-launcher.log`, `logs/attempt-01-provider.log` with `root_exists=false`, and `logs/cleanup.txt`). Both `/private/tmp/ovrcr-cua-490-35759/tmp/ovrcr-gui-5cfoGz` and `/private/tmp/ovrcr-cua-490-35759` were removed, and `ps` confirmed zero remaining `ovrcr-gui`, `OVRCR GUI`, or fixture server processes.

## Not covered here

This native GUI pass verifies the wide desktop layout against installed Pi `0.87.1` and the deterministic loopback provider (`6` HTTP requests total). Custom-prompt branch summarization (`Summarize with custom prompt`), `Esc` step-back navigation (`Custom prompt -> Summarize branch? -> /tree`), foreground `Ctrl-x` `Stop` rollback during in-flight branch summarization, `watch_sessions` lease refusal, and narrow/floor terminal layouts are covered by the automated integration and snapshot suites in `tests/tui/operator.rs`, `tests/operator_worker.rs`, and `tests/operator_e2e.rs`.
