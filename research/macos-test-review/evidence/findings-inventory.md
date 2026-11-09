# Findings inventory (all review shards)

Extraction and grouping from the existing grounded reviews and independent receipts. No test was re-reviewed; no `reviews/*.jsonl` or other evidence file was modified.

## Snapshot

- Review shards: 50; review rows: 3171; unique test ids: 3171.
- Receipts: 633 rows across 15 `quality-review-*.jsonl` files at extraction time.
- Inventory: 3171 rows; every review test_id resolves in `evidence/inventory.csv` (checked programmatically).
- Combined sha256 of `reviews/*.jsonl` (sorted `file:sha256` lines): `1a1cfda80c2aa02d716443ac110ff1000563e8eae02f25cf63b1dae24640cb8d`.
- Combined sha256 of `quality-review-*.jsonl` at extraction: `88a458cfe49135cfbdd71e5a6832f81dfbc0678904cfe7312ae5c0eae8ddd0ab`.
- The author review files and receipts were being rewritten/appended while this pass ran; the two hashes above pin the exact snapshot used.

## Selection rules

1. **claim_mismatch** - rows whose own review record quotes a name/claim the body does not exercise, with the missing assertion named.
2. **vacuous_or_weak** - every reviewed row with author signal S<=1, with author and checker verdicts.
3. **fixture_fidelity** - every row with author F<=2 whose record states a stand-in/helper/plumbing limit, plus F=3 rows whose record declares a real boundary replaced or out of scope where the claim depends on it.
4. **proposal_rejected** - every prune/consolidate proposal an independent receipt resolved to keep.
5. **proposal_confirmed** - every prune/consolidate proposal an independent receipt agreed with, naming the retained test.
6. **product_observation** - incidental product concerns surfaced by the reviews/receipts, explicitly not test-value claims.

Categories may overlap: entries vs unique tests and multi-category rows are listed under Counts.

## claim_mismatch (11)

- `nextest|Test|ovrcr%3A%3Api_delegation|codex%3A%3Acodex_custom_backend_and_unenforceable_limits_refuse_without_a_child|default`
  - Name claims refusal without a child; review signal_reason: "Both assertions are disjunctions accepting an error or a null/absent field ... so a refusal-with-allocation or a silently defaulted limit could escape; assertion at tests/pi_delegation/codex.rs:905-906". Missing assertion: an exact error and unchanged allocation count.
  - evidence: `tests/pi_delegation/codex.rs:905-906`
- `nextest|Test|ovrcr%3A%3Atask_tui|unicode_prompt_render_is_safe_while_another_field_has_focus|default`
  - Name claims a unicode prompt render check; review signal_reason: "The body only calls draw_tasks once: it asserts no panic and nothing about the rendered text, cursor or field layout (tests/task_tui.rs:623-630)". Missing assertion: any rendered text, cursor or field-layout expectation.
  - evidence: `tests/task_tui.rs:623-630`
- `nextest|Test|ovrcr%3A%3Atui|mouse%3A%3Aterminal_mode_keeps_dashboard_mouse_capture_without_a_ready_session|default`
  - Name claims Terminal-mode mouse capture; review fidelity_reason: "The fixture never enters Terminal mode (it has no session), so the claimed state is replaced by a Browse-mode click on an empty dashboard"; signal_reason: "Only assert_ne against PtyBytes(Vec::new()) passes for almost any action, including non-empty PtyBytes or Detach".
  - evidence: `tests/tui/mouse.rs:434-451`
- `nextest|Test|ovrcr-protocol|operator%3A%3Atests%3A%3Aoperator_requests_and_worker_batches_round_trip_framed|default`
  - Name promises worker batches are framed; review recommendation_reason: "the body does not actually frame worker batches, so the name overstates it". The body frames only Request::Operator(OperatorRequest::Create). Missing assertion: an OperatorEventBatch round trip.
  - evidence: `crates/ovrcr-protocol/src/operator.rs:511-538`
- `nextest|Test|ovrcr-runtime|retained%3A%3Aoperator_schema_tests%3A%3Aregistry_migration_adds_operator_journal_to_existing_owner|default`
  - Name claims a migration onto an existing owner; review signal_reason: "the claimed migration to an existing owner (an older registry without the tables) is not exercised, so an upgrade that fails to add them would pass". Missing assertion: table creation from an older schema.
  - evidence: `crates/ovrcr-runtime/src/retained.rs:2129-2139`
- `nextest|Test|ovrcr-runtime|server%3A%3Adashboard%3A%3Atests%3A%3Aforget_session_drops_history_and_focused_view|default`
  - Name claims history is dropped; review fidelity_reason: "slot.history stays None, so the history-dropping branch at crates/ovrcr-runtime/src/server/dashboard.rs:376-385 is not exercised despite the test name". Missing assertion: a slot with installed history whose removal clears it.
  - evidence: `crates/ovrcr-runtime/src/server/dashboard.rs:376-385`
