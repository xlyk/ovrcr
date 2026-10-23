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

No official bridge executable, real credential, real account, cloud agent,
repository publication or paid operation was used. One official darwin-arm64
release archive was downloaded for checksum/manifest inspection only; the digest
matched `56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb`.
