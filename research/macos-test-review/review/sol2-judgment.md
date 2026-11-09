reviewer_model: openai-codex/gpt-6.1-sol

| test_id | C/S/U/F | recommendation | verdict | one-line reason |
|---|---|---|---|---|
| `nextest\|Test\|ovrcr%3A%3Atask_tui\|unicode_prompt_render_is_safe_while_another_field_has_focus\|default` | 2/1/2/3 | improve | DISAGREE | 1. Draw success catches a panic, but no buffer or cursor assertion can detect mere misalignment. |
| `nextest\|Test\|ovrcr%3A%3Atui\|mouse%3A%3Aterminal_mode_keeps_dashboard_mouse_capture_without_a_ready_session\|default` | 2/1/1/2 | improve | DISAGREE | 2. Only the exact empty PtyBytes value is excluded; nonempty/partial PTY bytes still pass. |
| `nextest\|Test\|ovrcr-runtime%3A%3Asettings_matrix\|generator_spells_near_misses\|default` | 1/3/3/1 | improve | AGREE | 3. Exact helper outputs catch omitted/duplicate near misses; fixture-only F=1 correctly leads to improve. |
| `nextest\|Test\|ovrcr-tui\|dashboard%3A%3Apalette%3A%3Amouse_tests%3A%3Amouse_form_focus_unicode_edit_and_cancel\|default` | 2/1/1/3 | improve | DISAGREE | 4. Rendered-text lookup and Cancel are observed, but a wrong grapheme insertion/deletion can still pass. |
| `nextest\|Test\|ovrcr%3A%3Atask_tui\|destructive_actions_require_confirmation_and_escape_cancels\|default` | 3/2/0/3 | consolidate | DISAGREE | 5. The retained matrix recreates the view for each answer; it misses cancel-then-confirm on one view. |
| `nextest\|Test\|ovrcr-protocol\|codec%3A%3Atests%3A%3Aappended_resource_requests_round_trip\|default` | 3/2/1/3 | keep | AGREE | 6. Exact round trips catch loss of text/run fields; happy-path S=2 and decode-only distinction are fair. |
| `nextest\|Test\|ovrcr-protocol\|codec%3A%3Atests%3A%3Asplit_view_frames_round_trip\|default` | 3/2/1/3 | keep | AGREE | 7. Exact SetView message equality protects decoding; the single positive case supports S=2. |
| `nextest\|Test\|ovrcr-runtime\|session%3A%3Atests%3A%3Ashell_round_trip_updates_the_current_screen\|default` | 3/2/1/4 | keep | AGREE | 8. Real shell output must reach the screen; one marker merits S=2, not a precise parser invariant. |
| `python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-guards.py\|client-op%5B%7B%27type%27%3A%20%27deliver%27%2C%20%27title%27%3A%20%27private%20prompt%27%2C%20%27subtitle%27%3A%20%27x%27%2C%20%27body%27%3A%20%27x%27%7D%5D%3Aln65\|production` | 3/2/1/4 | keep | DISAGREE | 9. Missing navigation and invalid title mask each other; the failed reply cannot isolate either guard. |
| `python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-packaging.py\|source-accepted%3Aln44\|local-development` | 3/2/1/4 | keep | DISAGREE | 10. The pristine local bundle must pass; accepting a wrong-profile bundle is not tested by this invocation. |
| `nextest\|Test\|ovrcr%3A%3Agit_lifecycle\|forced_dirty_removal_keeps_generation_and_root_guards\|default` | 4/4/4/4 | keep | AGREE | 11. Force succeeds only on the owned dirty tree; repository root and replacement identity remain protected. |
| `nextest\|Test\|ovrcr%3A%3Agit_lifecycle\|refuses_registry_and_git_path_disagreement\|default` | 4/4/4/4 | keep | AGREE | 12. Refusal and safe stale-record cleanup both preserve the actual Git-listed tree. |
| `nextest\|Test\|ovrcr%3A%3Agit_lifecycle\|refuses_replacement_worktree_with_different_identity\|default` | 4/4/4/4 | keep | AGREE | 13. Inspect and removal reject a real replacement identity and preserve its path; destructive C=4 is justified. |
| `nextest\|Test\|ovrcr%3A%3Agit_lifecycle\|refuses_symlink_substitution_without_touching_either_worktree\|default` | 4/4/4/4 | keep | AGREE | 14. Real symlink substitution is refused while both trees and both registrations survive. |
| `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_send_crash_and_discard_boundaries_preserve_uncertainty_and_exact_late_identity\|default` | 3/4/4/3 | keep | AGREE | 15. Crash/discard matrix checks Send counts, durable uncertainty, run-id presence and payload deletion. |
| `nextest\|Test\|ovrcr%3A%3Abin%2Fovrcr\|cli%3A%3Adelegation_publisher%3A%3Atests%3A%3Astdio_has_only_framed_json_and_oversized_or_truncated_lines_have_no_effect\|default` | 3/4/3/3 | keep | DISAGREE | 16. Invalid initialize/non-JSON inputs never attempt publication; they do not prove effectful-call admission safety. |
| `nextest\|Test\|ovrcr-tui\|dashboard%3A%3Aview_handshake%3A%3Atests%3A%3Achange_revokes_readiness_and_stale_responses_are_ignored\|default` | 3/4/3/4 | keep | AGREE | 17. Granted/revoked state and both stale-response refusals exercise the real readiness state machine. |
| `python-check\|Relocated%20macOS%20startup%20assets\|native%2Fbridge%2Fcheck-packaging.py\|bundle-copy-rejected%5B%27display%27%5D%3Aln47\|production` | 3/3/3/4 | keep | DISAGREE | 18. This mutation checks display-name metadata, not an executable/content or destructive safety boundary; C=2 fits. |
| `node-test\|Extension%20tests\|tests%2Fpi_reporting_extension.mjs\|queue%20overflow%20disables%20reporting%20and%20sends%20one%20content-free%20unavailable%20notice\|default` | 3/3/3/3 | keep | AGREE | 19. Production queue overflow emits the overflow notice, limits helper completions and remains disabled. |
| `cargo-test\|Installed%20Pi%20effective%20child%20tools%20through%20production%20assignment%20argv\|ovrcr%3A%3Alib\|coding%3A%3Aproduction_assignment_argv_preserves_installed_sdk_child_publication_tools\|production` | 3/4/4/4 | keep | AGREE | 20. Real installed parser/loader/AgentSession asserts exact active/configured child tools for both assignments. |
| `nextest\|Test\|ovrcr%3A%3Atui\|palette%3A%3Afailed_reopen_retries_the_current_run_from_the_confirm\|default` | 3/3/3/3 | keep | DISAGREE | 21. A retained test already pins run-1/run-4 retry and the false acknowledgement flag; the entry/error-path distinction is small. |
| `nextest\|Test\|ovrcr-runtime\|server%3A%3Aquota%3A%3Atests%3A%3Anative_error_reason_is_classified_without_its_body\|default` | 4/4/2/3 | keep | DISAGREE | 22. Exact redacted error classifications are strong, but zero Retry-After and an expired buffered reply are absent. |
| `nextest\|Test\|ovrcr%3A%3Atui\|agent_search%3A%3Ashared_control_jump_binding_appears_in_footer_key_popup_and_palette\|default` | 1/2/3/3 | keep | AGREE | 23. The real shared binding appears on three rendered surfaces; discoverability C=1 and S=2 fit. |
| `nextest\|Test\|ovrcr-runtime\|codex_delegation%3A%3Atests%3A%3Acatalog_keeps_exact_listed_choices_and_refuses_malformed_or_oversized_data\|default` | 2/4/3/4 | keep | DISAGREE | 24. The oversized whitespace is invalid JSON regardless of the byte bound, so bound-removal can pass. |

