reviewer_model: unknown

| test_id | C/S/U/F | recommendation | verdict | one-line reason |
|---|---|---|---|---|
| `nextest\|Test\|ovrcr-protocol\|settings%3A%3Atests%3A%3Atheme_appearance_metadata_groups_every_existing_theme\|default` | 1/2/0/3 | consolidate | AGREE | 1. The retained manifest test compares appearance for every theme, including these eight (settings.rs:2010-2043). |
| `node-test\|Extension%20tests\|tests%2Fpi_reporting_extension.mjs\|fixture%20result%20receipt%20appears%20only%20after%20its%20complete%20JSON%20is%20written\|default` | 1/3/1/0 | keep | AGREE | 2. The preload checks receipt invisibility before bytes are written and final JSON content; F=0 correctly limits this to the fixture. |
| `node-test\|Extension%20tests\|tests%2Fpi_workflow_tracker.mjs\|the%20bounded%20tracker%20stub%20exposes%20duplicate%20attempts%20and%20refuses%20parent%20modification\|default` | 1/3/1/0 | keep | AGREE | 3. Success, duplicate refusal, parent immutability and ledger counts are asserted against the explicitly bounded stub. |
| `nextest\|Test\|ovrcr-tui\|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_confirmed_without_unread_does_not_queue\|default` | 2/1/0/3 | consolidate | AGREE | 4. Both bodies assert an empty queue with unread=None; unread.rs:23-26 exits before quality matters. |
| `nextest\|Test\|ovrcr%3A%3Aclient_startup_server\|accepted_restart_stops_older_protocol_server_and_starts_this_build\|default` | 4/4/4/4 | keep | AGREE | 5. Client restart exercises real peer PID, SIGTERM, socket removal and replacement; the older peer is a controlled subprocess fixture. |
| `nextest\|Test\|ovrcr%3A%3Aclient_startup_server\|changed_identity_during_consent_does_not_shutdown_server\|default` | 4/4/4/4 | keep | AGREE | 6. The callback changes captured identity; the error and subsequent List jointly protect the destructive consent boundary. |
| `nextest\|Test\|ovrcr%3A%3Aclient_startup_server\|server_refuses_unrecognized_existing_identity_without_replacing_user_file\|default` | 4/4/4/4 | keep | AGREE | 7. Nonzero exit, preserved exact bytes and removed socket protect real user-file preservation. |
| `nextest\|Test\|ovrcr%3A%3Aclient_startup_server\|socket_replacement_during_consent_never_receives_shutdown\|default` | 4/4/4/4 | keep | AGREE | 8. Replacement listener receives no connection while the original still answers List; wrong-target C=4 is justified. |
| `node-test\|Extension%20tests\|tests%2Fpi_workflow_tracker.mjs\|lost%20create%20reply%20is%20attributable%20through%20ordinary%20issue%20list%20without%20a%20duplicate%20publication\|default` | 1/3/2/0 | keep | DISAGREE | 9. F=0 and U=2 require improve under the ordered policy; fixture qualification is not an exception. |
| `node-test\|Extension%20tests\|tests%2Fpi_reporting_extension.mjs\|reporting%20fixture%20publishes%20one%20complete%20EOF-terminated%20Unicode%20frame%20before%20interruption\|default` | 1/4/1/1 | keep | DISAGREE | 10. Framing and payload are checked, but interruption happens after the complete write; split-write detection is overstated. |
| `node-test\|Extension%20tests\|tests%2Fpi_reporting_extension.mjs\|reports%20the%20current%20model%20from%20getModel%20and%20model_select\|default` | 1/3/2/3 | keep | AGREE | 11. Exact startup and changed-model frames plus provider exclusion exercise real Pi handlers with an appropriate context fixture. |
| `python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|client-extra-argv%3Aln56\|local-development` | 2/4/0/4 | consolidate | AGREE | 12. The extra-argv guard precedes either profile branch; the retained production invocation runs the same statement. |
| `python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-argv%5B%5B%27--check-contract%27%2C%20%27-1%27%2C%20str%28wire%29%5D%5D%3Aln53\|local-development` | 2/4/0/4 | consolidate | AGREE | 13. Negative UInt32 argument rejection precedes profile selection and matches the retained production invocation. |
| `python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-argv%5B%5B%27--check-contract%27%2C%20str%28schema%29%2C%20%274294967296%27%5D%5D%3Aln53\|local-development` | 2/4/0/4 | consolidate | AGREE | 14. Overflowing UInt32 argument rejection precedes profile selection and matches the retained production invocation. |
| `nextest\|Test\|ovrcr%3A%3Abin%2Fovrcr-gui\|terminal_fonts_render_mockup_glyphs_or_default_symbols\|default` | 1/1/2/3 | improve | AGREE | 15. The environment-dependent has_glyphs check leaves style selection unpinned; improve follows S=1 and U=2. |
| `nextest\|Test\|ovrcr%3A%3Acli\|dashboard_selection_frames_skip_only_quota_telemetry\|default` | 1/2/2/1 | improve | AGREE | 16. Exact frame sequence qualifies test-local filtering, not a server attach; F=1 and U=2 require improve. |
| `nextest\|Test\|ovrcr%3A%3Acli\|quota_filter_preserves_other_events_and_responses\|default` | 1/2/2/1 | improve | AGREE | 17. Exact non-quota pass-through qualifies the same local helper; F=1 and U=2 require improve. |
| `nextest\|Test\|ovrcr-runtime\|settings%3A%3Atests%3A%3Aquota_in_instance_identity_is_a_finding_and_configures_nothing\|default` | 2/4/0/4 | consolidate | DISAGREE | 18. The counterpart compares report.rows, not effective report.settings.quota; complete subsumption and U=0 are unsupported. |
| `nextest\|Test\|ovrcr-runtime\|server%3A%3Aquota%3A%3Atests%3A%3Anative_client_is_not_gated_on_the_harness_version\|default` | 2/4/1/3 | consolidate | AGREE | 19. Both tests forbid a version probe and assert app-server argv; extra version responses are never consumed. |
| `nextest\|Test\|ovrcr-protocol\|auto_trust%3A%3Atests%3A%3Abare_managed_launches_gain_one_provider_flag\|default` | 3/3/1/3 | consolidate | AGREE | 20. The retained CLI case asserts all four exact bare-launch flags through the production caller. |
| `nextest\|Test\|ovrcr%3A%3Aprovider_quota\|unknown_method_or_unrecognized_reply_not_a_version_is_unsupported\|default` | 2/3/3/3 | keep | AGREE | 21. Two controlled RPC failures require exact Unsupported reasons, no windows/account leak, and the long retry deadline. |
| `python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|client-input%5Bb%27%5B%5D%27%5D%3Aln57\|local-development` | 2/4/1/4 | keep | DISAGREE | 22. Failed JSON output does not establish rejection before IPC; S=4 overstates this admission evidence. |
| `nextest\|Test\|ovrcr-runtime\|server%3A%3Atests%3A%3Aobsolete_dashboard_writer_cannot_revoke_replacement_or_its_quota_source\|default` | 3/4/3/4 | keep | AGREE | 23. A gated real writer races a replacement owner; pointer identity, view, source and subsequent quota delivery are checked. |
| `node-test\|Extension%20tests\|tests%2Fomp_reporting_extension.mjs\|error%20and%20aborted%20ends%20settle%20as%20error%20and%20aborted%3B%20no%20assistant%20message%20is%20none\|default` | 2/4/2/3 | keep | DISAGREE | 24. Exact outcome/cycle frames are asserted, but each input has at most one assistant; first-vs-last selection is untested. |

