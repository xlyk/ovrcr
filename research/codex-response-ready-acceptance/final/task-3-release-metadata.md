# Release metadata correction

Changed only the two release-status strings in `src/cli/codex_setup.rs`: requirements now state exact Codex CLI 0.153.0 hooks-only support passed acceptance, and doctor JSON emits `accepted_exact_0.153.0_hooks_only`. Added the corresponding assertion to the existing public Codex diagnostic test in `tests/agent_setup.rs`. Existing assertions still require configuration hook trust and delivery to remain unverified; release acceptance does not certify a user's setup.

No TOML composition, hook behavior, version admission, native GUI or other production behavior changed. No staging or commits.

Verification:

- `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup --all-features codex -- --nocapture` with required process permissions and default threading: exit 0; 3 passed, 0 failed, 0 ignored, 7 filtered; 2.74s.
- Initial `rtk proxy env CARGO_INCREMENTAL=0 cargo fmt --all -- --check` exited 1, requiring multiline formatting of the added assertion. Applied `rtk proxy env CARGO_INCREMENTAL=0 cargo fmt --all`; repeated check exited 0.
- `rtk proxy git diff --check -- src/cli/codex_setup.rs tests/agent_setup.rs`: exit 0.

No full suite or native GUI repeat was run for these status strings; actual setup TOML output is unchanged from accepted native evidence.