## Selection and review boundary

Reviewed the current `tests.csv`, SHA-256 `5b1a9803d927888181554b3ac3b13c1a1bf469ebd2725a9b8ea2fcf0df7a75fd`. The previous report was used only to extract its 24 table IDs for exclusion, not to adopt its judgments. The model name above is the model exposed in this task's workstation context.

The current file has eight improve rows, but four appear in the previous table. The coordinator approved this adjustment: **4 eligible improve rows** (rows 1–4), **6 lowest-usefulness C>=3 rows** (5–10), **4 highest-retention rows** (11–14), and **10 smallest-SHA-256 remaining IDs** (15–24). Each stratum excludes earlier selections and all previous-table IDs. Ties in score strata use ascending full test_id. Hashes use the full UTF-8 test_id, not its displayed Markdown escaping. The hash stratum starts at `0071b8f10da90231bc0eff6d0c8cb0b780a3a60020a2d930ab80002ee5f66937` and ends at `01279c0e133d9da2f40e4c213f2b2c8d0ed1e09b4d256bf59cc3feec851d1252`.

All 24 defining bodies and their cited production ranges were read, together with relevant assertion/fixture helpers. The sole consolidate row's named retained counterpart was read in full. Source inspection, CSV parsing and deterministic sample calculation were performed; no product tests, fixtures, builds, formatters or report validator were run. Proposed scores below are C/S/U/F. Table scores and recommendations are the existing CSV values. AGREE concerns the requested behavioral/value judgment, not an exhaustive provenance audit.