## Selection and evidence boundary

All CSV content came from `review/frozen/tests.csv`, SHA-256 `d0566038af5be661826c7263c369b09f9052044b47022ad2e13be7cd38b3d5c1`. The runtime exposed no model/thinking-level environment values; no thinking level was observable, so the header is unknown.

Rows 1–4 are the lowest retention scores and rows 5–8 the highest. Ties use ascending full test_id. Rows 9–11 are the lowest-scoring previously unselected Node/Cargo rows; 12–14 the lowest-scoring previously unselected Python rows; 15–17 the lowest-scoring previously unselected improve rows. Rows 18–20 are the three cross-file consolidate rows, ordered by retention_score then test_id. Rows 21–24 have the smallest SHA-256 hex digests of full UTF-8 test_id, computed directly with hashlib. These four are also the global four smallest, without any exclusion adjustment. All 24 IDs are distinct. The three dedicated consolidation selections are additional to consolidate rows selected by the other strata.

| sample row | sha256(test_id) |
|---|---|
| 21 | `00352949fa2b2ebce72b4d8e8d909d8e6186ca860f580c5bc330eef91a7afe8c` |
| 22 | `00466b013d338b8a129abae531654dfde3b49125c906efbc4b437bd482829389` |
| 23 | `0068e0ede84dd7271fe242de044e3e5075428c48f4684cc1874922bbe3a2064f` |
| 24 | `006d4d88838086b4f65f5741c6a45eb5e9d5826d3a66f36b452e694dddb2a867` |

