# Integrated explicit UUID resume acceptance

Tested 2026-09-10 at independently reviewed product commit
`958527167fc4d6f9e36502ece593d6bdd67e86c4`. `source.txt` and the empty
`tracked-status-prebuild.txt` were recorded before `CARGO_INCREMENTAL=0 just gui`.
The actual bundle launched from this checkout. Each native invocation checked
`2.1.267 (Claude Code)` through a fixture-local `claude` symlink to the retained
executable. The default 2.1.268 executable was not used or changed.

The fixture forwarded original synchronous hook/status-line bytes to the bundled
reporter. `capture.py` retains only identity/field names and numeric measurements;
credentials were neither logged nor saved. Settings and trust applied only within
the disposable fixture. The native launcher ran directly in the runtime PTY.

## Observations

| Check | Retained result |
| --- | --- |
| Seed session 11 | Actual output `OVRCR_RESUME_SEED_OK`; root UUID `8919f4f9-3c01-4152-95b3-31cda4e4fe2e`; one recognized assistant row. |
| Resume session 12 | Exact separate `--resume UUID`, no injected `--session-id`; matching root `SessionStart(source=resume)` and exact transcript path. New invocation, generation 1, same conversation. |
| Before resumed input | The same source device/inode and old message/request identity were retained. OVRCR imported 41,248 input / 19 output, with 30,339 cache-read / 10,907 cache-write tokens. Activity was unknown, not inferred from restored history. |
| After resumed input | Actual output `OVRCR_RESUME_HISTORY_cobalt`; a distinct second row contributed 46,309 input / 23 output. OVRCR totals were 87,557 input / 42 output, including 65,372 cache-read / 22,181 cache-write. |
| Labels and exit | Idle / Observed after the turn; Partial / Conversation throughout. Cost remained an independent estimate. Exit retained totals with `incomplete_final_accounting`; both terminal exit codes were 0. |
| Foreground ownership | Seed native PID/PGID/TPGID 3527; resume native PID/PGID/TPGID 11412. Supervisor and collector ownership were captured before exit. |
| Cleanup | GUI launcher exit 0; all 21 recorded PIDs and 18 PGIDs absent; fixture root and socket removed. No query of the closed app was made. |

Screenshots and full accessibility captures show the actual response markers and
reporting labels. The native exit frame retains some previous terminal text behind
its resume hint; this run does not certify a clean native exit repaint. The initial
fresh startup briefly reported `source_unavailable` before its first transcript
existed, then recovered on the first turn.

`snapshot.py` records every parseable assistant row from each captured exact source,
without filtering by identity or sidechain. It skips malformed JSON lines, so this
is not a malformed-line audit or proof of full accounting. The two recognized rows
and their unchanged identities/numbers prove the tested history import and addition.
This does not certify child/auxiliary categories, conflicting corrections, a final
source boundary, alternate resume forms, continue, or in-process rebinding.

Accessibility files have trailing line padding removed, with final newline retained.
JPEG screenshots are unchanged tool bytes. JSON measurements and identity values
are unchanged; provider prompt/response content is retained only in the intended
native GUI evidence.
