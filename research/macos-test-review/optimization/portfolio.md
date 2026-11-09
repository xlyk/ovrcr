reviewer_model: openai-codex/gpt-6.1-sol

# Value-weighted consolidation economics

## Ranked consolidations

The confirmed consolidation portfolio removes little measured runtime: **42 timed candidates account for 4.344 cumulative test-process seconds**. Twelve Python candidates have no individual duration. Removing all 54 does not support a claim of minutes saved. Keep the expensive, distinct protection and optimize its mechanisms instead.

This analysis uses `../tests.csv` (SHA-256 `dcccdb0ec9d19a3569a05285f33f7620e71308f3765edf2d0086996ca767fb5a`), `../evidence/attempts.csv`, and the 28 objects in `../evidence/overlap-groups.json`. The CSV contains 54 `consolidate`, eight `improve`, 3,109 `keep`, and no `prune_candidate` rows. All timings below are from hosted run [42429838269, job 128576086691](https://github.com/xlyk/ovrcr/actions/runs/42429838269/job/128576086691), at revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`. These are single-run elapsed durations of test processes, not CPU time or observed CI savings.

The group order first takes measured groups with maximum criticality C≤2, in descending removable seconds. It then takes C=3 and C=4 groups, subject to preserving the exact named survivors. Unknown-duration groups come last until timed; their ordering is by criticality, not an invented saving. Criticality is an ordinal protection score, not a probability, so dividing seconds by C would give a false risk model. The separate time-rank column allows a reader to choose a pure seconds-first order.

`Cnn` identifies one candidate in the per-row table below. `Rnn` identifies an existing retained test in the survivor table. All members of a group are its listed candidates plus its listed survivors. The source of group membership and preservation claims is `../evidence/overlap-groups.json`, keyed by group ID. No proposed removal may include a survivor.

| Action order | Group | Members: candidates; survivors | Total members | Raw seconds rank | Removable process seconds | Incremental seconds in this order | Max C |
|---:|---|---|---:|---:|---:|---:|---:|
| 1 | og-001 | C02, C03, C04, C05, C06, C07, C08, C09, C10, C11, C12, C13, C14, C15, C16, C17, C18, C19, C20, C21, C22, C23, C24, C25, C26, C27, C28; R01 | 28 | 1 | 3.345 | 3.345 | 2 |
| 2 | og-011 | C01; R03 | 2 | 2 | 0.751 | 0.751 | 2 |
| 3 | og-003 | C29; R04 | 2 | 4 | 0.023 | 0.023 | 2 |
| 4 | og-008 | C37, C38; R09 | 3 | 5 | 0.020 | 0.020 | 2 |
| 5 | og-017 | C30; R14 | 2 | 6 | 0.017 | 0.017 | 2 |
| 6 | og-018 | C31; R15 | 2 | 7 | 0.015 | 0.015 | 1 |
| 7 | og-004 | C32; R05 | 2 | 8 | 0.013 | 0.013 | 2 |
| 8 | og-007 | C33; R08 | 2 | 10 | 0.013 | 0.013 | 2 |
| 9 | og-015 | C34; R12 | 2 | 12 | 0.013 | 0.013 | 2 |
| 10 | og-016 | C35; R13 | 2 | 13 | 0.012 | 0.012 | 2 |
| 11 | og-006 | C36; R06 | 2 | 14 | 0.011 | 0.011 | 2 |
| 12 | og-009 | C38; R09 | 2 | 16 | 0.010 | 0.000 | 2 |
| 13 | og-013 | C39, C41; R11 | 3 | 3 | 0.085 | 0.085 | 3 |
| 14 | og-005 | C40; R02 | 2 | 9 | 0.013 | 0.013 | 3 |
| 15 | og-014 | C41; R10, R11 | 3 | 15 | 0.011 | 0.000 | 3 |
| 16 | og-010 | C42; R07 | 2 | 11 | 0.013 | 0.013 | 4 |
| 17 | og-019 | C43; R16 | 2 | — | unknown | unknown | 2 |
| 18 | og-020 | C44; R17 | 2 | — | unknown | unknown | 2 |
| 19 | og-021 | C45; R18 | 2 | — | unknown | unknown | 2 |
| 20 | og-022 | C46; R19 | 2 | — | unknown | unknown | 2 |
| 21 | og-023 | C47; R20 | 2 | — | unknown | unknown | 2 |
| 22 | og-028 | C48; R25 | 2 | — | unknown | unknown | 2 |
| 23 | og-030 | C49; R27 | 2 | — | unknown | unknown | 2 |
| 24 | og-024 | C50; R21 | 2 | — | unknown | unknown | 3 |
| 25 | og-025 | C51; R22 | 2 | — | unknown | unknown | 3 |
| 26 | og-026 | C52; R23 | 2 | — | unknown | unknown | 3 |
| 27 | og-027 | C53; R24 | 2 | — | unknown | unknown | 4 |
| 28 | og-029 | C54; R26 | 2 | — | unknown | unknown | 4 |

The raw group totals sum to **4.365 s**, but the unique candidate total is **4.344 s**. `og-009` repeats the 0.010 s Claude-label candidate already in `og-008`. `og-014` repeats the 0.011 s Hermes candidate already in `og-013`. They are preservation receipts, not additional removals. `og-014` also names an extra survivor needed for Hermes' non-executable case; retain both survivors even when executing the removal under `og-013`.

`og-001` and `og-011` together remove **4.096 s**, or **94.29%** of the measured consolidation portfolio. The rest remove 0.248 s. This makes those two groups the defensible first consolidation work, although neither is a material response to a 33-minute nextest run. The C=3/4 groups are worthwhile only as maintenance simplifications; their timed savings together are 0.111 s. The Python groups need timing before their economics can be ranked.

The twelve Python removals are individual duplicate logical checks, not permission to drop the local-development script invocations. Keep both profile passes: `native/bridge/check-packaging.py:115-118` has local-only checks, and `native/bridge/check-guards.py:24-25,77-98` exercises profile-dependent client behavior. Their other cases and setup remain outside this removal set.

### Candidate-by-candidate ranking

The action order here ranks individual candidates, rather than whole groups: measured C≤2 first by seconds, then C=3/4 by risk and seconds, then unknown duration. A group's maximum C, not just the removed row's C, controls its protection tier. Raw time rank covers the 42 measured candidates only; ties use full test ID. All durations and C/S/U/F values are from the candidate's row in `../tests.csv`. Rust rows have `runner=nextest`, `ci_step=Test`, `variant=default`. Python rows have `runner=python-check`, `ci_step=Relocated macOS startup assets`; their displayed variant disambiguates the two executions. Suite, name, variant and runner/step therefore identify each row without relying on a basename.

| Action order / ID | Raw time rank | Suite / test / variant | Seconds | Row C/S/U/F | Group max C | Groups | Source |
|---|---:|---|---:|---|---:|---|---|
| 1 / C01 | 1 | `ovrcr-runtime` / `server::quota::tests::native_client_is_not_gated_on_the_harness_version` | 0.751 | 2/4/1/3 | 2 | og-011 | crates/ovrcr-runtime/src/server/quota.rs:2194-2221 |
| 2 / C02 | 2 | `ovrcr::tui` / `live::command_clears_inherited_reporting_authority` | 0.294 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 3 / C03 | 3 | `ovrcr::cursor_cloud` / `live::command_clears_inherited_reporting_authority` | 0.229 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 4 / C04 | 4 | `ovrcr::pi_package` / `live::command_clears_inherited_reporting_authority` | 0.203 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 5 / C05 | 5 | `ovrcr::quota_ui_option_a_smoke` / `live::command_clears_inherited_reporting_authority` | 0.194 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 6 / C06 | 6 | `ovrcr::claude_reporting_load` / `live::command_clears_inherited_reporting_authority` | 0.151 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 7 / C07 | 7 | `ovrcr::wip_save` / `live::command_clears_inherited_reporting_authority` | 0.148 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 8 / C08 | 8 | `ovrcr::workspace_identity` / `live::command_clears_inherited_reporting_authority` | 0.148 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 9 / C09 | 9 | `ovrcr::task_execution` / `live::command_clears_inherited_reporting_authority` | 0.142 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 10 / C10 | 10 | `ovrcr::provider_quota` / `live::command_clears_inherited_reporting_authority` | 0.137 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 11 / C11 | 11 | `ovrcr::retained_sessions` / `live::command_clears_inherited_reporting_authority` | 0.127 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 12 / C12 | 12 | `ovrcr::client_startup_server` / `live::command_clears_inherited_reporting_authority` | 0.126 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 13 / C13 | 13 | `ovrcr::registry_migration` / `live::command_clears_inherited_reporting_authority` | 0.125 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 14 / C14 | 14 | `ovrcr::claude_quota_probe` / `live::command_clears_inherited_reporting_authority` | 0.118 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 15 / C15 | 15 | `ovrcr::terminal_acceptance` / `live::command_clears_inherited_reporting_authority` | 0.107 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 16 / C16 | 16 | `ovrcr::settings_lifecycle` / `live::command_clears_inherited_reporting_authority` | 0.106 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 17 / C17 | 17 | `ovrcr::retained_capacity` / `live::command_clears_inherited_reporting_authority` | 0.103 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 18 / C18 | 18 | `ovrcr::retained_ownership` / `live::command_clears_inherited_reporting_authority` | 0.103 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 19 / C19 | 19 | `ovrcr::resource_cli` / `live::command_clears_inherited_reporting_authority` | 0.099 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 20 / C20 | 20 | `ovrcr::server_lifecycle` / `live::command_clears_inherited_reporting_authority` | 0.097 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 21 / C21 | 21 | `ovrcr::initial_prompt` / `live::command_clears_inherited_reporting_authority` | 0.093 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 22 / C22 | 22 | `ovrcr::operator_scope` / `live::command_clears_inherited_reporting_authority` | 0.086 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 23 / C23 | 23 | `ovrcr::pi_delegation` / `live::command_clears_inherited_reporting_authority` | 0.078 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 24 / C24 | 25 | `ovrcr::git_lifecycle` / `live::command_clears_inherited_reporting_authority` | 0.072 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 25 / C25 | 26 | `ovrcr::codex_initial_prompt` / `live::command_clears_inherited_reporting_authority` | 0.070 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 26 / C26 | 27 | `ovrcr::cli` / `live::command_clears_inherited_reporting_authority` | 0.068 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 27 / C27 | 28 | `ovrcr::client_startup` / `live::command_clears_inherited_reporting_authority` | 0.064 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 28 / C28 | 29 | `ovrcr::operator_e2e` / `live::command_clears_inherited_reporting_authority` | 0.057 | 2/3/0/3 | 2 | og-001 | tests/support/live.rs:568-579 |
| 29 / C29 | 30 | `ovrcr::tui` / `mouse::dragging_tracked_mouse_into_sidebar_releases_application_button` | 0.023 | 2/3/1/3 | 2 | og-003 | tests/tui/mouse.rs:527-551 |
| 30 / C30 | 31 | `ovrcr-tui` / `dashboard::palette::launch_tests::detected_omp_model_query_exposes_exact_selectors_and_initial_prompt_support` | 0.017 | 2/3/1/3 | 2 | og-017 | crates/ovrcr-tui/src/dashboard/palette.rs:12248-12354 |
| 31 / C31 | 32 | `ovrcr-tui` / `dashboard::render::harness_agent_label_tests::cli_created_claude_with_ovrcr_label_shows_claude_harness` | 0.015 | 1/3/1/4 | 1 | og-018 | crates/ovrcr-tui/src/dashboard/render.rs:1685-1698 |
| 32 / C32 | 33 | `ovrcr::tui` / `split::split_layout_geometry_and_narrow_fallback` | 0.013 | 2/3/1/4 | 2 | og-004 | tests/tui/split.rs:186-196 |
| 33 / C33 | 35 | `ovrcr-runtime` / `server::build_identity::tests::fingerprint_detects_byte_changes_with_same_length_and_preserves_open_build` | 0.013 | 2/2/1/4 | 2 | og-007 | crates/ovrcr-runtime/src/server/build_identity.rs:266-275 |
| 34 / C34 | 37 | `ovrcr-tui` / `dashboard::desktop::tests::desktop_confirmed_without_unread_does_not_queue` | 0.013 | 2/1/0/3 | 2 | og-015 | crates/ovrcr-tui/src/dashboard/desktop.rs:2083-2097 |
| 35 / C35 | 38 | `ovrcr-tui` / `dashboard::desktop::tests::desktop_health_loss_before_dispatch_cancels_pending_ready` | 0.012 | 2/3/1/3 | 2 | og-016 | crates/ovrcr-tui/src/dashboard/desktop.rs:1979-1998 |
| 36 / C36 | 39 | `ovrcr-protocol` / `settings::tests::theme_appearance_metadata_groups_every_existing_theme` | 0.011 | 1/2/0/3 | 2 | og-006 | crates/ovrcr-protocol/src/settings.rs:1974-1992 |
| 37 / C37 | 41 | `ovrcr-runtime` / `server::default_session_label_tests::direct_agent_executable_matches_kind_when_label_omitted` | 0.010 | 2/1/0/4 | 2 | og-008 | crates/ovrcr-runtime/src/server/mod.rs:3251-3262 |
| 38 / C38 | 42 | `ovrcr-runtime` / `server::default_session_label_tests::managed_claude_launcher_uses_agent_kind_not_ovrcr_argv0` | 0.010 | 2/2/1/4 | 2 | og-008, og-009 | crates/ovrcr-runtime/src/server/mod.rs:3209-3228 |
| 39 / C39 | 24 | `ovrcr-tui` / `dashboard::agents::tests::cursor_detection_uses_managed_launch` | 0.074 | 2/3/1/4 | 3 | og-013 | crates/ovrcr-tui/src/dashboard/agents.rs:321-356 |
| 40 / C40 | 34 | `ovrcr-protocol` / `auto_trust::tests::bare_managed_launches_gain_one_provider_flag` | 0.013 | 3/3/1/3 | 3 | og-005 | crates/ovrcr-protocol/src/auto_trust.rs:81-93 |
| 41 / C41 | 40 | `ovrcr-tui` / `dashboard::agents::tests::hermes_detection_uses_managed_launch_and_preserves_explicit_override` | 0.011 | 2/3/1/4 | 3 | og-013, og-014 | crates/ovrcr-tui/src/dashboard/agents.rs:454-501 |
| 42 / C42 | 36 | `ovrcr-runtime` / `server::delegation_files::read_tests::checkout_read_is_bounded_relative_and_never_follows_links` | 0.013 | 4/4/3/4 | 4 | og-010 | crates/ovrcr-runtime/src/server/delegation_files.rs:236-279 |
| 43 / C43 | — | `native/bridge/check-guards.py` / `client-extra-argv:ln56` [local-development] | unknown | 2/4/0/4 | 2 | og-019 | native/bridge/check-guards.py:56-56 |
| 44 / C44 | — | `native/bridge/check-guards.py` / `contract-argv[['--check-contract', '-1', str(wire)]]:ln53` [local-development] | unknown | 2/4/0/4 | 2 | og-020 | native/bridge/check-guards.py:53-55 |
| 45 / C45 | — | `native/bridge/check-guards.py` / `contract-argv[['--check-contract', str(schema), '4294967296']]:ln53` [local-development] | unknown | 2/4/0/4 | 2 | og-021 | native/bridge/check-guards.py:53-55 |
| 46 / C46 | — | `native/bridge/check-guards.py` / `contract-argv[['--check-contract']]:ln53` [local-development] | unknown | 2/4/0/4 | 2 | og-022 | native/bridge/check-guards.py:53-55 |
| 47 / C47 | — | `native/bridge/check-guards.py` / `contract-argv[['--unexpected']]:ln53` [local-development] | unknown | 2/4/0/4 | 2 | og-023 | native/bridge/check-guards.py:53-55 |
| 48 / C48 | — | `native/bridge/check-guards.py` / `opposite-profile-extra-argv:ln98` [local-development] | unknown | 2/4/0/4 | 2 | og-028 | native/bridge/check-guards.py:98-98 |
| 49 / C49 | — | `native/bridge/check-packaging.py` / `fixture-identity-accepted-production:ln114` [local-development] | unknown | 2/4/0/4 | 2 | og-030 | native/bridge/check-packaging.py:114-114 |
| 50 / C50 | — | `native/bridge/check-guards.py` / `contract-available:ln50` [local-development] | unknown | 3/4/0/4 | 3 | og-024 | native/bridge/check-guards.py:50-50 |
| 51 / C51 | — | `native/bridge/check-guards.py` / `contract-schema-newer:ln51` [local-development] | unknown | 3/4/0/4 | 3 | og-025 | native/bridge/check-guards.py:51-51 |
| 52 / C52 | — | `native/bridge/check-guards.py` / `contract-wire-newer:ln52` [local-development] | unknown | 3/4/0/4 | 3 | og-026 | native/bridge/check-guards.py:52-52 |
| 53 / C53 | — | `native/bridge/check-guards.py` / `entitlement-pin:ln35` [local-development] | unknown | 4/4/0/4 | 4 | og-027 | native/bridge/check-guards.py:28-35 |
| 54 / C54 | — | `native/bridge/check-packaging.py` / `entitlement-pin:ln32` [local-development] | unknown | 4/4/0/4 | 4 | og-029 | native/bridge/check-packaging.py:25-32 |

### Survivors and protection preserved

Keep every row below. The identities use the same runner/step conventions as the candidate table; all Python survivors are the **production** variant. `og-001` keeps the lexicographically first full execution ID, not the fastest observed execution. These are existing survivors, not future replacements.

| Survivor | Suite / test / variant | C | Source |
|---|---|---:|---|
| R01 | `ovrcr::automatic_titles` / `live::command_clears_inherited_reporting_authority` | 2 | tests/support/live.rs:568-579 |
| R02 | `ovrcr::cli` / `agent_run_adds_default_auto_trust_only_when_the_permission_family_is_absent` | 3 | tests/cli.rs:2164-2236 |
| R03 | `ovrcr::provider_quota` / `codex_native_read_reaches_remaining_bars_through_real_server_and_socket` | 2 | tests/provider_quota.rs:141-249 |
| R04 | `ovrcr::tui` / `mouse::leaving_the_rectangle_releases_at_last_valid_cell` | 2 | tests/tui/mouse.rs:500-524 |
| R05 | `ovrcr::tui` / `split::pane_rects_keep_split_geometry_and_hide_unfocused_tiny_pane` | 2 | tests/tui/split.rs:173-183 |
| R06 | `ovrcr-protocol` / `settings::tests::theme_catalog_matches_the_exact_approved_manifest` | 2 | crates/ovrcr-protocol/src/settings.rs:1995-2068 |
| R07 | `ovrcr-runtime::delegation_fallback_files` / `delegation_files::read_tests::checkout_read_is_bounded_relative_and_never_follows_links` | 4 | crates/ovrcr-runtime/src/server/delegation_files.rs:236-279 |
| R08 | `ovrcr-runtime` / `server::build_identity::tests::capture_covers_small_files_and_fixed_block_boundaries` | 2 | crates/ovrcr-runtime/src/server/build_identity.rs:278-291 |
| R09 | `ovrcr-runtime` / `server::default_session_label_tests::managed_pi_launcher_uses_agent_kind_not_ovrcr_argv0` | 2 | crates/ovrcr-runtime/src/server/mod.rs:3197-3206 |
| R10 | `ovrcr-tui` / `dashboard::agents::tests::detects_executables_then_shell_then_custom` | 2 | crates/ovrcr-tui/src/dashboard/agents.rs:241-287 |
| R11 | `ovrcr-tui` / `dashboard::agents::tests::managed_agents_launch_through_the_agent_run_route` | 3 | crates/ovrcr-tui/src/dashboard/agents.rs:359-451 |
| R12 | `ovrcr-tui` / `dashboard::desktop::tests::desktop_does_not_queue_when_unread_is_absent` | 2 | crates/ovrcr-tui/src/dashboard/desktop.rs:2072-2081 |
| R13 | `ovrcr-tui` / `dashboard::desktop::tests::desktop_queued_health_loss_then_recovery_stays_cancelled` | 2 | crates/ovrcr-tui/src/dashboard/desktop.rs:1925-1957 |
| R14 | `ovrcr-tui` / `dashboard::palette::launch_tests::detected_pi_model_query_exposes_exact_ids_and_keeps_its_binding` | 2 | crates/ovrcr-tui/src/dashboard/palette.rs:12133-12245 |
| R15 | `ovrcr-tui` / `dashboard::render::harness_agent_label_tests::cli_created_pi_with_ovrcr_label_shows_pi_harness` | 1 | crates/ovrcr-tui/src/dashboard/render.rs:1671-1682 |
| R16 | `native/bridge/check-guards.py` / `client-extra-argv:ln56` [production] | 2 | native/bridge/check-guards.py:56-56 |
| R17 | `native/bridge/check-guards.py` / `contract-argv[['--check-contract', '-1', str(wire)]]:ln53` [production] | 2 | native/bridge/check-guards.py:53-55 |
| R18 | `native/bridge/check-guards.py` / `contract-argv[['--check-contract', str(schema), '4294967296']]:ln53` [production] | 2 | native/bridge/check-guards.py:53-55 |
| R19 | `native/bridge/check-guards.py` / `contract-argv[['--check-contract']]:ln53` [production] | 2 | native/bridge/check-guards.py:53-55 |
| R20 | `native/bridge/check-guards.py` / `contract-argv[['--unexpected']]:ln53` [production] | 2 | native/bridge/check-guards.py:53-55 |
| R21 | `native/bridge/check-guards.py` / `contract-available:ln50` [production] | 3 | native/bridge/check-guards.py:50-50 |
| R22 | `native/bridge/check-guards.py` / `contract-schema-newer:ln51` [production] | 3 | native/bridge/check-guards.py:51-51 |
| R23 | `native/bridge/check-guards.py` / `contract-wire-newer:ln52` [production] | 3 | native/bridge/check-guards.py:52-52 |
| R24 | `native/bridge/check-guards.py` / `entitlement-pin:ln35` [production] | 4 | native/bridge/check-guards.py:28-35 |
| R25 | `native/bridge/check-guards.py` / `opposite-profile-extra-argv:ln98` [production] | 2 | native/bridge/check-guards.py:98-98 |
| R26 | `native/bridge/check-packaging.py` / `entitlement-pin:ln32` [production] | 4 | native/bridge/check-packaging.py:25-32 |
| R27 | `native/bridge/check-packaging.py` / `fixture-identity-accepted-production:ln114` [production] | 2 | native/bridge/check-packaging.py:114-114 |

The following preservation evidence comes from each confirmed group record. In particular, C≥3 removals retain the stated high-criticality invariant. The evidence must remain true at implementation time; if a survivor changes, re-review that group before removal.

- **og-001 (max C=2):** Same input/boundary: the single body tests/support/live.rs:567-579 removes the three inherited reporting environment variables and asserts their removal, and every listed target includes tests/support/live.rs verbatim (for example tests/automatic_titles.rs:1-2 for the retained execution and tests/claude_quota_probe.rs:1-3 for a candidate), with no cfg guard on the inclusion. Live::idle (tests/support/live.rs:111-149) and Live::command (tests/support/live.rs:163-196) have no cfg branch on this path, so the same production boundary and the same assertions (tests/support/live.rs:571-578) run in every binary. The 27 candidate executions are identical to the retained one; the receipt confirms the 27-way duplication.

- **og-011 (max C=2):** Same input/boundary: the native quota client argv and the absence of a --version probe, through the same client (crates/ovrcr-runtime/src/server/quota.rs:975-976). Candidate asserts the exact argv '-s read-only -a never app-server' and no probe across three version strings (crates/ovrcr-runtime/src/server/quota.rs:2194-2221). Retained asserts the same exact argv and probe absence through the real server and socket (tests/provider_quota.rs:229-249). Surviving assertions: exact argv and no version gating.

- **og-003 (max C=2):** Same input/boundary: a pressed application button dragged outside the focused pane, through forward_mouse -> release_held_leaving_rectangle (crates/ovrcr-tui/src/dashboard/state.rs:3447-3461). Candidate presses at (3,4) and drags to x=10 (tests/tui/mouse.rs:532-550); retained performs the same drag to inner.x-1 (tests/tui/mouse.rs:505-523). Both points are outside the focused terminal rect and both expect identical release bytes, so the same production branch and assertion survive.

- **og-008 (max C=2):** Same input/boundary: an Agent-kind session whose argv0 differs from the kind, through the same default_session_label Agent arm (crates/ovrcr-runtime/src/server/mod.rs:3177-3180). Candidate passes kind codex with argv ['codex'], where kind and basename coincide, so it cannot reject an argv0-derived defect (crates/ovrcr-runtime/src/server/mod.rs:3251-3262). Retained asserts kind-versus-argv0 with a discriminating argv for pi (crates/ovrcr-runtime/src/server/mod.rs:3197-3207). Surviving assertion: the label is the kind name when argv0 names the launcher.

- **og-017 (max C=2):** Same input/boundary: a model query for a detected preset exposing its exact selector and initial-prompt support, through the same launch-form path (crates/ovrcr-tui/src/dashboard/palette.rs:12133-12245). Candidate repeats the pi body with preset omp and its --auto-approve argv (crates/ovrcr-tui/src/dashboard/palette.rs:12248-12354). Retained asserts the same selectors and binding retention for pi (crates/ovrcr-tui/src/dashboard/palette.rs:12133-12245), with the remembered-explicit model assertion covered at crates/ovrcr-tui/src/dashboard/palette.rs:11939-12023. Surviving assertions: exact selectors, argv and binding retention.

- **og-018 (max C=1):** Same input/boundary: an Agent-kind session created through the CLI with the ovrcr label, through the same harness label branch (crates/ovrcr-tui/src/dashboard/render.rs:1470-1491). Candidate asserts harness_agent_name == "claude" and ("claude","") (crates/ovrcr-tui/src/dashboard/render.rs:1685-1698). Retained asserts the identical branch and tuple shape with pi (crates/ovrcr-tui/src/dashboard/render.rs:1671-1682). Surviving assertion: the harness name comes from the Agent kind, not the ovrcr argv label.

- **og-004 (max C=2):** Same input/boundary: the pane_rects layout at (120x40,focused=1), (80x24,focused=1) and (1x1,focused=1) (tests/tui/split.rs:187-195), through the same production function (crates/ovrcr-tui/src/dashboard/mod.rs:347-388) where focused is unused for wide geometry and the 1x1 area is empty regardless. The retained assertions cover (120x40,0), (80x24,1) and (1x1,0) (tests/tui/split.rs:174-182), exercising the same exact Rect values; the candidate adds no geometric boundary.

- **og-007 (max C=2):** Same input/boundary: a same-length byte rewrite in a file under the build tree, through the same production function executable_build (crates/ovrcr-runtime/src/server/build_identity.rs:47-105). Candidate rewrites byte 70000 of a 130000-byte file and asserts inequality only (crates/ovrcr-runtime/src/server/build_identity.rs:266-275). Retained rewrites the final byte at 3/65535/65536/65537/131079 and additionally asserts repeated-capture equality and the byte count (crates/ovrcr-runtime/src/server/build_identity.rs:278-291). Surviving assertions: fingerprint inequality on same-length rewrites plus capture repetition and byte-count stability.

- **og-015 (max C=2):** Same input/boundary: a Confirmed ResponseReady snapshot with unread None, through the producer gate that consults only unread (crates/ovrcr-tui/src/dashboard/desktop.rs:642-660). Candidate sets unread None plus Confirmed and asserts no queue (crates/ovrcr-tui/src/dashboard/desktop.rs:2083-2097). Retained uses the same snapshot(2,"a",ResponseReady) fixture with unread None and asserts no queue (crates/ovrcr-tui/src/dashboard/desktop.rs:2072-2081). Surviving assertion: a Ready without Unread is never queued.

- **og-016 (max C=2):** Same input/boundary: a queued Ready whose session then reports health Unavailable before dispatch, through observe_desktop_session's current-session gate (crates/ovrcr-tui/src/dashboard/desktop.rs:632-660). Candidate queues Ready, applies Unavailable, then asserts no host receipt (crates/ovrcr-tui/src/dashboard/desktop.rs:1979-1998). Retained performs the same observation and asserts pending empty plus no receipt after finishing the in-flight delivery (crates/ovrcr-tui/src/dashboard/desktop.rs:1925-1957). Surviving assertion: health loss cancels the pending desktop notification.

- **og-006 (max C=2):** Same input/boundary: the appearance metadata of every ThemeId, through the same settings theme table. Candidate hardcodes eight themes and their appearance (src/settings.rs:1974-1992); retained iterates ThemeId::ALL (80 themes) and compares every appearance() to the shipped approved manifest, including all eight (src/settings.rs:1995-2068). Surviving assertions: each theme's appearance class and the (54,26,4,27) totals; the candidate's eight rows are a subset of that matrix.

- **og-009 (max C=2):** Same input/boundary: the default label for an Agent session whose argv0 is not the harness name, through default_session_label's Agent arm that returns the kind name and never reads argv (crates/ovrcr-runtime/src/server/mod.rs:3177-3180). Candidate asserts only 'claude' for an alternate --provider spelling (crates/ovrcr-runtime/src/server/mod.rs:3209-3228). Retained asserts the same arm with the same discriminating shape for pi (crates/ovrcr-runtime/src/server/mod.rs:3197-3207). Surviving assertion: the label equals the Agent kind, not argv0.

- **og-013 (max C=3):** Same input/boundary: detection of the cursor-agent executable through the managed agent-run launcher and override lookup under the temp-stub/launcher fixture (crates/ovrcr-tui/src/dashboard/agents.rs:402-410). Candidate asserts the wrapped cursor-agent argv and the override argv (crates/ovrcr-tui/src/dashboard/agents.rs:321-356). Retained asserts cursor-agent's exact wrapped argv inside the MANAGED_AGENTS loop and runs the same apply_overrides lookup (crates/ovrcr-tui/src/dashboard/agents.rs:359-451,436-450). Surviving assertions: wrapped argv and explicit-override preservation for cursor-agent. The high-criticality invariant survives in the retained test: every MANAGED_AGENTS name is launched as `launcher agent run <name> -- <path>` with its auto-trust flag, so the launched agent keeps OVRCR ownership and reporting (crates/ovrcr-tui/src/dashboard/agents.rs:402-447).

- **og-005 (max C=3):** Same input/boundary: a bare managed launch through apply_default_auto_trust (src/auto_trust.rs:54-70). Candidate asserts the four bare-launch flags at src/auto_trust.rs:81-93; retained asserts the same flag injection end-to-end through the real `ovrcr agent run`, including '--' placement and family-present cases (tests/cli.rs:2164-2236). Surviving assertions: each managed provider gains exactly one default auto-trust flag and an explicit family suppresses it. The high-criticality invariant survives in the retained test: the injected flag decides whether the agent auto-approves actions, so a wrong injection changes the agent's safety posture (tests/cli.rs:2164-2236).

- **og-014 (max C=3):** Same input/boundary: detection and wrapping of a managed harness executable, including the non-executable 0644 skip through executable_in (crates/ovrcr-tui/src/dashboard/agents.rs:176-191). Candidate asserts hermes wrapping, override replacement and removal after chmod 0644 (crates/ovrcr-tui/src/dashboard/agents.rs:454-501). Retained asserts hermes' exact wrapped argv in the MANAGED_AGENTS loop (crates/ovrcr-tui/src/dashboard/agents.rs:359-451) and the same 0644 skip via executable_in in detects_executables_then_shell_then_custom (crates/ovrcr-tui/src/dashboard/agents.rs:241-287). Surviving assertions: wrapped argv, override replacement, and the non-executable skip. The high-criticality invariant survives in the retained test: every MANAGED_AGENTS name is launched as `launcher agent run <name> -- <path>` with its auto-trust flag, so the launched agent keeps OVRCR ownership and reporting (crates/ovrcr-tui/src/dashboard/agents.rs:402-447).

- **og-010 (max C=4):** Same input/boundary: a bounded relative read inside the delegated checkout, through ReadTree::read opening each component with O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC and still_at comparing dev/ino (crates/ovrcr-runtime/src/server/delegation_files.rs:172-230). Candidate asserts the positive nested read and thirteen negative names with still_at true then false after the checkout is renamed away (crates/ovrcr-runtime/src/server/delegation_files.rs:235-279); the retained second binary includes the identical file via #[path] (crates/ovrcr-runtime/tests/delegation_fallback_files.rs:5-6) and asserts the same positive plus thirteen negatives and the rename check (crates/ovrcr-runtime/src/server/delegation_files.rs:246-278). The high-criticality invariant survives in the retained execution: the checkout is the only file scope a delegated child is granted, so path escapes and symlink/link following remain rejected (crates/ovrcr-runtime/src/server/delegation_files.rs:33-60,172-230).

- **og-019 (max C=2):** Same input/boundary: the literal extra-argv call at native/bridge/check-guards.py:56, which passes an argv literal rather than client_arguments (native/bridge/check-guards.py:24-25) and drives the argv-count guard at native/bridge/main.swift:24-26 before any bundle field is read; candidate and retained differ only in the bundle selected at .github/workflows/ci.yml:55-63. Surviving assertions: the exact failed exit code, empty stderr and one JSON line.

- **og-020 (max C=2):** Same input/boundary: the literal --check-contract -1 <wire> call at native/bridge/check-guards.py:53-55, outside every mode branch and driving the profile-independent guard at native/bridge/main.swift:14-18; candidate and retained differ only in the bundle selected at .github/workflows/ci.yml:55-63, both built from the same sources by scripts/build-bridge.sh:96-105. Surviving assertions: the exact exit code, empty stderr, one JSON line and the status value.

- **og-021 (max C=2):** Same input/boundary: the literal --check-contract <schema> 4294967296 call at native/bridge/check-guards.py:53-55, outside every mode branch and driving the profile-independent guard at native/bridge/main.swift:14-18; candidate and retained differ only in the bundle selected at .github/workflows/ci.yml:55-63. Surviving assertions: the exact overflow exit code, empty stderr, one JSON line and the status value.

- **og-022 (max C=2):** Same input/boundary: the literal-argv call at native/bridge/check-guards.py:53-55, outside every mode branch (local-development only rewrites client_arguments at native/bridge/check-guards.py:24-25 and the forced-profile block at 75-98), driving the argv-count guard at native/bridge/main.swift:14-18 which reads no bundle profile; candidate and retained differ only in the bundle selected at .github/workflows/ci.yml:55-63. Surviving assertions: the exact exit code, empty stderr, one JSON line and the status value.

- **og-023 (max C=2):** Same input/boundary: the literal --unexpected call at native/bridge/check-guards.py:53-55, outside every mode branch and driving the argv-count guard at native/bridge/main.swift:14-18; candidate and retained differ only in the bundle selected at .github/workflows/ci.yml:55-63. Surviving assertions: the exact usage exit code, empty stderr, one JSON line and the status value.

- **og-028 (max C=2):** Same input/boundary: the literal [--client, --local-development, unexpected] call at native/bridge/check-guards.py:98; with four arguments main.swift:23-25 computes localDevelopment false and the count guard at native/bridge/main.swift:24-26 rejects before any profile branch, so the candidate (local bundle) and retained (production bundle) execute the same branch and assertion. Surviving assertions: the exact failed exit code, empty stderr and one JSON line.

- **og-030 (max C=2):** Same input/boundary: the fixture-identity acceptance at native/bridge/check-packaging.py:103-114, outside the if local_development block (indentation at line 115), called with local=False so no --local-development flag is passed (native/bridge/check-packaging.py:34-38) and with the fixture marker popped and identity fields overwritten (lines 107-113); both executions run identically. Surviving assertions: validate-bundle.py:44-88 accepts the marker, identity, executable/display names, entitlements and assets while the local-derived fixture is indistinguishable from the production one.

- **og-024 (max C=3):** Same input/boundary: the contract-availability guard at native/bridge/check-guards.py:50, outside every mode branch, driving the handshake at native/bridge/main.swift:14-21; the candidate and retained executions differ only in the bundle CI passes (.github/workflows/ci.yml:55-63), and both bundles come from the same swiftc invocation and sources with only Info.plist identity differing (scripts/build-bridge.sh:96-105,136-140) that this guard never reads. Surviving assertions: exact exit code, empty stderr, a single newline-terminated JSON line and the status value. The high-criticality invariant survives in the retained production execution: the contract handshake decides whether the Dashboard may use the bridge at all (native/bridge/main.swift:14-21).

- **og-025 (max C=3):** Same input/boundary: the schema-newer contract rejection at native/bridge/check-guards.py:51, outside every mode branch and reading no bundle profile (native/bridge/main.swift:14-21); candidate and retained differ only in the bundle selected at .github/workflows/ci.yml:55-63, both built from the same sources by scripts/build-bridge.sh:96-105. Surviving assertions: the exact incompatible exit code, empty stderr, one JSON line and its status. The high-criticality invariant survives in the retained production execution: silent contract drift would misparse bridge replies and notification state (native/bridge/main.swift:14-21).

- **og-026 (max C=3):** Same input/boundary: the wire-newer contract rejection at native/bridge/check-guards.py:52, outside every mode branch and reading no bundle profile (native/bridge/main.swift:14-21); candidate and retained differ only in the bundle selected at .github/workflows/ci.yml:55-63, both built from the same sources by scripts/build-bridge.sh:96-105. Surviving assertions: the exact incompatible exit code, empty stderr, one JSON line and its status. The high-criticality invariant survives in the retained production execution: silent contract drift would misparse bridge replies and notification state (native/bridge/main.swift:14-21).

- **og-027 (max C=4):** Same input/boundary: the standalone entitlement pin at native/bridge/check-guards.py:28-35, which has no mode reference; the local-development execution (candidate) and the production execution (retained) both copy the same entitlements file from one source (scripts/build-bridge.sh:141) and run that block byte-for-byte. Surviving assertions: file identity, the exact SHA256 pin, the exact entitlement dictionary and absence of the legacy Contents/ copy (native/bridge/check-guards.py:28-35). The high-criticality invariant survives in the retained production execution: a wrong or extra automation entitlement changes what the signed bridge may do on the user's behalf (native/bridge/entitlements.plist:1-5).

- **og-029 (max C=4):** Same input/boundary: the standalone entitlement pin at native/bridge/check-packaging.py:28-32 with no validate() call and no mode branch; the same copied entitlements source bytes reach both bundles (scripts/build-bridge.sh:141). Surviving assertions: file identity, the SHA256 pin, the exact dictionary and the legacy path's absence (native/bridge/check-packaging.py:25-32). The high-criticality invariant survives in the retained production execution: the pin is what codesign seals into the shipped app (native/bridge/validate-bundle.py:59-65).

## Improve rows and expensive rows with low C/S scores

### All eight improve rows

The eight rows total **0.251 s** (0.189 s nextest and 0.062 s Node) in `../tests.csv`. None justifies pruning now. Improve the missing assertion or boundary first; only prune later if a named surviving test proves complete subsumption. Improving a test need not create an additional runner case: the assertion belongs in its existing case unless a distinct boundary requires one. No runtime saving is booked for these changes, and a stronger test may cost more.

| Row | C/S/U/F | Seconds | Source | Decision and protection at stake |
|---|---|---:|---|---|
| `tests/pi_workflow_tracker.mjs` / `lost create reply is attributable through ordinary issue list without a duplicate publication` | 1/3/2/0 | 0.062 | tests/pi_workflow_tracker.mjs:21-31 | Fix fidelity first; retain the fixture contract separately. Drive the production tracker boundary with the real requested issue-list projection and assert attributable publication without a second create. The current test launches only the stub and reads its ledger; the requested projection omits blockedBy, which the stub still returns. Removing it now loses lose-once stub qualification, not production duplicate-publication protection. Do not claim production coverage until added. |
| `ovrcr-tui` / `dashboard::palette::mouse_tests::mouse_form_focus_unicode_edit_and_cancel` | 2/1/1/3 | 0.047 | crates/ovrcr-tui/src/dashboard/palette.rs:9059-9071 | Fix assertions first; retain. Assert the field value and cursor after click, paste, Backspace and Delete, before Cancel. The current test can pass with broken Unicode editing because it only checks capture and closing. Removing it would also discard the existing cancel path. |
| `ovrcr::bin/ovrcr-gui` / `terminal_fonts_render_mockup_glyphs_or_default_symbols` | 1/1/2/3 | 0.046 | src/bin/ovrcr-gui.rs:158-186 | Fix first; retain. Use a controlled font directory and assert distinct per-style font families, then paint a status cell. The current machine-dependent glyph set can collapse to one braille glyph. Removing it now loses the existing atlas/fallback smoke check; no confirmed equivalent survivor exists. |
| `ovrcr::tui` / `mouse::terminal_mode_keeps_dashboard_mouse_capture_without_a_ready_session` | 2/1/1/2 | 0.031 | tests/tui/mouse.rs:434-451 | Fix state and assertion first; retain. Establish an actual unready session in Terminal mode and assert mouse_capture_required plus the exact allowed action (Redraw/None as applicable). The present empty-dashboard fixture and inequality permit non-empty PTY bytes or Detach. Pruning now loses even the narrow empty-byte negative check and leaves the claimed state unprotected. |
| `ovrcr::cli` / `dashboard_selection_frames_skip_only_quota_telemetry` | 1/2/2/1 | 0.026 | tests/cli.rs:1425-1492 | Fix fidelity first; retain helper qualification. The socket-pair sequence precisely checks the local filter, but never drives a production server. Add an attach-path assertion for ordered non-quota frames. Do not prune its interleaved-quota filter check merely because it is test infrastructure. |
| `ovrcr::task_tui` / `unicode_prompt_render_is_safe_while_another_field_has_focus` | 2/1/2/3 | 0.016 | tests/task_tui.rs:623-630 | Fix assertions first; retain. Assert intact Prompt text, focused Name field and cursor in the rendered buffer. Drawing already catches a panic; deleting this smoke test before replacement loses that particular Unicode/nonfocused render input. |
| `ovrcr::cli` / `quota_filter_preserves_other_events_and_responses` | 1/2/2/1 | 0.013 | tests/cli.rs:1495-1543 | Fix fidelity first; retain. Exercise the real attach boundary while retaining the six exact local pass-through variants. This case and the preceding one share a helper but assert different frame sets; there is no confirmed full subsumption. |
| `ovrcr-runtime::settings_matrix` / `generator_spells_near_misses` | 1/3/3/1 | 0.010 | crates/ovrcr-runtime/tests/settings_matrix.rs:569-574 | Improve the coverage contract first; retain exact helper examples. Add an independent expected set of key segments and enum near-miss cases consumed by the loader matrix. The helper test already asserts exact outputs, including equal-neighbor handling. Removing it now lets a generator regression silently reduce loader coverage; no redundancy group proves it dispensable. |

### Expensive rows selected by C≤2 and S≤2

The requested score filter selects **156 nextest rows totaling 24.405 s**, including repeated fixture executions; it is not a finding that all 156 are disposable. For a reproducible high-cost definition, the 90th-percentile cutoff of all 2,928 timed, non-child rows in `../tests.csv` is **6.280 s** (ascending array, zero-based index `floor(0.90*n)`). One score-filtered row exceeds that cutoff. To avoid hiding moderately costly weak assertions, the table also includes **every score-filtered row ≥1 s**: five rows totaling **19.273 s**, 78.97% of that filter's measured cost. The next row is only 0.357 s. These five are all currently `keep`, and none belongs to a confirmed overlap group. Scores C=2/S=2 can describe a useful happy-path integration check; they do not establish a pruning case.

| Row, descending seconds | C/S/U/F | Seconds | Source | Decision and exact loss if removed |
|---|---|---:|---|---|
| `ovrcr-runtime` / `server::tests::cli_created_agent_label_uses_kind_not_launcher_argv0` | 2/2/2/3 | 6.371 | crates/ovrcr-runtime/src/server/tests.rs:8875-8952 | Retain; optimize fixture/cleanup, not assertions away. Exact kind-derived labels and unchanged names for Pi, Claude and Terminal are already checked through create_session. Pure label tests do not prove that integration path. A future cheaper integration fixture must keep all three comparisons; pruning now loses composition coverage. The test launches three sleep children and explicitly terminates them with a 200 ms budget, but this single timing does not isolate the cost of those operations. |
| `ovrcr::cli` / `context_helper_selects_session_from_executable_directory_alias` | 2/2/2/4 | 4.788 | tests/cli.rs:1274-1300 | Strengthen first, then optimize its Live fixture. Assert the selected session and context identity, not only a successful Hierarchy reply. It is the only reviewed alias-path startup case. Deleting it loses executable-directory symlink normalization through startup/select; ordinary path coverage is not equivalent. |
| `ovrcr::provider_quota` / `providers_remain_visible_before_any_native_quota_source_reports` | 2/2/2/4 | 3.433 | tests/provider_quota.rs:837-874 | Retain; consider a smaller initial-snapshot fixture only if the real initial hello boundary remains covered. It already checks four exact visible labels after the first server frame. The distinct protection is provider visibility before native data, not a generic successful render. No confirmed survivor permits pruning. |
| `ovrcr::resource_cli` / `remove_project_reports_workspaces_remain` | 2/2/2/4 | 3.395 | tests/resource_cli.rs:749-763 | Retain; strengthen to assert the project/workspaces remain unchanged, then compare the entire direct-CLI error contract before considering consolidation. It checks exit 1, WorkspacesRemain and message text. The CSV notes overlap with an end-to-end removal case but names no independently confirmed group; deleting it now loses this exact CLI diagnostic contract. |
| `ovrcr::server_lifecycle` / `dashboard_request_id_zero_does_not_block_followup_response` | 2/2/3/4 | 1.286 | tests/server_lifecycle.rs:6275-6305 | Fix the assertion first. Drain/identify initial attach events and require the correlated request_id=1 List/Hierarchy response; read_frame.is_ok alone could accept an unrelated frame. It is the sole reviewed zero-ID dashboard case. Pruning now removes the only boundary check rather than removing proven duplication. |

The honest result is **no new prune-now rows**. The entire 156-row low-C/low-S filter costs only 24.405 process-s; even deleting all of it (which this report rejects because it would lose distinct coverage) would represent only 8.135/6.101 s in the three/four-slot work model. Decisions about those rows are therefore a coverage question, not a route to minutes saved. The 54 independently confirmed consolidations are the removal portfolio. Low signal without a retained equivalent calls for a better test, not removal. The ≥1 s weak rows, the eight improve rows and the consolidation set are three separate accounting sets; do not add them as hypothetical savings merely because they appear in this report.

## Observed retries and what would establish flakiness

`../evidence/attempts.csv:342-345,717-720` records two failures followed by successful second attempts. `.config/nextest.toml:7-13` permits two retries, but each used only one. The following numbers count attempts once and omit the repeated final FLAKY summaries.

| Test | First failed attempt | Successful retry | Total process seconds | Elapsed from failure log to retry-pass log | C/S/U/F |
|---|---:|---:|---:|---:|---|
| `ovrcr::codex_initial_prompt / codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path` | 4.855 | 4.372 | 9.227 | 4.376512 s | 4/4/4/3 |
| `ovrcr::pi_delegation / parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects` | 24.507 | 21.743 | 46.250 | 21.746746 s | 3/4/4/4 |
| Sum | 29.362 | 26.115 | 55.477 | 26.123258 s across two separate intervals | — |

The timestamp intervals are computed from the raw completion lines: Codex `00:34:39.0325860Z` to `00:34:43.4090980Z`; Pi `00:50:58.2534000Z` to `00:51:20.0001460Z`. They include a few milliseconds outside measured retry execution. They are **local retry-chain elapsed time, not additive job-wall delay**: other tests run concurrently in both intervals.

There are two different counterfactuals. Relative to stopping after the observed failed attempts, the retry policy added **26.115 test-process seconds**, but produced a passing result instead of a failed job. This is not a reason to disable retries. Relative to an otherwise identical run that passed on its first attempt at the observed successful-attempt speed, the avoidable failed work is **29.362 s**. The latter assumes successful-attempt duration is representative, which one run cannot establish. These two figures describe overlapping attempts and must never be added.

The hosted log cannot identify either counterfactual's exact marginal wall time. A fixed-duration, non-regressing work-conserving schedule model would allow zero to roughly 26.123 s of extra wall time for the actual retry chains, depending on critical-path placement. A balanced slot model spreads 26.115 s over three/four threads as **8.705/6.529 s**; removing failed-attempt work instead gives **9.787/7.341 s**. These are alternative estimates, not measured savings or guaranteed bounds for a different runner. Scheduling, contention and successful-attempt variation can change them. Keep both tests: their protected boundaries are the privacy/lifecycle path at `tests/codex_initial_prompt.rs:322-506` and partial-effect recovery at `tests/pi_delegation/recovery.rs:801-911` (scores/source mapping in `../tests.csv`).

### Repeated-run measurement design

A fail/pass pair is observed recovery, not a population flake rate or proof of test-code fault. Reproduce under an unchanged pinned test/product revision and fixture inputs. Classify the failure assertion and captured state before calling it nondeterministic. Distinguish a product race, a test race, an external service failure and a hosted-runner failure. A production race remains a defect even if rare.

1. Start with 30 independent hosted runs across fresh jobs to estimate ordinary duration spread and detect common failures. Preserve the normal 3/4-vCPU and nextest scheduling environment; record the actual runner shape rather than inferring it from macos-latest. A focused repeat on one warm machine is a separate experiment, not a replacement for hosted independent jobs.
2. For each of the two tests, collect at least **299 independent first-attempt observations** to have a 95% chance of seeing ≥1 failure if the true failure probability is 1%. To detect 0.1% at the same confidence requires **2,995 observations**. This uses `n=ceil(log(0.05)/log(1-p))` and assumes independent, identically distributed trials. Correlated repeats on one runner reduce the effective sample size. These counts are detection targets, not a requirement to burn 299 full-suite runs: targeted runs can locate causes, followed by full-suite confirmation of contention-sensitive behavior.
3. Record run/job/revision, OS/architecture, vCPU and nextest thread count, nextest version/filter/profile, test ID, invocation, attempt number, result, monotonic start/end/duration, queue delay and retry delay, failure signature, stdout/stderr and preserved fixture artifacts. Record seed/input order, cold/warm cache, resources held, concurrent test set and any exclusivity. Keep first attempts as the denominator; retries are conditional on a failure and are not independent trials.
4. Report first-attempt failures / independent observations with an exact binomial interval, recovered failures, hard failures and failure-signature counts separately. Report median/p95 durations, total failed-work seconds, retry-work seconds and whole-job wall time. With zero failures, the one-sided 95% upper probability is `1-0.05^(1/n)` (about 1% at 299, not zero). A 30-run clean sample still allows about 9.5% under the same model.
5. To price a hardening change, compare paired or randomized baseline/treatment jobs with the same selection and runner shape. Require the behavior assertions to stay intact and compare whole-run wall-time distributions. Do not infer a precise saving from two recovered failures in one job.

## Rules that prevent double counting

1. Join on full `test_id` and invocation, never on a short function name. The 54 candidate IDs are the union of group members minus all designated survivors. Sum this union once. The group ledger has **56 candidate memberships but 54 unique candidates** because `og-008/009` and `og-013/014` overlap.
2. Assign a removed test its observed full removal value once. If the same test is a sleep/fixture/process hotspot, its mechanism saving is **zero after removal**. Apply mechanism deltas only to retained test IDs and only to the portion of their duration actually changed.
3. `og-001` removes **27 of 28** executions. Their measured durations sum to 3.345 s; all 28 sum to 3.446 s and the retained automatic_titles execution is 0.101 s. A uniform-body model must use **27 × body cost**, not 28. Because whole-process startup varies, the measured ledger uses the individual 27 durations rather than `27 × 0.101`. Removing the test case does not remove every Live fixture setup in the binary or remove the binary's compilation.
4. Keep all named survivors, including both in `og-014`. No last protector of a C=3/4 invariant may be removed. A future refactor must recheck actual boundary equivalence rather than inherit an old group label.
5. Charge retries once. `total_attempt_seconds` already includes failures and successful retries. Never add the attempt ledger, the CSV total and the final FLAKY summary. Do not add the 26.115 s retry-policy cost to the 29.362 s failed-work counterfactual. If a mechanism change accelerates the retried test, book its retained-work delta first, then any separate empirically demonstrated failure-rate change against the remaining work.
6. Mechanism sets also overlap each other. A shared-fixture improvement, test-specific sleep reduction and cheaper cleanup cannot each claim the full test duration. For each retained ID, price disjoint time intervals or a measured combined delta; cap the total at that test's baseline duration. An event wait replacing a fixed sleep still costs its observed signal latency; an assertion-required observation window is not free savings.
7. Keep process work separate from wall time. Divide parallel nextest work only under an explicit slot model. Add genuinely sequential Python/package wall-time deltas directly, not divided by nextest threads. Do not mix CI compile, cache or runner setup savings into test-execution savings. They have separate step baselines.
8. Keep parent/child durations separate. The four installed-Pi Node child durations are already inside the two explicit Rust parent runs. Do not add both. Likewise, do not add suite wall time to the per-check times of checks inside it.
9. Scheduling changes reprice the same remaining workload. Compute removal and mechanism effects first, then schedule the remaining tasks. Do not add a claimed three-to-four-thread gain to the same work reduction's old three-thread conversion. Run-alone tests reserve every slot (`.config/nextest.toml:15-22`); their wall delta may behave as a serial interval rather than `seconds/threads`.
10. Consolidation may preserve a binary for its other tests, so it does not automatically save compile/link time, a whole suite setup, or all helper consumers. Better assertions are coverage work, not negative runtime. Unknown Python per-check time stays unknown until measured; never assign each one an equal fraction of a suite duration.

## Ceiling arithmetic and assumptions

### Consolidations alone

The main nextest baseline is **P=5,931.968 process-seconds** and **T=2,014.918 elapsed seconds** (`../evidence/attempts.csv`, `../evidence/cost-analysis.json:15-35`). The measured average occupancy is P/T=**2.944 concurrent test processes**, not evidence of either a three-core or four-core runner and not a CPU utilization measurement. The full job is **2,418 s** from `../evidence/run.json` job timestamps. The **2,415 s** step-covered span in `../evidence/job-steps.csv:2-24` and the older `cost-analysis.json` label are different boundaries. Compilation inside Test is 136 s (`../evidence/job-steps.csv:11`); no consolidation compile saving is assumed.

Use three and four **test slots** as alternative utilization models for the possible 3/4-vCPU hosted shape. A slot runs a process that can wait on I/O; a slot is not a core-second. Four exclusive tests consume **E=37.240 s** in total (sum their rows in `../tests.csv`; exact selection at `.config/nextest.toml:15-22`). They are not consolidation candidates. A work-floor model that honors exclusivity is:

`modeled T(k) = E + (P-E)/k + R(k)`

Choose the residual R(k) to match the observed T once: **R(3)=12.769 s**, **R(4)=503.996 s**. These residuals are unexplained model remainders, not proven waste or an independent scheduling opportunity. Hold E, R, all retained durations and compilation constant when converting a nonexclusive work reduction S to an estimate `S/k`. This is a conditional amortized estimate; the critical path, queue placement and contention can make actual savings different. The entire removal portfolio is much too small to claim a precise end-to-end speedup from this run.

| Quantity | Three-slot model | Four-slot model |
|---|---:|---:|
| Baseline work/exclusivity floor, E+(P-E)/k | 2,002.149 s | 1,510.922 s |
| Observed nextest T used to calibrate the residual | 2,014.918 s | 2,014.918 s |
| Unique timed consolidation work removed | 4.344 process-s | 4.344 process-s |
| Amortized nextest wall reduction S/k | **1.448 s** | **1.086 s** |
| Conditional nextest wall after consolidation | 2,013.470 s | 2,013.832 s |
| Conditional whole-job wall, keeping every other step fixed | 2,416.552 s | 2,416.914 s |

The timed reduction is **0.0732%** of cumulative nextest process time; nextest work would fall to **5,927.624 s**. Under unchanged measured occupancy rather than ideal k slots, the same arithmetic is **1.476 s** of wall time. Under an ideal fixed-duration monotone schedule, removing 4.344 s of nonexclusive tasks could affect the wall by anywhere from nearly zero to 4.344 s depending on placement; neither S/3 nor S/4 is a universal upper bound. There is no measured removal run.

For **all 54 candidates**, keep the twelve untimed Python checks separate. Let **U** be their combined *sequential check-elapsed time* inside the relocated-assets step. Then:

- Known removed nextest work is **4.344 process-s**; removed Python check time is **U, unknown**. No complete numerical process-time total for all 54 is supported by the evidence.
- Conditional whole-job reduction is **1.448 + U s** at three slots, or **1.086 + U s** at four. Python checks are outside nextest and do not get divided by its thread count.
- `../evidence/job-steps.csv:13` measures the entire relocated-assets step at **82 s**, including builds and all retained checks. Thus 0≤U≤82 s is a deliberately loose enclosing step-time bound, not an allocation of the step to these twelve checks and not a claim that 82 s is removable. The corresponding conditional model ranges are **1.448–83.448 s / 1.086–83.086 s**; actual savings may be near zero if removals miss the critical path. Preserving the rest of that step makes the upper endpoints unattainable in practice. Measure these checks individually before reporting a useful all-54 estimate.

### Consolidations plus mechanism changes

For the combined plan, let D be the 42 timed nextest candidate IDs. For each retained nextest ID i, let m(i) be the *measured or explicitly hypothesized* reduction from the combined mechanism changes on that ID, bounded by its original elapsed duration. Set m(i)=0 for i in D. Separate the retained deltas into nonexclusive M and exclusive Q. The candidate set contains no exclusive test. The accounting identities are:

`removed nextest work = 4.344 + M + Q`

`conditional nextest wall saving(k) = (4.344 + M)/k + Q`

`conditional job wall saving(k) = (4.344 + M)/k + Q + U + V`

Here U is the untimed Python consolidation saving above, and V is any *disjoint measured sequential* mechanism saving outside nextest. Cap U+V at the time actually measured for the affected sequential intervals; never reuse the 82 s step envelope for both. If a scheduling change is proposed, recompute the remaining schedule instead of adding a second utilization dividend. These formulas hold the calibrated residual constant and therefore remain estimates, not guarantees.

The quantities **M, Q, U and V are unidentified by the available single-run evidence**. Whole-test durations identify the work at stake, not the removable setup/sleep/cleanup fraction. No phase-timing ledger supports a numeric sum of mechanism savings. Therefore the defensible combined ceiling is conditional, rather than a fabricated precise speedup:

| Portfolio | Removed nextest process work | Three-slot nextest wall reduction | Four-slot nextest wall reduction | Additional sequential job-wall reduction |
|---|---:|---:|---:|---:|
| Confirmed timed consolidations only | 4.344 s | 1.448 s | 1.086 s | U, unknown for twelve Python candidates |
| Consolidations plus retained mechanism changes | 4.344 + M + Q | **1.448 + M/3 + Q** | **1.086 + M/4 + Q** | **U + V**, unknown |
| Combined whole-job prediction, holding residual and other work fixed | — | **2,416.552 − M/3 − Q − U − V** | **2,416.914 − M/4 − Q − U − V** | Included in preceding cells |

For a range, use the measured confidence bounds for M, Q, U and V in those formulas, with overlap removed at the test-ID and phase level. Until phase measurements exist, their evidence-supported lower removable-work bound is zero; there is no useful numerical coverage-preserving upper bound beyond each affected test/step's original duration. The absolute nextest work envelope is P=5,931.968 s, but erasing all retained test time would erase the behavior under test. It is not a mechanism-fix recommendation or an achievable savings ceiling.

The companion `mechanisms.md` reaches the same measurement limit: it supplies conditional setup/observer envelopes, not measured removable phase time. Do not sum those envelopes into M or Q. The combined plan therefore has **4.344 s of measured consolidation work plus an unquantified mechanism opportunity**, with U still unknown for the Python checks. In particular, a whole-test duration, a protected behavioral timeout, and an exclusive scheduling penalty are not interchangeable with safely removable mechanism time.

The final plan should carry the mechanism rows as experiments until each has a named retained-ID set, a disjoint phase delta, preservation assertions and a baseline/treatment wall-time comparison. It may report the 4.344 s consolidation work now; it must not add all of a hotspot's elapsed time merely because a source line contains a sleep, process spawn or polling loop.

## Implementation and measurement boundary

This report changes no tests, product source, workflows or configuration. It recommends keeping all listed survivors and strengthening distinct weak protection before considering further removal. Every quoted observed duration comes from the frozen CSV/attempt ledger or a named CI step. The source citations explain behavior and preservation, not measured phase costs. No build, test suite, Python check, Node fixture or formatter was executed for this analysis.

To turn any mechanism estimate into an accepted saving, collect per-phase monotonic times and per-attempt scheduling intervals for the retained test IDs. Then run the complete unchanged selection before and after the candidate optimization across comparable hosted jobs, with actual runner slots recorded. Compare both coverage assertions and wall-time distributions. Removal of the last C=3/4 protector, deletion of assertions, wholesale test disabling, and shortening a race-sensitive observation window without an equivalent deterministic ordering proof are outside this portfolio.
