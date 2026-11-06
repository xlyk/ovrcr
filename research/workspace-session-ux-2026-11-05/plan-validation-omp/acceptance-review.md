Read-only review against the supplied plan, design, and observed excerpts. I did not run tests, providers, or the application; runtime behavior below is `[INFERENCE]` from source excerpts.

## 1. Blocker — Child creation has no returned workspace identity, and existing completion paths discard the dialog

**Scenario:** User opens New agent session, fills the draft, chooses Create new branch, and presses Create. The design requires the child dialog to remain visible while creation is pending, then select the “actual returned identity” and restore the parent draft. The plan says “use the returned workspace ID” and “Use actual lifecycle completion, not provisional branch text.”

**Evidence:** `CreateWorkspace` maps `Ok(())` to `LifecycleOutcome::Succeeded`; `CreateWorkspaceWithLaunch { launch: None }` maps `Ok(None)` to the same (`crates/ovrcr-runtime/src/server/lifecycle.rs:167-176`). `Succeeded` carries no project or workspace ID (`crates/ovrcr-protocol/src/wire.rs:502-507`). The Dashboard lifecycle acceptance path clears the palette (`crates/ovrcr-tui/src/dashboard/lifecycle_ui.rs:256-260`), and the standalone workspace attach path also clears the palette and selects the workspace container when it has no sessions (`crates/ovrcr-tui/src/dashboard/palette.rs:3336-3375`). Neither preserves a parent session draft.

**Why the plan misses it:** It assumes a server-returned workspace identity and a pending child dialog that the current lifecycle protocol and Dashboard state transitions do not provide. The client-generated `workspace_id` is sent to the server, but success is not acknowledged with that identity.

**Smallest correction:** Add a typed created-workspace outcome (for example `CreatedWorkspace { project, id }`) or explicitly make the accepted client-generated ID the returned identity with a registration receipt. Define a child-draft/pending owner that survives lifecycle acceptance instead of `palette = None`, then assert the selected workspace equals that exact ID and the parent draft/focus is restored. Update the wire snapshot and completion path in the same change.

## 2. Blocker — Prompt retention across reconnect/restart and prompt-content absence have no implementation mechanism or mapped test

**Scenario:** An explicit multiline prompt is accepted; the Dashboard reconnects; the prompt must remain pending with server-owned target/run identity. A server restart must not replay it. The prompt must not appear in settings, retained metadata, Events, or logs.

**Evidence:** Current wire requests carry no prompt/model or pending-prompt readback (`crates/ovrcr-protocol/src/wire.rs:167-183`), and `LifecycleOutcome` has no prompt-delivery state (`wire.rs:502-507`). Plan §2 asserts reconnect/restart semantics, but §3’s exit list, §4’s CUA bullets, and §5’s check list name no reconnect/restart fixture and no scan for prompt bytes. The repository guide requires mapping each acceptance requirement to an actual assertion and separating live fixture evidence (`docs/development/testing.md`), and Events are persisted to `events.jsonl` (`docs/development/runtime.md`). CUA “pending/error and post-create restoration” does not cover a new Dashboard connection or server restart.

**Why the plan misses it:** “Preserve pending text across Dashboard reconnects” has no defined request/response or in-memory lookup by which a reconnected Dashboard learns the pending prompt. “Never serialize … into Events or logs” has no named negative assertion.

**Smallest correction:** Specify the exact server-held pending-prompt lookup/status for a session/run, and add a live-server test using a unique prompt marker: reconnect retains it; restart does not replay; settings file, retained store/metadata, and `events.jsonl` do not contain the marker. Put those assertions in §3 exit, §4, and §5.

## 3. Blocker — The multiline submit chord and its encoding have no implementation or acceptance step

**Scenario:** Prompt focused; Enter must insert a newline; Alt+Enter must submit; picker Enter must only accept the picker choice. The design requires the portable chord’s actual terminal encoding to be verified.