- `nextest|Test|ovrcr-runtime|server%3A%3Adefault_session_label_tests%3A%3Adirect_agent_executable_matches_kind_when_label_omitted|default`
  - Name claims the argv-based label resolution; review signal_reason: "asserts a string that equals the result under either the kind or the argv implementation, so it cannot reject the defect the sibling fixtures exist for". Missing assertion: a case where kind- and argv-derived labels differ.
  - evidence: `crates/ovrcr-runtime/src/server/mod.rs:3251-3262`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_confirmed_without_unread_does_not_queue|default`
  - Name claims Confirmed quality never queues; review signal_reason: "The fixture sets unread=None, which alone blocks the queue ... so the asserted length cannot detect the named Confirmed-quality defect". Missing assertion: a Confirmed sample with unread present.
  - evidence: `crates/ovrcr-tui/src/dashboard/desktop.rs:2083-2097`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Aoperator%3A%3Atests%3A%3Aoperator_sidebar_rows_share_render_and_hit_geometry|default`
  - Name claims shared render and hit geometry; review signal_reason: "it performs no click, so the shared-geometry half of the name is unchecked". Missing assertion: a click on a rendered operator row mapping to the drawn row.
  - evidence: `crates/ovrcr-tui/src/dashboard/operator.rs:3218-3237`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Apalette%3A%3Amouse_tests%3A%3Amouse_form_focus_unicode_edit_and_cancel|default`
  - Name claims a unicode edit; review signal_reason: "The only assertion after the edits is that the palette closed; the field value and cursor are never checked, so wrong edits pass (palette.rs:9059-9071)". Missing assertion: the edited value and cursor.
  - evidence: `crates/ovrcr-tui/src/dashboard/palette.rs:9059-9071`
- `nextest|Test|ovrcr|remote%3A%3Ahttp%3A%3Atests%3A%3Aparses_a_body_and_reports_a_missing_content_length_as_zero|default`
  - Name claims body assembly; review signal_reason: "both bodies arrive in the same read as the head, so the 'while body.len() < content_length' loop and the exact MAX_BODY boundary are never exercised and an off-by-one there would pass". Missing assertion: a short read that forces the loop plus a MAX_BODY boundary case.
  - evidence: `src/remote/http.rs:313-319`

## vacuous_or_weak (6)

- `nextest|Test|ovrcr%3A%3Abin%2Fovrcr-gui|terminal_fonts_render_mockup_glyphs_or_default_symbols|default`
  - Author verdict improve (confidence medium); signal_reason: "The accepted symbol set changes with the developer's ~/Library/Fonts: without the nerd font install_terminal_fonts returns current_monospace where all four FontIds are the same monospace id (src/bin/ovrcr-gui.rs:34-48, 108-110), so the style mapping is unasser"; checker quality-review-shared.jsonl verdict agree.
  - evidence: `src/bin/ovrcr-gui.rs:158-186`
- `nextest|Test|ovrcr%3A%3Atask_tui|unicode_prompt_render_is_safe_while_another_field_has_focus|default`
  - Author verdict improve (confidence high); signal_reason: "The body only calls draw_tasks once: it asserts no panic and nothing about the rendered text, cursor or field layout (tests/task_tui.rs:623-630)."; no independent checker receipt.
  - evidence: `tests/task_tui.rs:623-630`
- `nextest|Test|ovrcr%3A%3Atui|mouse%3A%3Aterminal_mode_keeps_dashboard_mouse_capture_without_a_ready_session|default`
  - Author verdict improve (confidence high); signal_reason: "Only assert_ne against PtyBytes(Vec::new()) passes for almost any action, including non-empty PtyBytes or Detach, so most wrong outcomes escape."; no independent checker receipt.
  - evidence: `tests/tui/mouse.rs:434-451`
- `nextest|Test|ovrcr-runtime|server%3A%3Adefault_session_label_tests%3A%3Adirect_agent_executable_matches_kind_when_label_omitted|default`
  - Author verdict consolidate (confidence high); signal_reason: "crates/ovrcr-runtime/src/server/mod.rs:3251-3262 asserts a string that equals the result under either the kind or the argv implementation, so it cannot reject the defect the sibling fixtures exist for."; checker quality-review-runtime.jsonl verdict agree.
  - evidence: `crates/ovrcr-runtime/src/server/mod.rs:3251-3262`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_confirmed_without_unread_does_not_queue|default`
  - Author verdict consolidate (confidence high); signal_reason: "The fixture sets unread=None, which alone blocks the queue at crates/ovrcr-tui/src/dashboard/desktop.rs:642-660, so the asserted length cannot detect the named Confirmed-quality defect."; checker quality-review-tui.jsonl verdict agree.
  - evidence: `crates/ovrcr-tui/src/dashboard/desktop.rs:2083-2097`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Apalette%3A%3Amouse_tests%3A%3Amouse_form_focus_unicode_edit_and_cancel|default`
  - Author verdict improve (confidence medium); signal_reason: "The only assertion after the edits is that the palette closed; the field value and cursor are never checked, so wrong edits pass (palette.rs:9059-9071)."; no independent checker receipt.
  - evidence: `crates/ovrcr-tui/src/dashboard/palette.rs:9059-9071`

## fixture_fidelity (25)

- `nextest|Test|ovrcr%3A%3Abin%2Fovrcr-gui|startup_offers_are_recognised_only_while_waiting_for_their_answer|default`
  - F=1; limit stated by the record: "The subject (src/bin/ovrcr-gui.rs:928-960) is a #[cfg(test)]-only helper fed three hand-written screen strings at src/bin/ovrcr-gui.rs:963-982; no live vt100 screen is read and the sending half of the harness (src/bin/ovrcr-gui.rs:908-925) is not exercised."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `src/bin/ovrcr-gui.rs:963-982`
- `nextest|Test|ovrcr%3A%3Abin%2Fovrcr|cli%3A%3Astartup_bridge%3A%3Atests%3A%3Alocal_startup_plans_only_fixed_local_validation_and_ad_hoc_installation|default`
  - F=3; the record declares the real boundary replaced or out of scope: "Production run_at with a recording execute sink instead of real installer processes.".
  - evidence: `src/cli/startup_bridge.rs:779-837`
- `nextest|Test|ovrcr%3A%3Abin%2Fovrcr|cli%3A%3Astartup_bridge%3A%3Atests%3A%3Aproduction_default_empty_and_zero_keep_identity_discovery_and_installer_arguments|default`
  - F=3; the record declares the real boundary replaced or out of scope: "Production run_at with a recording execute sink.".
  - evidence: `src/cli/startup_bridge.rs:1030-1088`
- `nextest|Test|ovrcr%3A%3Acli|cli_fixture_commands_isolate_profiles_and_allow_explicit_overrides|default`
  - F=1; limit stated by the record: "It inspects a Command builder, not a product component, so the boundary is plumbing."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/cli.rs:39-75`