## Disagreements

### Row 1 — NIT: render alignment is unobserved

`tests/task_tui.rs:623-630` stores Unicode, enters editing and draws once. It reads neither buffer nor cursor. A renderer that draws the prompt at the wrong position, omits it, or misplaces the Name cursor can pass, provided it does not panic. The relevant separate prompt/name paths are `crates/ovrcr-tui/src/task_tui.rs:1271-1356`.

Proposed **2/1/2/3, improve**, unchanged. Narrow failure_caught to a panic/draw error for this stored Unicode/nonfocused-field combination. Its own signal/recommendation rationale already correctly identifies the missing layout assertions.

### Row 2 — NIT: the asserted inequality does not reject partial PTY bytes

`tests/tui/mouse.rs:434-451` only rejects `DashboardAction::PtyBytes(Vec::new())`. `PtyBytes(vec![1])`, Detach or another wrong action satisfies the inequality. The production router separates Browse and Terminal handling at `crates/ovrcr-tui/src/dashboard/state.rs:3029-3040`; the test constructs an empty Dashboard and never explicitly installs a ready or unready Terminal-mode session. Its recommendation already states both weaknesses.

Proposed **2/1/1/2, improve**, unchanged. Remove the failure_caught claim that partial PTY bytes are rejected. State the exact empty-byte regression currently detected, and retain the request for a real unready Terminal-mode case plus exact capture/action assertions.

### Row 4 — NIT: the wrong-grapheme defect is not asserted

`crates/ovrcr-tui/src/dashboard/palette.rs:9059-9071` pastes, clicks and deletes, then checks overlay closure. The helper at `palette.rs:9004-9032` provides useful implicit render assertions: its lookup must find `a界` before the second click, and it must find Cancel. It does **not** compare the value or cursor after that click, insertion, Backspace or Delete. A wrong insertion/deletion position can therefore pass. Production mouse dispatch and Cancel processing are real (`palette.rs:3423-3478`); the neighboring narrow-cursor case explicitly compares the edited value at `palette.rs:9274-9293`.

Proposed **2/1/1/3, improve**, unchanged. Limit failure_caught to failure to render the intermediate text/control, a panic, or failure to close on Cancel. Preserve the missing post-edit value/cursor assertion as the improvement, rather than claiming that assertion already exists.

### Row 5 — BLOCKER: the survivor omits cancel-then-confirm on the same view

Candidate `tests/task_tui.rs:231-240` uses **one** fixture for `d`, Esc, `d`, `y`, and finally requires `Delete(TaskId(1))`. Named retained test `nextest|Test|ovrcr%3A%3Atask_tui|task_confirmations_honor_default_no_and_case_insensitive_answers|default` creates a **fresh** fixture inside each answer iteration (`tests/task_tui.rs:471-481`). It checks initial confirmation, each independent answer, and unchanged task count (`task_tui.rs:513-533`), but never sends a second `d` after Esc to the same view.

