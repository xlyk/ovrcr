# Final verification record

Implementation reviewed through **56b84f9**. [Full independent review](final-review.md) found one P2: narrow Ready could hide unavailable health. [Scoped rereview](final-rereview.md) approved the renderer/docs correction. Earlier unit findings and failed attempts remain under sibling Task1–3 directories.

## Automated and platform results

[CI run34570470022](https://github.com/xlyk/ovrcr/actions/runs/34570470022), reviewed source56b84f9: **all four jobs passed**. Full log and capacity/memory artifacts are retained here.

| Environment | Result |
| --- | --- |
| Hosted macOS, all features | Format, all-target Clippy with warnings denied, workspace tests663passed/0failed, doc tests0cases |
| Hosted Linux, headless | Format, all-target Clippy, workspace tests637passed/0failed, doc tests0cases |
| Hosted Linux capacity | Existing isolated50-session shared/Claude reporting gate1passed; owned-process/accounting/latency artifacts retained |
| Hosted Linux memory | Existing isolated collector high-water gate1passed; provenance/memory artifacts retained |
| Local macOS focused | Task1 protocol23/TUI50/socket1/CLI2; Task2 managed CLI/PTY4, CLI4, bounds3, Claude2, probe2, native signals1; Task3 setup10/CLI32/config1; precise later correction tests retained in reports |
| Local full attempts | **Failed**, separately retained below; no local full-suite pass claimed |
| Local Clippy/doc tests | All-target/all-feature Clippy passed before the final rendering correction; hosted Clippy passed that correction. Local workspace doc tests passed with0cases. Final formatting/diff checks recorded at delivery. |

The14 ignored tests in each normal hosted workspace run are explicit helper/manual acceptance cases, not silently skipped failing feature assertions. The two required hosted load gates execute separately. Capacity/memory cover the reused foundation, not50 simultaneous native Codex cold starts or Codex metrics. Linux verifies actual CLI/PTY/socket paths with deterministic native fixtures, not an authenticated native Codex GUI.

Local full attempt1 atc3b8faf failed three setup startup fixtures (7passed/3failed). Test-resource coordination correction2db1897 preserved every assertion and the one-second production deadline; setup10/10 then passed with default test threading. Attempt2 failed existing probe cleanup version classification (38passed/1failed); b661988 uses the existing system shell through the same production probe/classifier, preserving five version/exit/group-cleanup cases; library39/39 allfeatures and30/30 default passed. Exact failing syscall was not captured, so host latency is not asserted as the individual proven cause.

Attempt3 at56b84f9 reached server_lifecycle and failed62passed/13failed/9ignored: native markers/startup/admission/collector timing and one subsequent poisoned-lock assertion. The host load snapshot and complete log are retained. A doc-test and GUI build overlapped part of this local run; no claim of an isolated host is made. No more fixture or production changes were made for these failures. The same revision subsequently passed full macOS and Linux suites on isolated hosted runners. Those hosted results establish the platform gates; they do not turn the local run into a pass.

## Native acceptance and scope

[Final actual GUI acceptance](../native-final/README.md) covers generated setup, real response markers, background Ready, reconnect, child isolation, next-prompt backtrack rebinding, split narrowing, native exit and owned cleanup. [Task2 native interruption](../native-task-2/README.md) covers Ctrl-C→Idle on unchanged hook code. Failed clipboard/capture attempts are explicit. No provider credential, live trust change, terminal-pattern telemetry, transcript collector, token/cost feature, Confirmed completion, notification or unread subsystem is included.

## Delivery procedure

The final documentation/evidence revision must pass all current-head PR checks before delivery. Its run/SHA are recorded in the PR and work diary, avoiding a self-referential evidence commit. Source56b84f9 acceptance remains pinned above. No merge or release is authorized.

Rulings retained: coordinator alone commits; evidence retained despite skill cleanup default; startup fixtures coordinated without relaxing deadlines/assertions; probe cleanup uses a system interpreter rather than a cold temporary executable. No concurrent provider-launch throughput claim follows from these choices.