- `nextest|Test|ovrcr%3A%3Acli|dashboard_selection_frames_skip_only_quota_telemetry|default`
  - F=1; limit stated by the record: "The filter is test-local plumbing; no production component is driven."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/cli.rs:1425-1492`
- `nextest|Test|ovrcr%3A%3Acli|quota_filter_preserves_other_events_and_responses|default`
  - F=1; limit stated by the record: "Local helper over a socket pair; no product code runs."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/cli.rs:1495-1543`
- `nextest|Test|ovrcr%3A%3Aoperator_e2e|compiled_worker%3A%3Adiagnostic_real_worker_classifies_provider_failures_without_retaining_remote_secrets|default`
  - F=3; the record declares the real boundary replaced or out of scope: "Real compiled worker/PTY/sqlite; provider is a fake over loopback, so native acceptance is not claimed.".
  - evidence: `tests/operator_e2e.rs:568-737`
- `nextest|Test|ovrcr%3A%3Api_delegation|control_proxy_closed_peer_does_not_prevent_the_next_valid_frame|default`
  - F=2; limit stated by the record: "Harness helper configure_proxy_stream plus production read_frame is exercised, not the production publisher forward loop; the helper is a test-only stand-in; real boundary at tests/pi_delegation.rs:1059-1120."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/pi_delegation.rs:1122-1153`
- `nextest|Test|ovrcr%3A%3Api_delegation|control_proxy_post_handshake_disconnect_preserves_the_next_request|default`
  - F=2; limit stated by the record: "Real ControlProxy, real server socket, and a real client::request, but the proxy is test-only plumbing rather than the production CLI publisher; real boundary at tests/pi_delegation.rs:1240-1300."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/pi_delegation.rs:1192-1221`
- `nextest|Test|ovrcr%3A%3Api_delegation|control_proxy_preserves_queued_frames_and_rejects_incomplete_or_invalid_requests|default`
  - F=2; limit stated by the record: "Uses production read_frame plus the test-local read_proxy_message wrapper rather than the production publisher loop; real boundary at tests/pi_delegation.rs:1059-1120."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/pi_delegation.rs:1156-1189`
- `nextest|Test|ovrcr%3A%3Aserver_lifecycle|empty_dashboard_response_keeps_open_peer_reads_bounded|default`
  - F=2; limit stated by the record: "Real socket pair with an open peer; the helper deadline is the subject (tests/server_lifecycle.rs:6215-6221)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/server_lifecycle.rs:6214-6224`
- `nextest|Test|ovrcr%3A%3Aserver_lifecycle|empty_dashboard_response_preserves_complete_frame_after_peer_close|default`
  - F=2; limit stated by the record: "Real socket pair with the real protocol codec, but the asserted component is a test helper, not the dashboard path (tests/server_lifecycle.rs:6135-6145)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/server_lifecycle.rs:6178-6190`
- `nextest|Test|ovrcr%3A%3Aserver_lifecycle|empty_dashboard_response_rejects_closed_peer_without_a_complete_frame|default`
  - F=2; limit stated by the record: "Real socket pair and real codec, but the helper is the subject (tests/server_lifecycle.rs:6194-6200)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/server_lifecycle.rs:6193-6211`
- `nextest|Test|ovrcr%3A%3Aterminal_acceptance|acceptance_cli_uses_fixture_environment|default`
  - F=2; limit stated by the record: "Runs the fixture command with a stand-in executable; no OVRCR product code is exercised (tests/terminal_acceptance.rs:310-325)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/terminal_acceptance.rs:303-330`
- `nextest|Test|ovrcr%3A%3Aterminal_acceptance|outer_dashboard_reports_early_child_exit_with_fixture_environment|default`
  - F=2; limit stated by the record: "Real PTY child with a script standing in for the dashboard; the subject is the harness, not shipped behaviour (tests/terminal_acceptance.rs:340-360)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/terminal_acceptance.rs:333-371`
- `nextest|Test|ovrcr%3A%3Atui|bridge_navigation%3A%3Abridge_navigation_revalidates_local_target_after_server_confirmation|default`
  - F=3; the record declares the real boundary replaced or out of scope: "Real Dashboard handler and protocol messages with a pure context fixture; the real server digest/socket/PTY boundary is explicitly out of scope (tests/tui/bridge_navigation.rs:1-2).".
  - evidence: `tests/tui/bridge_navigation.rs:1-2`
- `nextest|Test|ovrcr%3A%3Atui|dashboard_reader_channel_applies_bounded_backpressure|default`
  - F=2; limit stated by the record: "Uses the production channel constructor and capacity constant, but the reader thread's blocking send is replaced by a direct try_send."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/tui/main.rs:584-603`
- `nextest|Test|ovrcr%3A%3Atui|mouse%3A%3Aterminal_mode_keeps_dashboard_mouse_capture_without_a_ready_session|default`
  - F=2; limit stated by the record: "The fixture never enters Terminal mode (it has no session), so the claimed state is replaced by a Browse-mode click on an empty dashboard."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/tui/mouse.rs:434-451`
- `nextest|Test|ovrcr-runtime%3A%3Asettings_matrix|generator_spells_near_misses|default`
  - F=1; limit stated by the record: "Tests a test-side helper; the production loader is only reached through the matrix tests it feeds."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `crates/ovrcr-runtime/tests/settings_matrix.rs:569-574`