Each sampled defining body and its cited production ranges were read. Assertion helpers, relevant fixture behavior and retained counterparts were read where needed. No product test, fixture, build, formatter or validator was executed. This is a source review, not a measured mutation test.

## Disagreements

### Row 9 — CONCERN: the recommendation contradicts its own scores

`tests/pi_workflow_tracker.mjs:21-31` drives the fake tracker and checks its timeout, one matching issue, URL, blocker and ledger. `tests/fixtures/pi/workflow-gh.mjs:15-39` implements that stand-in; production tracker behavior is not exercised. The CSV acknowledges F=0 and assigns U=2, yet says C=1 prevents the improve gate. The gate is `(S<=1 or F<=1) and (C>=2 or U>=2)`, not a requirement for C>=2. There is no proven equivalent retained coverage here.

Proposed **1/3/2/0, improve**. Keep the fixture-only scope explicit; the ordered policy requires the row to name the missing production path rather than invent an infrastructure exception (`evidence/rubric.md:47-52`). This changes prioritization, not a pruning decision.

### Row 10 — NIT: the claimed split-write defect is not tested

`tests/pi_reporting_extension.mjs:68-73` puts the checkpoint and SIGSTOP after the shell has appended the complete line. `tests/pi_reporting_extension.mjs:451-457` waits for that checkpoint before SIGKILL. Consequently the test checks final newline framing and payload preservation, but does not interrupt an incomplete write. Two writes that both finish before the checkpoint are indistinguishable from one write. The assertion never drives the production transport at `src/ovrcr-reporting-transport.mjs:82-102`.

Proposed **1/4/1/1, keep**, unchanged. Narrow failure_caught to missing newline, extra/truncated final frames, or Unicode/escape corruption after the fixture checkpoint. Remove the claim that it detects splitting the body across writes.

### Row 18 — BLOCKER: the named retained test does not assert the effective quota settings

`crates/ovrcr-runtime/src/settings/mod.rs:1528-1544` writes a legacy identity containing only `[quota] enabled = true`, then directly compares `report.settings.quota` with defaults, asserts enabled, and checks one keyed/positioned finding. The retained `crates/ovrcr-runtime/tests/settings_matrix.rs:782-802` uses a different legacy body with an additional `quota.codex.command` and compares `report.rows`; it never asserts `report.settings.quota`. Its default oracle is `read("").rows` (`settings_matrix.rs:285-287,741-743`).

These are separate returned fields: `parse` builds rows and stores settings independently at `crates/ovrcr-runtime/src/settings/mod.rs:131-141`, before `load_document` examines the legacy identity at `mod.rs:75-88`. A change in that latter stage that alters effective settings without updating rows is directly rejected by the unit assertion, while the cited matrix assertions leave that value unobserved. The matrix therefore preserves the finding/row projection, not every assertion made by the proposed removal. Also, enabled=true equals the default, so neither this input nor the CSV wording proves that copying that boolean from the legacy file would be detected.

Proposed **2/4/2/4, keep**. Preserve the exact finding/state assertions, narrow failure_caught to what they reject, and remove the complete-subsumption claim. This changes a consolidation decision; U=0 is not defensible under the requested same-assertion/input/path rule.

### Row 22 — CONCERN: output alone does not prove the early admission boundary

