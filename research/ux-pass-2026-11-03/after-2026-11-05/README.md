# Native verification — 2026-11-05

Tested checkout: `/Users/xlyk/Code/ovrcr/.worktrees/ux-pass-integration`, branch `integration/ux-pass-2026-11-03`, revision `78ce52fd62ddc4522e53fe23afd97d29e6fe60c4`. Tracked tree was clean. No product source changed during either walkthrough. Binary SHA-256 values are in [binary-hashes.json](binary-hashes.json).

The first run was built and launched through `rtk proxy just gui`. The retry reused the same frozen app bundle without rebuilding or changing the target while automated tests ran. Both runs used private HOME, configuration, sockets and repositories, quota disabled, and task-owned provider stubs ahead of installed commands. Synthetic Claude generated local JSONL and accepted reporting callbacks; it made no provider request. Other provider command stubs refused execution. No personal accounts or live OVRCR instance were used.

## Observed checks

Each linked JPEG has matching `.txt` accessibility evidence with the same basename. Pixels were inspected as well as text.

| Check | Native evidence and result |
| --- | --- |
| Onboarding | [01](01-private-onboarding.jpg): one Connect / Not now screen names only the private profile paths. Declined setup. |
| Real terminal input | [03](03-input-unicode.jpg), [05](05-created-workspace-input.jpg), [12](12-retry-shell-input.jpg): separate `CUA_INPUT_OK`, `CUA_W_SHELL_OK`, `RETRY_INPUT_OK` output, beyond command echo. |
| Unicode | [05](05-created-workspace-input.jpg): pasted command renders red `RED 界🙂 END`, including CJK and emoji. `typeText` dropped those characters in attempt 03; paste delivered them. This does not establish IME composition. |
| Workspace validation | [04](04-workspace-validation.jpg): blank `feature/` remains in the form, Branch gets focus and `Add a name after feature/` appears. |
| Submission and folder name | [05](05-created-workspace-input.jpg): Enter from Branch creates one terminal; `pwd` ends in `/consigint/feature-cua-workspace`. |
| Cards | [06](06-workspace-card.jpg), [25](25-narrow-workspace-card.jpg), [26](26-project-card.jpg): selected rows show paths, sessions and next keys. At 783×624 pixels the split card retains state/age. Empty split headers still say `no session`; the card itself names its workspace/project (existing intentional split behavior covered by card acceptance). |
| Task launch | [07](07-agent-task-form.jpg), [08](08-agent-ready-unread.jpg): Task is first and the launched synthetic session uses its title. [24](24-task-derived-branch.jpg): `CUA task branch` derives `feature/cua-task-branch`; cancelled without creation. |
| Attention and pane header | [08](08-agent-ready-unread.jpg): `Ready · unread reply`, `✓ 1 ready`, task title and token summary. [17](17-transcript-resume-ack.jpg): after a reply, unread wording and the attention count are gone. |
| Commands and settings | [13](13-settings-command.jpg): searching `theme` returns `Settings: Theme` with `Appearance · theme`, Enter opens Settings on Theme. |
| Split selection | [14](14-split-chooser.jpg): `v` presents `New pane · choose its session` with full destination paths; Escape keeps the suggested pane. |
| Ctrl-O pass-through | [15](15-ctrl-o-passthrough.jpg): shell `stty -icanon -echo -iexten min 1; cat -v` displays `^O`; no Quick message opens. Ctrl-C and `stty sane` restore the shell. |
| Transcript contents | [10](10-transcript-open.jpg), [16](16-transcript-modal-input.jpg): synthetic user prompt, assistant text and `Read /fixture/example.rs` summary; End reload retains content. |
| Transcript input safety | [16](16-transcript-modal-input.jpg): typed `BLOCKED_KEY`, pasted `BLOCKED_PASTE`, Return and a click stay inside the modal. [17](17-transcript-resume-ack.jpg): Escape, Return, `AFTER_CLOSE` yields only `TRANSCRIPT_NATIVE_ACK:AFTER_CLOSE`; no blocked text is echoed or acknowledged. Native loading races remain automated evidence. |
| Transcript wheel | [18](18-transcript-wheel.jpg): wheel-up over the alternate-screen agent opens Transcript. |
| Ordinary History | [19](19-native-history.jpg): wheel-up opens frozen shell rows. [22](22-ed3-cleared-terminal.jpg), [23](23-history-cleared.jpg): actual ED3 plus clear-screen command prints `ED3_CLEARED`; reopened History contains that marker and prompt, with old `HISTORY_ROW_*` entries absent. Three rows per individual wheel tick is automated evidence; the CUA page gesture emits multiple ticks. |
| Resize | [21](21-narrow-dashboard.jpg), [24](24-task-derived-branch.jpg), [25](25-narrow-workspace-card.jpg): window resize retains split assignments, readable task form and cards. |

## Attempts and limits

- Attempt 09 did not open Transcript because input had lost focus; 10 is the successful capture. Later native coordinate control returned `Computer Use server error -10005: noWindowsAvailable`, while AX reads remained available. Reconnecting, Raise and a control-session reset did not restore input. Closed the owned window via its accessibility close control, received launcher exit 0, then retried after Kyle confirmed the Mac was unlocked and available. The fresh fixture completed the remaining interaction checks.
- Attempt 20 was still History: Escape returned to Browse, so pasted shell text was rejected. It does not prove ED3. Explicitly entering Terminal before the command produced successful captures 22–23.
- No clipboard-delivery, IME/dead-key, Linux, real-provider resume/restoration, fifty-session native tour or hosted CI claim. Waiting-Input/i-cycle and unsupported-provider fallback permutations remain automated evidence rather than claims from this bounded native tour. Copy selection, padding and wheel increments have automated regression evidence. The walkthrough uses a small synthetic conversation, not large-file or stale-reference race evidence.

## Ownership and cleanup

First fixture root/socket/private HOME and 17 owned groups are recorded in [metadata.json](metadata.json), [owned-processes-live.txt](owned-processes-live.txt) and [cleanup.json](cleanup.json). Retry ownership and 16 groups are in [retry-metadata.json](retry-metadata.json), [retry-owned-processes-final.txt](retry-owned-processes-final.txt) and [retry-cleanup.json](retry-cleanup.json). Both windows closed through CUA, both original launchers exited 0, GUI/server PIDs and every recorded group were gone (`ProcessLookupError`), and root/socket/private HOME were absent. Synthetic source SHA-256 stayed unchanged across each walkthrough; local copies are retained here. Temporary provider source files outside those roots are retained only as recovery evidence under `/private/tmp/ovrcr-ux-resume/native/`.

Readable AX/log text has line-end padding and excess final blank lines removed for repository whitespace checks. Exact original bytes are preserved in [the raw evidence archive](../verification-2026-11-05/raw-evidence.tar.gz), with SHA-256 entries in [the manifest](../verification-2026-11-05/raw-evidence-manifest.json). No failed attempt or test result was replaced.