- `nextest|Test|ovrcr-runtime|retained%3A%3Aoperator_schema_tests%3A%3Aregistry_migration_adds_operator_journal_to_existing_owner|default`
  - F=3; the record declares the real boundary replaced or out of scope: "Production SessionStore::open runs the real schema creation, but the upgrade boundary the name claims is replaced by a fresh database (F=3). Exercised production path: crates/ovrcr-runtime/src/retained/operator.rs:6-60.".
  - evidence: `crates/ovrcr-runtime/src/retained/operator.rs:6-60`
- `nextest|Test|ovrcr-runtime|server%3A%3Adashboard%3A%3Atests%3A%3Aforget_session_drops_history_and_focused_view|default`
  - F=2; limit stated by the record: "The test installs only the view; slot.history stays None, so the history-dropping branch at crates/ovrcr-runtime/src/server/dashboard.rs:376-385 is not exercised despite the test name."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `crates/ovrcr-runtime/src/server/dashboard.rs:734-760`
- `node-test|Extension%20tests|tests%2Fpi_reporting_extension.mjs|fixture%20result%20receipt%20appears%20only%20after%20its%20complete%20JSON%20is%20written|default`
  - F=0; limit stated by the record: "The subject is a stand-in: the fixture's own writer is instrumented with a mock fs module and a stubbed extension, so no production behaviour is exercised (tests/pi_reporting_extension.mjs:20-36)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/pi_reporting_extension.mjs:13-56`
- `node-test|Extension%20tests|tests%2Fpi_reporting_extension.mjs|reporting%20fixture%20publishes%20one%20complete%20EOF-terminated%20Unicode%20frame%20before%20interruption|default`
  - F=1; limit stated by the record: "This is a plumbing check of the fixture's own stub helper: the assertion never touches the extension or the transport (tests/pi_reporting_extension.mjs:446-448)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/pi_reporting_extension.mjs:445-459`
- `node-test|Extension%20tests|tests%2Fpi_workflow_tracker.mjs|lost%20create%20reply%20is%20attributable%20through%20ordinary%20issue%20list%20without%20a%20duplicate%20publication|default`
  - F=0; limit stated by the record: "The subject is a stand-in: the test drives tests/fixtures/pi/workflow-gh.mjs, an explicit 'command-boundary fixture only' that never contacts GitHub, so production tracker behaviour is out of scope (tests/pi_workflow_tracker.mjs:12-13)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/pi_workflow_tracker.mjs:21-31`
- `node-test|Extension%20tests|tests%2Fpi_workflow_tracker.mjs|the%20bounded%20tracker%20stub%20exposes%20duplicate%20attempts%20and%20refuses%20parent%20modification|default`
  - F=0; limit stated by the record: "Stand-in subject: the assertions read the fixture's ledger after driving the fixture CLI (tests/pi_workflow_tracker.mjs:14-32)."; the fixture/helper replaces the claimed subject rather than exercising it.
  - evidence: `tests/pi_workflow_tracker.mjs:33-41`

## proposal_rejected (7)

- `nextest|Test|ovrcr%3A%3Aserver_lifecycle|agent_admission_later_patch_preserves_resume_identity_and_lifecycle|default`
  - Author proposed consolidate (U=1); receipt quality-review-lifecycle.jsonl resolved to keep because: "Consolidate rejected. The row calls the shared resume helper with -r/2.1.274 (tests/server_lifecycle.rs:9956-9958). Production has no version gate and the version is diagnostic only (src/report/admission.rs:21,206-221), so the path is identical to the 2.1.268 row; however the retained not-gated row covers 2.1.266 and an unparsable version, not an above-floor release, so a future upper-bound regression could escape th"
  - evidence: `tests/server_lifecycle.rs:9956-9958`
- `nextest|Test|ovrcr%3A%3Atui|agent_search%3A%3Acontrol_hold_labels_visible_running_agents_without_moving_the_title_column|default`
  - Author proposed consolidate (U=1); receipt quality-review-dashboard.jsonl resolved to keep because: "Consolidate not confirmed: the body's post-release restoration at tests/tui/agent_search.rs:145-161 (no row contains [1] and the original row from column five is restored) is absent from both named tests. tests/tui/session_jump_single_digit.rs:338-349 asserts only the absence of "[01]", which is vacuous for single-digit labels; label hiding is covered by a different neighbour, tests/tui/agent_search.rs:337-346. Geome"
  - evidence: `tests/tui/agent_search.rs:92-162; tests/tui/session_jump_single_digit.rs:338-349`
- `nextest|Test|ovrcr%3A%3Atui|agent_search%3A%3Along_visible_agent_list_uses_first_nine_single_digit_labels_and_search_stays_available|default`
  - Author proposed consolidate (U=1); receipt quality-review-dashboard.jsonl resolved to keep because: "Single-digit labels and the footer are duplicated by tests/tui/session_jump_single_digit.rs:57-104 (per-id labels, [01] and [10] negatives, footer) and 191-254. The named retained tests never press and release Control before opening the Agents palette, so tests/tui/agent_search.rs:698-704 remains unique. Resolved to keep with U=2."
  - evidence: `tests/tui/agent_search.rs:659-705; tests/tui/session_jump_single_digit.rs:57-104`