`native/bridge/check-guards.py:38-46,57-58` feeds `[]` and checks exit 0, empty stderr, one newline and exactly the failed reply. It observes neither launch nor IPC. The intended rejection is `native/bridge/Contract.swift:92-101`, called at `native/bridge/main.swift:43-44`; later failures also emit `.failed` (`main.swift:45-53`). The row claims malformed stdin accepted far enough to reach IPC would be caught, but that transition is not distinguished from a later failed result.

Proposed **2/3/1/4, keep**. The real compiled entry and real stdin justify F=4, and the malformed-input result is useful failure coverage. S=4 is too strong for the claimed pre-IPC protection. Restrict failure_caught to the observable failure-response contract, or add explicit evidence that launch/IPC was untouched before making the stronger claim.

### Row 24 — NIT: first-assistant versus last-assistant selection is not exercised

`tests/omp_reporting_extension.mjs:383-386` supplies one assistant marked error, one assistant marked aborted, and an empty list. None contains two assistant messages. `src/ovrcr-reporting-transport.mjs:8-17` searches backward, but searching forward would produce the same expected frames here. `src/omp-reporting-extension.mjs:59-69` and `tests/omp_reporting_extension.mjs:391-396` do support the stated end/settled classification, cycle numbering and prompt exclusion.

Proposed **2/4/2/3, keep**, unchanged. Remove “read the first assistant message” from failure_caught; retain the missing/misclassified outcome defect. The exact three-cycle frame invariant remains defensible at S=4.

## The three dedicated consolidation claims

### Row 18 — REFUTED

Candidate `settings::tests::quota_in_instance_identity_is_a_finding_and_configures_nothing` (`crates/ovrcr-runtime/src/settings/mod.rs:1525-1545`) names retained `quota_in_instance_identity_is_one_finding_and_configures_nothing` (`crates/ovrcr-runtime/tests/settings_matrix.rs:778-803`). Both exercise `load_document` and migration diagnostics, but the retained case checks rows rather than effective quota settings and changes the legacy input. The BLOCKER above gives the missing protection.

### Row 19 — CONFIRMED

Candidate `server::quota::tests::native_client_is_not_gated_on_the_harness_version` (`crates/ovrcr-runtime/src/server/quota.rs:2194-2221`) names retained `codex_native_read_reaches_remaining_bars_through_real_server_and_socket` (`tests/provider_quota.rs:141-249`). Both run production `native_client` (`quota.rs:969-1002`), require the read-only app-server argv, forbid a version probe, and require correctly parsed quota. The candidate’s shell advertises three version outputs only under `--version` (`quota.rs:2171-2181`); the retained shell advertises an old version under the same flag (`provider_quota.rs:148`). Since both assert that flag was never sent, these version strings provide no executed boundary. The retained test additionally observes real server/socket delivery and exact remaining values. The same no-version-gate contract is preserved despite different rate-limit fixture values.

### Row 20 — CONFIRMED

Candidate `auto_trust::tests::bare_managed_launches_gain_one_provider_flag` (`crates/ovrcr-protocol/src/auto_trust.rs:81-93`) names retained `agent_run_adds_default_auto_trust_only_when_the_permission_family_is_absent` (`tests/cli.rs:2164-2236`). The candidate checks the exact four bare argv results. The retained executable prints every received argument (`cli.rs:2166-2196`) and asserts the same provider flags at `cli.rs:2200-2228`, including no duplicates/extra arguments through exact vectors. `auto_trust.rs:54-69` is the production insertion path. The C=3 managed-launch permission invariant is preserved by the retained real caller, without claiming native provider approval acceptance.

The five additional consolidate rows selected by other strata were also compared with their retained counterparts: row 1 against `settings.rs:1995-2068`; row 4 against `desktop.rs:2072-2081`; and rows 12–14 against their identical production-mode `check-guards.py:53-56` invocations. Those claims are confirmed: the first covers the same theme appearances; the second hits the same unread=None early return; the bridge argv checks run before any profile-dependent code (`main.swift:14-27`).

## Totals

**19 AGREE; 5 DISAGREE**: 1 BLOCKER, 2 CONCERN, 2 NIT. The three dedicated consolidation claims: **2 confirmed, 1 refuted**. Across all eight sampled consolidate rows: **7 confirmed, 1 refuted**.