The retained matrix preserves the initial default-No refusal and a fresh-view positive confirmation. It does not preserve the candidate's later positive assertion after cancellation. A cancellation regression that leaves a bad selection or another input mode can keep the matrix's Esc case request-free and task count unchanged, yet prevent the candidate's later Delete. The relevant modal transition is `crates/ovrcr-tui/src/task_tui.rs:888-898`, and a new delete confirmation reads the current selection at `task_tui.rs:966-969`.

Proposed **3/2/2/3, keep**. U=0 and confirmed complete subsumption are unsupported by the named survivor. Withdraw consolidation unless an existing retained test is named that exercises this same-instance cancellation/reconfirmation sequence. This changes a proposed removal/consolidation decision; the initial high-criticality refusal remains covered, but the complete candidate is not subsumed.

### Row 9 — NIT: the compound-invalid input isolates neither admission conjunct

The sampled input contains `title="private prompt"` **and no navigation** (`native/bridge/check-guards.py:65-70`). Admission requires navigation before the allowlisted title (`native/bridge/Contract.swift:110-116`). Removing only navigation validation still rejects this payload on title; removing only title validation still rejects it on navigation. The test observes a generic failed reply, exit status, stderr and framing (`check-guards.py:38-46`), not the IPC boundary, which follows admission in `native/bridge/main.swift:43-48`.

Proposed **3/2/1/4, keep**, unchanged. The existing rationale acknowledges the masked title check, but failure_caught still overstates the missing-required-field/pre-notification protection. Describe rejection of this **compound-invalid payload** and its failed-response contract. To claim either individual guard, provide otherwise-admissible input with exactly that field invalid and observe the relevant effect boundary.

### Row 10 — NIT: positive acceptance does not establish wrong-profile rejection

The sampled invocation is only `validate(source)` at `native/bridge/check-packaging.py:44-44`. Its helper checks exit-code success against the supplied local profile (`check-packaging.py:35-41`). A validator that accepts both local and production profiles indiscriminately still passes it. The real profile checks are `native/bridge/validate-bundle.py:49-56`; separate wrong-profile inputs are actually supplied at `check-packaging.py:115-138`.

Proposed **3/2/1/4, keep**, unchanged. Narrow this row's failure_caught to rejecting the legitimately built local bundle. Attribute wrong-profile acceptance detection to the corresponding negative rows, not to this positive baseline.

### Row 16 — CONCERN: effectful-call admission safety is not constructed

The invalid cases are overlong non-JSON `x` bytes, a newline-free **initialize** frame, and malformed JSON (`src/cli/delegation_publisher.rs:906-920`). Every `serve` invocation starts in `Phase::New` (`delegation_publisher.rs:528-535`), and no invalid input is a valid effectful `tools/call` after initialization. Consequently, processing the truncated initialize frame cannot reach the publication closure even if the missing-newline guard is removed; the error-code assertion would still catch accepting initialization, but the closure panic is not independent proof of publication safety. The overlong non-JSON input likewise still fails JSON parsing without the size guard (`delegation_publisher.rs:537-555`).

Proposed **3/3/3/3, keep**. Exact line count/tool-list shape and parser refusals justify S=3. S=4 and failure_caught's publication-from-partial-tool-call claim need a fully initialized session followed by an otherwise-valid truncated/oversized effectful frame. Narrow current failure_caught to malformed/truncated initialization refusal and JSON output framing. This changes the signal score, not a pruning recommendation.

### Row 18 — CONCERN: C=3 relies on a broader artifact hazard than this mutation tests

The `display` iteration changes **only** `CFBundleDisplayName` to `unexpected` (`native/bridge/check-packaging.py:47-57`) and expects validator failure at line 99. The matching production assertion compares the display name and LSUIElement (`native/bridge/validate-bundle.py:54-57`). This does not exercise changed executable bytes, signing, installation, permissions or a wrong-target action. The CSV reason that the field is sealed by codesign does not establish a major-workflow consequence of accepting this particular display-name mutation.

Proposed **2/3/3/4, keep**. This protects bounded bundle-identity/packaging correctness with a rebuild/corrected metadata as a workaround. Retain the real-artifact fidelity and negative-case signal. Replace the broad “wrong or unreviewed artifact could be signed and installed” defect with failure to reject this incorrect display-name metadata. This changes C, not a pruning decision.