- `nextest|Test|ovrcr-runtime|server%3A%3Aquota%3A%3Atests%3A%3Aprotocol_failure_not_version_is_what_reports_unsupported|default`
  - Author proposed consolidate (U=1); receipt quality-review-runtime.jsonl resolved to keep because: "The retained test tests/provider_quota.rs:1270-1285 only loops two modes: the -32600 'unknown variant' reply and a limits-shaped result (tests/provider_quota.rs:66-72). The candidate's third shape '{"unexpected":true}' carries neither result nor error, which leaves Rpc::call through the distinct ok_or_else(unrecognized) line at crates/ovrcr-runtime/src/server/quota.rs:938-940 rather than native_windows' unrecognized "
  - evidence: `crates/ovrcr-runtime/src/server/quota.rs:2224-2253; tests/provider_quota.rs:1270-1285`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Acards%3A%3Atests%3A%3Aa_narrow_unread_row_keeps_its_whole_state_and_age|default`
  - Author proposed consolidate (U=1); receipt quality-review-tui.jsonl resolved to keep because: "Resolved: consolidate is not subsumption. This body (cards.rs:1114-1131, assertion at cards.rs:1129) renders a 25-cell state text `Ready · unread reply · 2m` at width 40, while the retained a_narrow_pane_keeps_state_and_elapsed_and_shortens_the_rest (cards.rs:1058-1111, assertion at cards.rs:1089) renders the 11-cell `Working · 2m`. session_lines computes state_width as the max over the row set (cards.rs:342-352), so"
  - evidence: `crates/ovrcr-tui/src/dashboard/cards.rs:1114-1131`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_input_request_in_a_visible_pane_alerts|default`
  - Author proposed consolidate (U=1); receipt quality-review-tui.jsonl resolved to keep because: "Resolved: the retained comprehensive visibility test covers the Ready lane only. This body (desktop.rs:1734-1751) delivers snapshot_with_requests with a Select request while session 1 is focused and asserts one pending input alert (desktop.rs:1746), i.e. the input-request lane created at desktop.rs:670-700; desktop_visibility_does_not_suppress_alerts_including_split_and_empty_view (desktop.rs:2190-2262) delivers Resp"
  - evidence: `crates/ovrcr-tui/src/dashboard/desktop.rs:1734-1751`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_provider_session_event_reaches_notification_observer|default`
  - Author proposed consolidate (U=1); receipt quality-review-tui.jsonl resolved to keep because: "Resolved: the SessionChanged route is distinct. This body (desktop.rs:2063-2070) is the only test that queues a Ready alert from an empty queue through ServerEvent::SessionChanged, the provider-event path at desktop.rs:628-630; the deliver helper used by the retained rows maps to HierarchyChanged (desktop.rs:1867-1870), and desktop_queued_health_loss_then_recovery_stays_cancelled (desktop.rs:1925-1957) and desktop_pe"
  - evidence: `crates/ovrcr-tui/src/dashboard/desktop.rs:2063-2070`

## proposal_confirmed (55)

- `nextest|Test|ovrcr%3A%3Aclaude_quota_probe|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aclaude_reporting_load|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aclient_startup_server|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aclient_startup|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Acli|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Acodex_initial_prompt|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Acursor_cloud|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Agit_lifecycle|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Ainitial_prompt|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aoperator_e2e|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aoperator_scope|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Api_delegation|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Api_package|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aprovider_quota|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aquota_ui_option_a_smoke|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aregistry_migration|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aresource_cli|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aretained_capacity|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aretained_ownership|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aretained_sessions|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aserver_lifecycle|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Asettings_lifecycle|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Atask_execution|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aterminal_acceptance|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Atui|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Atui|mouse%3A%3Adragging_tracked_mouse_into_sidebar_releases_application_button|default`
  - Author proposed consolidate and receipt quality-review-dashboard.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Atui|mouse%3A%3Aleaving_the_rectangle_releases_at_last_valid_cell|default. Record quote: "Preservation evidence: same release_held_leaving_rectangle production path (state.rs:3460-3560), identical expected press/release bytes, and the retained representative is named by full test_id nextest|Test|ovrcr%3A%3Atui|mouse%3A%3Aleaving_the_rectangle_releases_at_last_valid_cell|default in overla"
  - evidence: `tests/tui/mouse.rs:527-551; tests/tui/mouse.rs:500-524`
- `nextest|Test|ovrcr%3A%3Atui|split%3A%3Asplit_layout_geometry_and_narrow_fallback|default`
  - Author proposed consolidate and receipt quality-review-dashboard.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Atui|split%3A%3Apane_rects_keep_split_geometry_and_hide_unfocused_tiny_pane|default. Record quote: "Same function and same asserted geometry as pane_rects_keep_split_geometry_and_hide_unfocused_tiny_pane; that retained test already covers the narrow focused-index branch, so removal loses no boundary (crates/ovrcr-tui/src/dashboard/mod.rs:324-390)."
  - evidence: `tests/tui/split.rs:186-196; tests/tui/split.rs:173-183`
- `nextest|Test|ovrcr%3A%3Awip_save|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr%3A%3Aworkspace_identity|live%3A%3Acommand_clears_inherited_reporting_authority|default`
  - Author proposed consolidate and receipt quality-review-shared.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default. Record quote: "Repeated execution of an identical body: the assertions and the call path (Live::idle -> Live::command, tests/support/live.rs:111-196) are byte-identical to the retained representative nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default, and nothin"
  - evidence: `tests/support/live.rs:568-579`
- `nextest|Test|ovrcr-protocol|auto_trust%3A%3Atests%3A%3Abare_managed_launches_gain_one_provider_flag|default`
  - Author proposed consolidate and receipt quality-review-protocol.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Acli|agent_run_adds_default_auto_trust_only_when_the_permission_family_is_absent|default. Record quote: "Fully subsumed by ovrcr::cli agent_run_adds_default_auto_trust_only_when_the_permission_family_is_absent; that retained acceptance test asserts the same four flags from the real CLI, preserving the high-criticality invariant with no unit-only boundary lost."
  - evidence: `crates/ovrcr-protocol/src/auto_trust.rs:81-93; tests/cli.rs:2164-2236`
- `nextest|Test|ovrcr-protocol|settings%3A%3Atests%3A%3Atheme_appearance_metadata_groups_every_existing_theme|default`
  - Author proposed consolidate and receipt quality-review-protocol.jsonl agreed. Retained protection named: nextest|Test|ovrcr-protocol|settings%3A%3Atests%3A%3Atheme_catalog_matches_the_exact_approved_manifest|default. Record quote: "Fully subsumed by row 14 (theme_catalog_matches_the_exact_approved_manifest), which iterates all 80 themes and compares appearance against the manifest; retain that test rather than this eight-theme subset."
  - evidence: `crates/ovrcr-protocol/src/settings.rs:1974-1992; crates/ovrcr-protocol/src/settings.rs:1995-2068`
- `nextest|Test|ovrcr-runtime|server%3A%3Abuild_identity%3A%3Atests%3A%3Afingerprint_detects_byte_changes_with_same_length_and_preserves_open_build|default`
  - Author proposed consolidate and receipt quality-review-runtime.jsonl agreed. Retained protection named: nextest|Test|ovrcr-runtime|server%3A%3Abuild_identity%3A%3Atests%3A%3Acapture_covers_small_files_and_fixed_block_boundaries|default. Record quote: "The changed-bytes invariant is fully and more strictly covered by the block-boundary matrix; folding this case into that loop keeps the same protection without a second 130KB writer."
  - evidence: `crates/ovrcr-runtime/src/server/build_identity.rs:266-275; crates/ovrcr-runtime/src/server/build_identity.rs:278-291`
- `nextest|Test|ovrcr-runtime|server%3A%3Adefault_session_label_tests%3A%3Adirect_agent_executable_matches_kind_when_label_omitted|default`
  - Author proposed consolidate and receipt quality-review-runtime.jsonl agreed. Retained protection named: nextest|Test|ovrcr-runtime|server%3A%3Adefault_session_label_tests%3A%3Amanaged_pi_launcher_uses_agent_kind_not_ovrcr_argv0|default. Record quote: "No independent expectation: merging it into the Agent-kind representative loses nothing, and the existing pi and claude fixtures already guard the same invariant."
  - evidence: `crates/ovrcr-runtime/src/server/mod.rs:3251-3262; crates/ovrcr-runtime/src/server/mod.rs:3197-3206`
- `nextest|Test|ovrcr-runtime|server%3A%3Adefault_session_label_tests%3A%3Amanaged_claude_launcher_uses_agent_kind_not_ovrcr_argv0|default`
  - Author proposed consolidate and receipt quality-review-runtime.jsonl agreed. Retained protection named: nextest|Test|ovrcr-runtime|server%3A%3Adefault_session_label_tests%3A%3Amanaged_pi_launcher_uses_agent_kind_not_ovrcr_argv0|default. Record quote: "Independently confirmed duplicate of the pi fixture: the Agent arm at crates/ovrcr-runtime/src/server/mod.rs:3178-3180 never inspects argv, so one harness name proves the rule; keep the pi representative and its assertion."
  - evidence: `crates/ovrcr-runtime/src/server/mod.rs:3209-3228; crates/ovrcr-runtime/src/server/mod.rs:3197-3206`
- `nextest|Test|ovrcr-runtime|server%3A%3Adelegation_files%3A%3Aread_tests%3A%3Acheckout_read_is_bounded_relative_and_never_follows_links|default`
  - Author proposed consolidate and receipt quality-review-runtime-delegation.jsonl agreed. Retained protection named: nextest|Test|ovrcr-runtime%3A%3Adelegation_fallback_files|delegation_files%3A%3Aread_tests%3A%3Acheckout_read_is_bounded_relative_and_never_follows_links|default. Record quote: "Consolidate with the retained representative nextest|Test|ovrcr-runtime%3A%3Adelegation_fallback_files|delegation_files%3A%3Aread_tests%3A%3Acheckout_read_is_bounded_relative_and_never_follows_links|default: both build the same real checkout, symlink, hardlink and oversized file and assert the same "
  - evidence: `crates/ovrcr-runtime/src/server/delegation_files.rs:236-279`
- `nextest|Test|ovrcr-runtime|server%3A%3Aquota%3A%3Atests%3A%3Anative_client_is_not_gated_on_the_harness_version|default`
  - Author proposed consolidate and receipt quality-review-runtime.jsonl agreed. Retained protection named: nextest|Test|ovrcr%3A%3Aprovider_quota|codex_native_read_reaches_remaining_bars_through_real_server_and_socket|default. Record quote: "The no-version-gate claim, the exact app-server argv and the parsed limits are asserted end to end by codex_native_read_reaches_remaining_bars_through_real_server_and_socket; this unit copy adds only two extra version strings that the code never reads."
  - evidence: `crates/ovrcr-runtime/src/server/quota.rs:2194-2221; tests/provider_quota.rs:141-249`
- `nextest|Test|ovrcr-runtime|settings%3A%3Atests%3A%3Aquota_in_instance_identity_is_a_finding_and_configures_nothing|default`
  - Author proposed consolidate and receipt quality-review-storage.jsonl agreed. Retained protection named: nextest|Test|ovrcr-runtime%3A%3Asettings_matrix|quota_in_instance_identity_is_one_finding_and_configures_nothing|default. Record quote: "High-confidence redundancy: keep nextest|Test|ovrcr-runtime%3A%3Asettings_matrix|quota_in_instance_identity_is_one_finding_and_configures_nothing|default (settings_matrix.rs:778-804) as the protector of the C=2 invariant that [quota] in the preserved identity configures nothing and still yields the "
  - evidence: `crates/ovrcr-runtime/src/settings/mod.rs:1525-1545; crates/ovrcr-runtime/tests/settings_matrix.rs:778-803`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Acursor_detection_uses_managed_launch|default`
  - Author proposed consolidate and receipt quality-review-tui.jsonl agreed. Retained protection named: nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Amanaged_agents_launch_through_the_agent_run_route|default. Record quote: "Retained protection: nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Amanaged_agents_launch_through_the_agent_run_route|default asserts cursor-agent's exact wrapped argv inside the MANAGED_AGENTS loop at agents.rs:402 with the same launcher and temp PATH configuration, and its override b"
  - evidence: `crates/ovrcr-tui/src/dashboard/agents.rs:321-356; crates/ovrcr-tui/src/dashboard/agents.rs:359-451`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Ahermes_detection_uses_managed_launch_and_preserves_explicit_override|default`
  - Author proposed consolidate and receipt quality-review-tui.jsonl agreed. Retained protection named: nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Adetects_executables_then_shell_then_custom|default, nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Amanaged_agents_launch_through_the_agent_run_route|default. Record quote: "Retained protection: nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Amanaged_agents_launch_through_the_agent_run_route|default asserts hermes' exact wrapped argv in the same MANAGED_AGENTS loop at agents.rs:402, and nextest|Test|ovrcr-tui|dashboard%3A%3Aagents%3A%3Atests%3A%3Adetects_ex"
  - evidence: `crates/ovrcr-tui/src/dashboard/agents.rs:454-501; crates/ovrcr-tui/src/dashboard/agents.rs:241-287`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_confirmed_without_unread_does_not_queue|default`
  - Author proposed consolidate and receipt quality-review-tui.jsonl agreed. Retained protection named: nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_does_not_queue_when_unread_is_absent|default. Record quote: "Retained protection: nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_does_not_queue_when_unread_is_absent|default uses the same snapshot(2,"a",ResponseReady) input with unread None and asserts the same empty queue at crates/ovrcr-tui/src/dashboard/desktop.rs:2073-2080; crates/o"
  - evidence: `crates/ovrcr-tui/src/dashboard/desktop.rs:2083-2097; crates/ovrcr-tui/src/dashboard/desktop.rs:2072-2081`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_health_loss_before_dispatch_cancels_pending_ready|default`
  - Author proposed consolidate and receipt quality-review-tui.jsonl agreed. Retained protection named: nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_queued_health_loss_then_recovery_stays_cancelled|default. Record quote: "Retained protection: nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_queued_health_loss_then_recovery_stays_cancelled|default performs the same health-loss observation at crates/ovrcr-tui/src/dashboard/desktop.rs:1937-1948, asserts pending is empty, then emits and asserts no ho"
  - evidence: `crates/ovrcr-tui/src/dashboard/desktop.rs:1979-1998; crates/ovrcr-tui/src/dashboard/desktop.rs:1925-1957`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Apalette%3A%3Alaunch_tests%3A%3Adetected_omp_model_query_exposes_exact_selectors_and_initial_prompt_support|default`
  - Author proposed consolidate and receipt quality-review-tui.jsonl agreed. Retained protection named: nextest|Test|ovrcr-tui|dashboard%3A%3Apalette%3A%3Alaunch_tests%3A%3Adetected_pi_model_query_exposes_exact_ids_and_keeps_its_binding|default. Record quote: "Retained protection: nextest|Test|ovrcr-tui|dashboard%3A%3Apalette%3A%3Alaunch_tests%3A%3Adetected_pi_model_query_exposes_exact_ids_and_keeps_its_binding|default asserts the same QueryAgentModels tuple, alias==resolved handling and selected_launch policy/binding/expected_model at crates/ovrcr-tui/sr"
  - evidence: `crates/ovrcr-tui/src/dashboard/palette.rs:12248-12354; crates/ovrcr-tui/src/dashboard/palette.rs:12133-12245`