**Evidence:** The plan’s key work is limited to changing `n` and palette/menu labels and preserving input priority. The current key table binds `n` to Create terminal and `w` to Create workspace (`crates/ovrcr-tui/src/dashboard/keymap.rs:1004-1005`); no prompt-submit action is proposed. §4 CUA mentions “picker Enter versus form submission, no input leakage” but not Enter-as-newline or Alt+Enter submission. Existing modified-key encoding coverage is named `control_punctuation_and_modified_keys_encode` in `tests/tui/main.rs`; the plan does not add a corresponding prompt-editor case. `docs/testing-computer-use.md` separately warns that modified-key encodings need native checks, not just unit tests.

**Why the plan misses it:** The design names a visible submit chord as required behavior, but the plan has no plan step for binding, encoding, dispatch, or native proof.

**Smallest correction:** Add an explicit keymap action and test through the production Dashboard key path: Enter inserts newline in Prompt, Alt+Enter submits exactly once, picker Enter accepts without form submission, paste remains multiline, and no input reaches the underlying terminal. Verify the chord natively in CUA and record the terminal/encoding used.

## 4. Clarification — Wire/PROTOCOL_VERSION and settings-document compatibility are not named

**Scenario:** Adding model/prompt fields to `CreateSessionRequest`/`SessionLaunch`, or a created-workspace identity to `LifecycleOutcome`, changes bincode. The wire snapshot test compares exact bytes and explicitly requires bumping `PROTOCOL_VERSION` in `codec.rs` and replacing the expected block (`crates/ovrcr-protocol/src/wire.rs:3980-3993`). The plan says “wire compatibility requirement” without naming that gate or the same-version/old-client policy.

Separately, `launch_choices` is a `{kind, preset}` document with `deny_unknown_fields` (`crates/ovrcr-runtime/src/settings/mod.rs:270-274,726-739`), catalog `Shape::Launches` documents only those fields (`crates/ovrcr-protocol/src/catalog.rs:486-490`), and the settings editor enumerates only Terminal plus agent presets (`crates/ovrcr-tui/src/dashboard/settings_editor.rs:332-338`). Extending remembered launch state with a model policy therefore also needs the document shape, catalog, editor, write fixtures, and settings matrix updated.

**Smallest correction:** Add the `PROTOCOL_VERSION`/snapshot update and client-version decision to §2, and name the settings document/catalog/editor/matrix files alongside “typed settings catalog/loader.” Otherwise the model policy can compile in the form path but be rejected or silently dropped by settings.

## 5. Clarification — “Actual installed-provider proof” lacks a per-harness evidence definition

**Scenario:** A CUA session launches Claude Code, Codex, Pi, or omp and shows a printed line. That does not prove explicit-model application, Harness-default semantics, or that the prompt reached the provider rather than a launcher banner or ignored input.

**Evidence:** Plan §4 requires actual installed-provider proof, but defines the evidence only as “a separately printed output line” and “existing provider fixtures as appropriate.” The named `tests/agent_setup.rs` contains setup/doctor fixtures (`setup_*`, `doctor_*`), and the existing Hermes session fixture is a stand-in (`tests/cli.rs:2615-2619`). The computer-use guide itself says a disposable configured preset “proves the launch UI only, not provider reporting” (`docs/testing-computer-use.md`).

**Smallest correction:** Define, per harness, the evidence tuple: installed version; exact argv including model or explicit Harness default; provider-visible resolved model or explicit refusal; prompt containing a unique marker with that marker observed in executed output; trust/approval interruption. Mark stand-in fixtures and UI-only preset launches as non-evidence for this gate.

## Proof gaps

I did not inspect current line numbers for every plan/design statement and did not run the four-provider or restart scenarios. Findings above are based only on the supplied excerpts. The most material unverified area is whether a reconnected Dashboard can retrieve pending prompt state without adding a wire surface; the plan currently does not establish that.

**Verdict: NEEDS REVISION** — plan quality. The lifecycle-identity/pending-dialog assumption and the missing prompt/input acceptance paths are substantive, not cosmetic.