### Row 21 — CONCERN: the stated distinct retry invariant is already retained

`tests/tui/palette.rs:3120-3171` enters through palette search, submits run 1, advances the hierarchy to run 4, receives Internal, and asserts a run-4 retry with `acknowledge_stopped: false`. Retained `nextest|Test|ovrcr-tui|dashboard%3A%3Apalette%3A%3Alaunch_tests%3A%3Areopen_confirm_keeps_captured_run_when_hierarchy_advances|default` already asserts the same run-1/run-4 requests and false acknowledgement flag at `crates/ovrcr-tui/src/dashboard/palette.rs:13672-13716`. Its request helper submits Enter through production palette handling (`palette.rs:11430-11446`).

The counterpart advances the hierarchy before the first submission and receives Conflict rather than Internal; the sampled test also exercises palette discovery. Those are real distinctions, so complete subsumption/U=0 is not claimed. However, the CSV uniqueness rationale says retry rebinding is distinct from captured-run retention, while the named counterpart actually checks both. Error handling rebuilds either confirmation through the same command path (`palette.rs:6900-6916`).

Proposed **3/3/1/3, keep**. Record the small discovery/Internal-error distinction and name the retained overlapping test. U=3 overstates a distinct failure mode; the recommendation remains keep.

### Row 22 — NIT: two concrete claimed boundary inputs are absent

The rate-limit fixture supplies `retryAfterSeconds: 5`, then asserts a 60-second retry (`crates/ovrcr-runtime/src/server/quota.rs:1952-1958,2009-2013`). It does not supply zero. This strongly checks the lower clamp for five, but a zero-specific mistake can pass. The expired-deadline case constructs a **fresh empty-buffer Rpc** (`quota.rs:2015-2021`); it does not preload a matching reply before expiry. The preloaded errors belong to separate Rpc instances (`quota.rs:1996-2005`). Production classification/clamping is at `quota.rs:911-935`.

Proposed **4/4/2/3, keep**, unchanged. Exact classifications, exact redacted reasons, retry clamp, deterministic flags, timeout and missing executable justify these scores and privacy C=4. Narrow failure_caught from zero-specific spinning to the observed 5-to-60-second clamp, and from accepting an expired buffered reply to accepting the fresh request with an expired deadline.

### Row 24 — NIT: invalid JSON masks the byte bound

The positive exact choice/visibility assertions and malformed catalog cases are effective (`crates/ovrcr-runtime/src/codex_delegation.rs:1461-1470`). However, the oversized input is only spaces (`codex_delegation.rs:1472-1472`). Removing the size check at `codex_delegation.rs:130-133` still makes `serde_json::from_slice` fail at line 134, so this assertion does not distinguish enforcing the byte bound from ordinary JSON rejection.

Proposed **2/4/3/4, keep**, unchanged. The exact extraction plus malformed/hidden-choice rejection remain precise. Narrow failure_caught's size-bound claim, or use an otherwise-valid catalog padded past MAX_CATALOG to isolate it. The test currently rejects this oversized invalid input, not every otherwise-valid oversized catalog.

## Policy and totals

All four sampled improve rows satisfy `(S<=1 or F<=1) and (C>=2 or U>=2)`. No sampled keep row satisfies that gate using its existing scores. All four sampled C=4 git tests protect destructive wrong-target removal using real Git/filesystem state; the additional C=4 quota row protects private error-body/account information. None was downgraded merely for cost, fixture use outside the claimed subject, or a lone boolean assertion. The only U=0 sample was checked against its named retained test and its complete-subsumption claim is refuted above.

**13 AGREE; 11 DISAGREE: 1 BLOCKER, 3 CONCERN, 7 NIT.** Only row 5 changes a consolidation/pruning decision. Other findings leave recommendations unchanged; rows 16, 18 and 21 change scores.

The sample contains exactly **24 distinct IDs**. The programmatically computed intersection with the previous round's 24 table IDs is **empty (0)**: **none of these 24 IDs appeared in the previous round's table**.
