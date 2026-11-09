# Overlap groups

Source receipts: `evidence/quality-review-*.jsonl`. Source proposals: `reviews/*.jsonl` rows whose recommendation is `consolidate` or `prune_candidate`.

## Summary

- Confirmed groups: **28**
- Member entries covered: **85** (81 unique test ids)
- Proposals without a group: **7** (7 rejected by an independent receipt, 0 unconfirmed)
- The 27 shared-fixture proposal rows for `live::command_clears_inherited_reporting_authority` are one group (`og-001`) with 28 member executions and one retained id, per the shared receipt.

## Confirmed groups

| group_id | status | members | retained id(s) | reason |
|---|---|---:|---|---|
| og-001 | confirmed | 28 | nextest\|Test\|ovrcr%3A%3Aautomatic_titles\|live%3A%3Acommand_clears_inherited_reporting_authority\|default | 28 executions of the single tests/support/live.rs:567-579 body; the receipt confirms every binary includes it with no cfg branch on the assertion path. |
| og-003 | confirmed | 2 | nextest\|Test\|ovrcr%3A%3Atui\|mouse%3A%3Aleaving_the_rectangle_releases_at_last_valid_cell\|default | same drag-outside-the-pane release path; both points are outside the focused rect and expect identical bytes. |
| og-004 | confirmed | 2 | nextest\|Test\|ovrcr%3A%3Atui\|split%3A%3Apane_rects_keep_split_geometry_and_hide_unfocused_tiny_pane\|default | pane_rects with focused=1 adds no geometry boundary; the retained row covers the wide and 1x1 cases. |
| og-005 | confirmed | 2 | nextest\|Test\|ovrcr%3A%3Acli\|agent_run_adds_default_auto_trust_only_when_the_permission_family_is_absent\|default | auto-trust flags are asserted end-to-end (with -- placement and families) by the retained CLI test. |
| og-006 | confirmed | 2 | nextest\|Test\|ovrcr-protocol\|settings%3A%3Atests%3A%3Atheme_catalog_matches_the_exact_approved_manifest\|default | the 80-theme manifest matrix subsumes the eight hardcoded rows. |
| og-007 | confirmed | 2 | nextest\|Test\|ovrcr-runtime\|server%3A%3Abuild_identity%3A%3Atests%3A%3Acapture_covers_small_files_and_fixed_block_boundaries\|default | the retained matrix uses the same executable_build path with a wider byte-offset set and repetition assertions. |
| og-008 | confirmed | 3 | nextest\|Test\|ovrcr-runtime\|server%3A%3Adefault_session_label_tests%3A%3Amanaged_pi_launcher_uses_agent_kind_not_ovrcr_argv0\|default | its argv0 coincides with the kind, so the retained pi fixture is the discriminating input. |
| og-009 | confirmed | 2 | nextest\|Test\|ovrcr-runtime\|server%3A%3Adefault_session_label_tests%3A%3Amanaged_pi_launcher_uses_agent_kind_not_ovrcr_argv0\|default | the retained pi fixture proves the same Agent-arm label with a discriminating argv. |
| og-010 | confirmed | 2 | nextest\|Test\|ovrcr-runtime%3A%3Adelegation_fallback_files\|delegation_files%3A%3Aread_tests%3A%3Acheckout_read_is_bounded_relative_and_never_follows_links\|default | the retained second binary includes the identical delegation_files.rs body and assertions. |
| og-011 | confirmed | 2 | nextest\|Test\|ovrcr%3A%3Aprovider_quota\|codex_native_read_reaches_remaining_bars_through_real_server_and_socket\|default | the retained real-server test asserts the same argv and the absence of a --version probe. |
| og-013 | confirmed | 3 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Aagents%3A%3Atests%3A%3Amanaged_agents_launch_through_the_agent_run_route\|default | cursor-agent argv and override are asserted inside the retained MANAGED_AGENTS loop. |
| og-014 | confirmed | 3 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Aagents%3A%3Atests%3A%3Adetects_executables_then_shell_then_custom\|default<br>nextest\|Test\|ovrcr-tui\|dashboard%3A%3Aagents%3A%3Atests%3A%3Amanaged_agents_launch_through_the_agent_run_route\|default | hermes argv is in the retained loop; the 0644 skip is covered by the executable_in test. |
| og-015 | confirmed | 2 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_does_not_queue_when_unread_is_absent\|default | same snapshot with unread None; the Confirmed field is inert at the producer gate. |
| og-016 | confirmed | 2 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_queued_health_loss_then_recovery_stays_cancelled\|default | the retained queued-health-loss test performs the same observation and pending/receipt checks. |
| og-017 | confirmed | 2 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Apalette%3A%3Alaunch_tests%3A%3Adetected_pi_model_query_exposes_exact_ids_and_keeps_its_binding\|default | the retained pi query body covers the same selectors and binding; omp adds only its preset name. |
| og-018 | confirmed | 2 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Arender%3A%3Aharness_agent_label_tests%3A%3Acli_created_pi_with_ovrcr_label_shows_pi_harness\|default | the retained pi case asserts the identical Agent-kind label branch and tuple shape. |
| og-019 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|client-extra-argv%3Aln56\|production | both variants pass the literal extra argv at check-guards.py:56, hitting main.swift:24-26. |
| og-020 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-argv%5B%5B%27--check-contract%27%2C%20%27-1%27%2C%20str%28wire%29%5D%5D%3Aln53\|production | both variants run the same -1/wire argv guard at check-guards.py:53-55. |
| og-021 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-argv%5B%5B%27--check-contract%27%2C%20str%28schema%29%2C%20%274294967296%27%5D%5D%3Aln53\|production | both variants run the same schema/4294967296 argv guard at check-guards.py:53-55. |
| og-022 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-argv%5B%5B%27--check-contract%27%5D%5D%3Aln53\|production | both variants run the same literal-argv guard at check-guards.py:53-55. |
| og-023 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-argv%5B%5B%27--unexpected%27%5D%5D%3Aln53\|production | both variants run the same unexpected-argv guard at check-guards.py:53-55. |
| og-024 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-available%3Aln50\|production | both variants run the same availability guard at check-guards.py:50. |
| og-025 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-schema-newer%3Aln51\|production | both variants run the same schema-newer guard at check-guards.py:51. |
| og-026 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|contract-wire-newer%3Aln52\|production | both variants run the same wire-newer guard at check-guards.py:52. |
| og-027 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|entitlement-pin%3Aln35\|production | both variants run the same standalone entitlement pin block at check-guards.py:28-35. |
| og-028 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|opposite-profile-extra-argv%3Aln98\|production | both variants run the four-argument call at check-guards.py:98, hitting the count guard. |
| og-029 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-packaging.py\|entitlement-pin%3Aln32\|production | both variants run the same standalone pin at check-packaging.py:28-32. |
| og-030 | confirmed | 2 | python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-packaging.py\|fixture-identity-accepted-production%3Aln114\|production | the block sits outside the local_development if and passes local=False in both runs. |