- `nextest|Test|ovrcr-tui|dashboard%3A%3Arender%3A%3Aharness_agent_label_tests%3A%3Acli_created_claude_with_ovrcr_label_shows_claude_harness|default`
  - Author proposed consolidate and receipt quality-review-tui.jsonl agreed. Retained protection named: nextest|Test|ovrcr-tui|dashboard%3A%3Arender%3A%3Aharness_agent_label_tests%3A%3Acli_created_pi_with_ovrcr_label_shows_pi_harness|default. Record quote: "Retained protection: nextest|Test|ovrcr-tui|dashboard%3A%3Arender%3A%3Aharness_agent_label_tests%3A%3Acli_created_pi_with_ovrcr_label_shows_pi_harness|default asserts the identical Agent-kind branch and ('harness','') tuple at crates/ovrcr-tui/src/dashboard/render.rs:1671-1682; fold the claude name "
  - evidence: `crates/ovrcr-tui/src/dashboard/render.rs:1685-1698; crates/ovrcr-tui/src/dashboard/render.rs:1671-1682`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|client-extra-argv%3Aln56|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|client-extra-argv%3Aln56|production. Record quote: "Identical statement in both modes; retain the production row."
  - evidence: `native/bridge/check-guards.py:56-56`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--check-contract%27%2C%20%27-1%27%2C%20str%28wire%29%5D%5D%3Aln53|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--check-contract%27%2C%20%27-1%27%2C%20str%28wire%29%5D%5D%3Aln53|production. Record quote: "Identical argv statement in both modes; retain the production row for this argv."
  - evidence: `native/bridge/check-guards.py:53-55`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--check-contract%27%2C%20str%28schema%29%2C%20%274294967296%27%5D%5D%3Aln53|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--check-contract%27%2C%20str%28schema%29%2C%20%274294967296%27%5D%5D%3Aln53|production. Record quote: "Identical argv statement in both modes; retain the production row for this argv."
  - evidence: `native/bridge/check-guards.py:53-55`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--check-contract%27%5D%5D%3Aln53|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--check-contract%27%5D%5D%3Aln53|production. Record quote: "Identical argv statement in both modes; retain the production row for this argv."
  - evidence: `native/bridge/check-guards.py:53-55`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--unexpected%27%5D%5D%3Aln53|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-argv%5B%5B%27--unexpected%27%5D%5D%3Aln53|production. Record quote: "Identical argv statement in both modes; retain the production row for this argv."
  - evidence: `native/bridge/check-guards.py:53-55`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-available%3Aln50|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-available%3Aln50|production. Record quote: "No mode-dependent branch: retain the production contract-available row."
  - evidence: `native/bridge/check-guards.py:50-50`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-schema-newer%3Aln51|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-schema-newer%3Aln51|production. Record quote: "Identical checks in both modes; retain the production row for this field."
  - evidence: `native/bridge/check-guards.py:51-51`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-wire-newer%3Aln52|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|contract-wire-newer%3Aln52|production. Record quote: "Identical checks in both modes; retain the production row for this field."
  - evidence: `native/bridge/check-guards.py:52-52`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|entitlement-pin%3Aln35|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|entitlement-pin%3Aln35|production. Record quote: "Identical statements in both modes; retain the production entitlement-pin row as the representative."
  - evidence: `native/bridge/check-guards.py:28-35`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|opposite-profile-extra-argv%3Aln98|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|opposite-profile-extra-argv%3Aln98|production. Record quote: "Identical literal command in both modes; retain the production row."
  - evidence: `native/bridge/check-guards.py:98-98`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-packaging.py|entitlement-pin%3Aln32|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-packaging.py|entitlement-pin%3Aln32|production. Record quote: "Identical block in both modes; retain the production entitlement-pin row."
  - evidence: `native/bridge/check-packaging.py:25-32`
