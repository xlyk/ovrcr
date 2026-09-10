# Task 3 review correction: private-channel setup must fail open

Review base: `32336e722f6c92843313301e91bb99ef6e6ad2b1`.

Verified the P1 source path before correction: `InvocationChannel::new()?` returned a reporting-only setup error before native spawn. The factory now uses standard `tempfile::Builder::tempdir()` so it honors the process temporary directory. This permits the deterministic fixture to point TMPDIR at an isolated regular file. No global directory permissions or test-only product configuration are changed.

RED18 retained the actual CLI failure after selecting the standard temporary directory: exit 1, no native output, and a setup error containing a private path, instead of required native output and exit 17. This is a behavioral failure, not a sandbox denial.

Correction: private-channel setup produces an optional channel. `run_native(argv, channel_ready)` invokes its one-shot setup callback before spawn. The CLI drops any allocated lease when the channel is unavailable, then emits exactly one concise reporting-unavailable diagnostic. The native command always removes both inherited private-channel variables and all three runtime reporting identity variables; it installs fresh private-channel variables only when setup succeeds. Thus setup failure cannot route native callbacks into an outer invocation. No binding or publication is added.

GREEN19: all 3 focused launcher CLI tests passed. The new test asserts actual native output, exit 17, absent inherited private/runtime reporting environment, one exact diagnostic without a private path, and no autostart.

GREEN20: all 3 launcher lifecycle tests passed (1 ignored child helper executes inside the two normal native PTYs). The new real server test asserts epoch 1 was allocated, waits for actual untracked native output, then starts another explicit invocation while that native process is still running. The second invocation reserves successfully at epoch 2, proving release occurred before fallback spawn. The original native exits 17 after real terminal input. The existing authenticated private-channel, terminal, and signal tests also pass using the normal macOS temporary directory, verifying its socket path works.

GREEN21: affected root/runtime compilation and clippy passed for all targets/all features with warnings denied. GREEN22 records formatting and diff checks. The wider suites were not repeated because this correction is limited to channel setup, environment composition, and pre-spawn reservation release.