## Proposals without a group

### Rejected by an independent receipt (final recommendation keep)

| proposal (review file:line) | candidate | reason |
|---|---|---|
| lifecycle-terminal-acceptance-001.jsonl:72 | nextest\|Test\|ovrcr%3A%3Aserver_lifecycle\|agent_admission_later_patch_preserves_resume_identity_and_lifecycle\|default | Consolidate rejected. The row calls the shared resume helper with -r/2.1.274 (tests/server_lifecycle.rs:9956-9958). Production has no version gate and the version is diagnostic only (src/report/admission.rs:21,206-221), so the path is identical to the 2.1.268 row; however the re… |
| public-dashboard-001.jsonl:8 | nextest\|Test\|ovrcr%3A%3Atui\|agent_search%3A%3Acontrol_hold_labels_visible_running_agents_without_moving_the_title_column\|default | Consolidate not confirmed: the body's post-release restoration at tests/tui/agent_search.rs:145-161 (no row contains [1] and the original row from column five is restored) is absent from both named tests. tests/tui/session_jump_single_digit.rs:338-349 asserts only the absence of… |
| public-dashboard-001.jsonl:19 | nextest\|Test\|ovrcr%3A%3Atui\|agent_search%3A%3Along_visible_agent_list_uses_first_nine_single_digit_labels_and_search_stays_available\|default | Single-digit labels and the footer are duplicated by tests/tui/session_jump_single_digit.rs:57-104 (per-id labels, [01] and [10] negatives, footer) and 191-254. The named retained tests never press and release Control before opening the Agents palette, so tests/tui/agent_search.… |
| runtime-server-session-002.jsonl:9 | nextest\|Test\|ovrcr-runtime\|server%3A%3Aquota%3A%3Atests%3A%3Aprotocol_failure_not_version_is_what_reports_unsupported\|default | The retained test tests/provider_quota.rs:1270-1285 only loops two modes: the -32600 'unknown variant' reply and a limits-shaped result (tests/provider_quota.rs:66-72). The candidate's third shape '{"unexpected":true}' carries neither result nor error, which leaves Rpc::call thr… |
| tui-library-001.jsonl:27 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Acards%3A%3Atests%3A%3Aa_narrow_unread_row_keeps_its_whole_state_and_age\|default | Resolved: consolidate is not subsumption. This body (cards.rs:1114-1131, assertion at cards.rs:1129) renders a 25-cell state text `Ready · unread reply · 2m` at width 40, while the retained a_narrow_pane_keeps_state_and_elapsed_and_shortens_the_rest (cards.rs:1058-1111, assertio… |
| tui-library-001.jsonl:42 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_input_request_in_a_visible_pane_alerts\|default | Resolved: the retained comprehensive visibility test covers the Ready lane only. This body (desktop.rs:1734-1751) delivers snapshot_with_requests with a Select request while session 1 is focused and asserts one pending input alert (desktop.rs:1746), i.e. the input-request lane c… |
| tui-library-001.jsonl:52 | nextest\|Test\|ovrcr-tui\|dashboard%3A%3Adesktop%3A%3Atests%3A%3Adesktop_provider_session_event_reaches_notification_observer\|default | Resolved: the SessionChanged route is distinct. This body (desktop.rs:2063-2070) is the only test that queues a Ready alert from an empty queue through ServerEvent::SessionChanged, the provider-event path at desktop.rs:628-630; the deliver helper used by the retained rows maps t… |

### Unconfirmed (no independent receipt yet)

None: every consolidate/prune proposal now has a receipt.

## Verification

- Every retained id is a member of its group and is a reviewed `keep` row (no retained id is a `consolidate` or `prune_candidate` row).
- Every member and retained id exists in `evidence/inventory.csv`.
- Every group cites the candidate and retained execution, the shared production path, and the surviving assertions; the groups containing a C>=3 member state the surviving high-criticality invariant.
- Members and retained ids are sorted; groups are ordered by `group_id`; JSON is UTF-8 with LF and two-space indent.