- `python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-packaging.py|fixture-identity-accepted-production%3Aln114|local-development`
  - Author proposed consolidate and receipt quality-review-packaging.jsonl agreed. Retained protection named: python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-packaging.py|fixture-identity-accepted-production%3Aln114|production. Record quote: "Byte-identical explicit-identity call in both modes; retain the production row."
  - evidence: `native/bridge/check-packaging.py:114-114`

## product_observation (2)

- `nextest|Test|ovrcr|remote%3A%3Atests%3A%3Aan_unconfigured_gateway_refuses_tailnet_requests|default`
  - INCIDENTAL PRODUCT OBSERVATION, not a test-value claim (surfaced by the agree receipt quality-review-shared): "host_is_loopback at src/remote/mod.rs:703-710 returns true for any Host starting with "[::1]" (and splits the port off the rest), so a tailnet peer sending Host: [::1].example is authorized without any login - a real fail-open path adjacent to this gate". The adjacent tests still pass; the observation is that the product's loopback test is prefix-based and fails open.
  - evidence: `src/remote/mod.rs:703-710`
- `nextest|Test|ovrcr|remote%3A%3Atests%3A%3Atailnet_requests_need_the_configured_login|default`
  - INCIDENTAL PRODUCT OBSERVATION, not a test-value claim (surfaced by the resolved receipt quality-review-shared): "host_is_loopback at src/remote/mod.rs:703-710 returns true for any Host starting with "[::1]" (and splits the port off the rest), so a tailnet peer sending Host: [::1].example is authorized without any login - a real fail-open path adjacent to this gate". The adjacent tests still pass; the observation is that the product's loopback test is prefix-based and fails open.
  - evidence: `src/remote/mod.rs:703-710`

