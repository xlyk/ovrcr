# #363 development receipts

All commands were prefixed with `rtk proxy` in the isolated
`codex/cursor-cloud-preflight` worktree. The observations below preserve outcomes
from the Codex session tool outputs; they are compact transcriptions, not raw logs.
Final committed-revision checks have separate raw log files.

| Attempt | Command | Executed / outcome | Meaning |
| --- | --- | --- | --- |
| 01 | `python3 -I -m unittest discover -s experiments/cursor-cloud -p test_client.py -v` | 1 / FAIL (1) | Bootstrap red: client absent, Python exit 2 rather than expected 1; no behavioral implementation proof |
| 02 | same | 1 / OK | Missing-input entry-point report implemented |
| 03 | same | 2 / FAIL (1) | Behavioral red: success fixture received `preflight is not yet implemented` |
| 04 | same | 2 / OK | Real synthetic bridge startup/catalog/cleanup path passed |
| 05 | same | 10 / FAIL (2 subcases) | Header/body stalls returned generic operation reason rather than deadline reason; both returned bounded |
| 06 | `python3 -I -m py_compile experiments/cursor-cloud/client.py experiments/cursor-cloud/test_client.py experiments/cursor-cloud/fake_bridge.py` | 3 files / exit 0 | Syntax check; no static type-checker configured |
| 07 | unittest discovery above | 12 / OK, 11.484 seconds | TimeoutError classified as deadline; error-body trickle, total deadline and replaced-token cases also passed |

| 08 | `python3 -I -m unittest discover -s experiments/cursor-cloud -p test_client.py -k interrupted_launch -v` | 1 / FAIL (1) | Behavioral red: interrupted external launch reported NOT STARTED rather than UNRESOLVED; corrected with explicit launch-attempt receipt and root retention |

| 09 | same interrupted-launch filter | 1 / ERROR (1) | Test assumed the peer had emitted its receipt before an interrupted launch; assert retained client-owned root independently of peer readiness |

| 10 | `python3 -I -m unittest discover -s experiments/cursor-cloud -p test_client.py -k malformed_discovery -v` | 1 / FAIL (3 subcases) | Both independent review axes found non-object discovery caused raw AttributeError/empty stdout; array, null and scalar reproduce before object validation |

No official bridge executable, real credential, real account, cloud agent,
repository publication or paid operation was used. One official darwin-arm64
release archive was downloaded for checksum/manifest inspection only; the digest
matched `56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb`.

## CI deadline correction

- Hosted run `40014086107`, exact `fe8f2063`: Rust nextest **2146 passed / 29 skipped**, **1 flaky** in 1446.129s. `terminal_acceptance::assignment_control::assigned_child_is_view_only_until_explicit_dashboard_takeover_and_return` failed attempt 1 and passed attempt 2 of a maximum 3. Offline Python: **14 tests / 18 failing subcases**, 23.347s, mostly 0.7-second work budget exhausted during synthetic startup. The error report in `repo_mismatch` also contained a truncated raw traceback. Subsequent extension/installed-Pi steps skipped; evidence upload failed after no files were produced.
- Exact original `fe8f2063` under deterministic normal-entry expired-budget regressions: **2 tests, 1 failure** (expired entry leaked a raw traceback); exhausted cleanup already retained uncertainty. Corrected client: **2 passed**. Raw red/green logs bind each client SHA-256.
- Correction: each timer is scoped to its blocking phase and disarmed before the outer error/report boundary; already-expired arms fail synchronously. Production 840/900-second deadlines are unchanged. Synthetic startup uses a realistic bounded 3/5-second budget; stalled peers prove the intended phase was reached and trickle beyond that same absolute deadline. Exhausted-entry/cleanup coverage remains separate. No assertion was removed.
- Corrected full feature suite: **16 passed**, 37.569s (37.620s wrapper); syntax **3 files passed**; diff whitespace passed. Both applicable CI jobs run the independent offline step before Rust Test, retaining every existing check/condition. New candidate awaits independent review before push.
- The separate serial all-feature Rust diagnostic remains tied to its original `fe8f2063` launcher, not the Python-only correction; all original parallel failures/resources remain retained.

## Synthetic startup DNS correction

- Hosted run `40020463574`, exact `1942360e`: offline Python **16 tests / 21 failing subcases**, 80.335s, repeatedly expiring at startup despite the unchanged 3-second work budget. Format/Lint passed; subsequent Rust/Doc/relocated-assets/extension/installed-Pi checks skipped, and upload lacked files. Original hosted logs remain separate from local results.
- Offline inspection shows `HTTPServer.server_bind` binds TCP, then calls `socket.getfqdn` before returning. Through normal client `main` and the real synthetic peer, controlled resolver delay/error produce **1 test / 2 failing subcases**: the delay expires at startup (3.015s report elapsed), while the error exits before ready. Both audit a lookup of `127.0.0.1` after token creation. This proves an unnecessary fixture dependency; actual hosted resolver latency was not directly measured.
- The synthetic loopback subclass retains `TCPServer.server_bind`, the actual ephemeral port and real socket/process/token behavior, but records the bound numeric host without reverse DNS. The same controlled resolver cases then **pass 1 test / 2 subcases**, 0.606s (0.657s wrapper), with no lookup and confirmed process/token cleanup.
- Full affected suite: **17 passed**, 38.323s (38.374s wrapper); Python syntax **3 files passed**; diff whitespace passed. Source-bound raw red/green/full/syntax receipts live under `checks/dns-startup-correction/` in the implementation worktree. Neither test budgets nor production 840/900-second deadlines changed; every existing assertion remains. No Rust rerun was started.
- The original serial Rust diagnostic is **INCOMPLETE**, not green: no original wait/exit receipt survived. The coordinator verified and terminated its exact orphaned task-owned group after the original bound and observed no remaining group members; supervisor loss remains unexplained. Partial GUI **9/9** and all earlier failures/resources are preserved separately. No directory or detached-descendant cleanup is inferred.
