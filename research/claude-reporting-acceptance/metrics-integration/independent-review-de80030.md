# Independent integration review

Reviewed exact commit `de80030b5a163882fdd0acf698c01bdacec36f63` against parent and current helper contract. Read-only source review; no source/test edits or rerun claims. Reviewed source diffs, real callers, and saved tests/evidence.

## Blocking finding

P2 — `src/report/admission.rs:581`: native completion can finish draining on a pre-exit collector response. Resetting `collector_caught_up` only changes receiver state. `CollectorController::advance` consumes an in-flight response before sending another Read. If the helper already scanned EOF at A and its response remains pending, then the native process appends B and exits, the first completion poll consumes A with `caught_up=true`, exits the loop, cancels the helper and finalizes without ever scanning B. This can discard available final records despite remaining drain time. Partial coverage avoids a false completeness claim but does not satisfy the assigned exit-before-catch-up drain behavior.

Require at least one scan requested after native-completion observation before using caught_up to terminate the drain, retaining the original absolute deadline and partial coverage. Add a deterministic ordering test through the receiver/native completion path: pending pre-exit EOF response, final append, completion, final metrics include the appended record. Existing lifecycle tests finalize the already observed 10-token sample (`tests/server_lifecycle.rs:8723`) and do not force this ordering.

## Other inspected contracts

- Statusline RawValue envelope retains the original decimal lexeme; default and external renderer failure independence is exercised by real CLI tests, including timeout and invalid/oversize bytes.
- Root receiver retains untouched metric components. Runtime measurement watermarks preserve their ages; replay suppression and context-age finalization assertions are present. Uncertain source freshness is explicit.
- Native supervision uses the original two-second exit-observation deadline, bounds thread joining, and preserves actual exit status in the stalled-callback fixture.
- Finalize recovery queries the original operation receipt on a fresh bounded connection; the proxy test verifies that matching receipt was observed.
- Collector health can degrade independently of working activity/context; missing-file recovery uses the pinned admitted path. Clear cancels collection and freezes admission.

No additional blocker found in these reviewed paths. This is not native provider/Linux/50-session acceptance, nor a PASS while the final-drain finding remains unresolved.


## Targeted correction review

Independent reviewer accepted05c38516e9ac03415b64fa75d3354008d4339bb2: the second consumed response requires a request after NativeCompleted through the one-in-flight controller. The forced real-helper/native-exit/socket-Finalize regression fails10 versus30 on the old path and passes30 after correction, preserving native exit17 and the existing deadline. No remaining blocker found; saved evidence reviewed without rerunning tests.