## Counts

| section | entries |
|---|---|
| claim_mismatch | 11 |
| vacuous_or_weak | 6 |
| fixture_fidelity | 25 |
| proposal_rejected | 7 |
| proposal_confirmed | 55 |
| product_observation | 2 |
| total entries | 106 |
| unique tests | 96 |

Rows in more than one category: 7.

- 3x `nextest|Test|ovrcr%3A%3Atui|mouse%3A%3Aterminal_mode_keeps_dashboard_mouse_capture_without_a_ready_session|default`
- 3x `nextest|Test|ovrcr-runtime|server%3A%3Adefault_session_label_tests%3A%3Adirect_agent_executable_matches_kind_when_label_omitted|default`
- 3x `nextest|Test|ovrcr-tui|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_confirmed_without_unread_does_not_queue|default`
- 2x `nextest|Test|ovrcr%3A%3Atask_tui|unicode_prompt_render_is_safe_while_another_field_has_focus|default`
- 2x `nextest|Test|ovrcr-runtime|retained%3A%3Aoperator_schema_tests%3A%3Aregistry_migration_adds_operator_journal_to_existing_owner|default`
- 2x `nextest|Test|ovrcr-runtime|server%3A%3Adashboard%3A%3Atests%3A%3Aforget_session_drops_history_and_focused_view|default`
- 2x `nextest|Test|ovrcr-tui|dashboard%3A%3Apalette%3A%3Amouse_tests%3A%3Amouse_form_focus_unicode_edit_and_cancel|default`

## Proposal reconciliation

- Total prune/consolidate proposals in all reviews: **63** (consolidate 63, prune_candidate 0).
- proposal_confirmed: **55**
- proposal_rejected: **7**
- Covered by an independent receipt verdict: **62** of 63.
- Proposals with no independent receipt verdict (1):
  - `nextest|Test|ovrcr%3A%3Atask_tui|destructive_actions_require_confirmation_and_escape_cancels|default` (C=3, consolidate)

## Validation

- Every entry's `test_id` exists in `evidence/inventory.csv` and its `source_file` matches the inventory row.
- Every `path:start-end` in `evidence` resolves to an existing file with an in-bounds line range (checked for all entries).
- Deterministic ordering: sections follow the fixed category order; entries are sorted by full `test_id`.
- Product observations are labelled incidental and are not counted as test-value findings.