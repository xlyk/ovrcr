# OVRCR #470 coordinator evidence

## Revision and scope

- Branch: `codex/470-coordinator`.
- Base: `1405e5b245bab08b2bfad797ae88de9958cb1bc1` (#469 integration tip).
- Commit: `a6b508b4` (`feat(operator): add coordinator skill`).
- Working tree is clean after the commit.
- Protocol remains 85; SQLite schema remains 33.
- Changes: private coordinator skill materialization and explicit `--skill`, policy and Operator docs, focused launch/isolation assertions, and the opt-in local-provider native scenario.
- The parent agent owns native CUA, final two-axis review, final-head CI and PR delivery. This child did not run the GUI walkthrough.

## Focused checks

| Check | Result |
|---|---|
| TDD argv check before implementation | Red: expected argv lacked the required explicit `--skill` path. |
| `cargo test -p ovrcr --lib rpc_argv_is_exact_and_sanitized -- --nocapture` | 1 passed; asserts exact argv, preserved discovery-off flags, coordinator file content, directory mode 0700, file mode 0600, and CLI-guide-only system prompt. |
| `cargo test -p ovrcr --features acceptance-diagnostics --test operator_e2e operator_gui_fixture_requires_a_launcher_ownership_marker -- --nocapture` | 1 passed; refuses an unmarked HOME and symlinked profile target, accepts only a private marker bound to the HOME and fixture token. |
| `cargo test -p ovrcr --features acceptance-diagnostics --test operator_e2e compaction_detection_distinguishes_coordinator_summary_guidance -- --nocapture` | Red then green; 1 passed. The fixture now recognizes Pi's actual compaction prompt and does not mistake the required `summary` trailer wording for compaction. |
| Pi 0.87.1 combined-option `--help` invocation | Exit 0 with `--no-skills --skill <file> --no-extensions --extension <file> --no-context-files --offline`; confirms the launch arguments parse. Installed loader source and the provider test confirm explicit skills remain advertised with ambient discovery disabled. |
| `installed_pi_operator_bundled_skill_system_only` with local loopback Provider | 1 passed, 32 filtered, 12.81s. Full chat, Quick message and resume advertise coordinator name/description and exact private profile path; personal skill metadata stays absent; CLI guide remains in the system prompt. |
| `cargo fmt --all -- --check`; `git diff --check` | Passed. |
| Ignored native helper `--no-run --message-format=json` | Exit 0. Executable: `/private/tmp/ovrcr-465-integration/target/debug/deps/operator_e2e-5232dd669418270c`. Direct `--list` confirms filter `installed_pi::operator_pi_gui_provider`. |

The first installed-Pi attempt was blocked before test setup because the sandbox denied `TcpListener::bind(127.0.0.1:0)`. The same focused offline check passed after approved loopback access. An early test-only assertion incorrectly expected the full skill body in the initial prompt; Pi intentionally advertises name, description and location, then reads the body on demand. The final assertion follows that contract, and the GUI helper makes an actual Pi `read` call on the private file before dispatch.

The integration branch tip and this branch's base both remained `1405e5b245bab08b2bfad797ae88de9958cb1bc1`; there were no newer integration commits. A no-op merge attempt could not update `ORIG_HEAD` because the sandbox denied writes to the common worktree metadata, and changed nothing. The parent confirmed the exact tip equality satisfies the up-to-date requirement. The staged review covered only the seven intended ticket files; `git diff --cached --check` passed.

## Native helper instructions

Compile the ignored helper before launching the GUI. The retained artifact above was built with the current source and shared integration target; keep `CARGO_TARGET_DIR=/private/tmp/ovrcr-465-integration/target` for `just gui` so its CLI image matches the helper's compile-time `CARGO_BIN_EXE_ovrcr`.

Use the prepared task-private HOME `/private/tmp/ovrcr-465-native-home`. Before `just gui`, make it private and write its owner marker with the same token that will bind the GUI fixture root. The marker format is exact:

```text
ovrcr-operator-gui-fixture v1
root=/private/tmp/ovrcr-465-native-home
token=operator-gui-owned-fixture
```

Prepare the HOME marker with:

```sh
rtk proxy sh -c '
home=/private/tmp/ovrcr-465-native-home
token=operator-gui-owned-fixture
umask 077
mkdir -p "$home"
chmod 700 "$home"
printf "ovrcr-operator-gui-fixture v1\\nroot=%s\\ntoken=%s\\n" "$home" "$token" > "$home/operator-pi-fixture-owner"
chmod 600 "$home/operator-pi-fixture-owner"
'
```

Keep the HOME and marker modes at 0700 and 0600. Launch the GUI process with that HOME, `PI_TELEMETRY=0`, and `PI_CODING_AGENT_DIR` unset. Preserve the native plan's `CARGO_HOME`, `RUSTUP_HOME`, quota setting and minimal environment. When the launcher reports its actual canonical `ovrcr-gui-*` root, write the matching `operator-pi-fixture-owner` marker there using that exact root and the same token.

```sh
rtk proxy sh -c '
root="$1"
token="$2"
root="$(cd "$root" && pwd -P)"
umask 077
printf "ovrcr-operator-gui-fixture v1\\nroot=%s\\ntoken=%s\\n" "$root" "$token" > "$root/operator-pi-fixture-owner"
chmod 600 "$root/operator-pi-fixture-owner"
' sh "$OVRCR_OPERATOR_GUI_HOME" operator-gui-owned-fixture
```

After the root marker is in place, run the retained helper binary directly (no Cargo rebuild while the GUI or Server is live):

```sh
rtk proxy env -i \
  PATH=/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin \
  HOME=/private/tmp/ovrcr-465-native-home \
  TMPDIR=/private/tmp \
  PI_TELEMETRY=0 \
  OVRCR_TEST_PI_EXECUTABLE=/Users/xlyk/.local/bin/pi \
  OVRCR_OPERATOR_GUI_HOME="$OVRCR_OPERATOR_GUI_HOME" \
  OVRCR_OPERATOR_GUI_TOKEN=operator-gui-owned-fixture \
  OVRCR_OPERATOR_GUI_COORDINATOR=1 \
  /private/tmp/ovrcr-465-integration/target/debug/deps/operator_e2e-5232dd669418270c \
  installed_pi::operator_pi_gui_provider --ignored --nocapture
```

The helper seeds both `operator-pi` and `$HOME/.pi/agent` to its loopback Provider, verifies the HOME marker/profile target, reads the coordinator file with Pi's `read` tool, then creates two managed Pi sessions using the matching CLI image and `/Users/xlyk/.local/bin/pi`. It prints their identities before `OPERATOR_PI_GUI_READY`. The existing `agent-bin/pi` shell fixture is not used.

Walkthrough sequence:

1. In Operator, send `REQUEST_COORDINATOR_DISPATCH`. The helper checks the skill read result, captures both sessions' `run`, numeric Reporter revision and opaque `unread.turn`, Pastes both tasks with completion trailers, registers both watches and returns the Task / Workspace / Session table. Both Workspace replies remain held.
2. Send `REQUEST_THREE_HELD`; wait for `HELD_BEFORE_RELEASE`, then press Ctrl-x. Confirm Stop rolls back while the same Operator worker run and both watches remain live.
3. Send `REQUEST_COORDINATOR_WARM_CHAT`. Confirm it completes while both Workspace replies remain held and the stopped prompt/partial text is absent from the resumed context.
4. Enter a unique unsent draft, hide Operator, then create `$OVRCR_OPERATOR_GUI_HOME/operator-pi/fixture-release`. Keep the provider helper running. Wait for both Reporter Ready observations and one batched `AsyncWakeup`; the hidden dock should show 1 unread.
5. Reopen Operator. Confirm the wakeup uses the clean Operator heading, the draft is preserved, both Pi replies were read, `git status`, `git diff --stat`, and both `git diff --check` exit codes were reported, and both Retained sessions remain open.
6. Save screenshots and matching accessibility evidence for dispatch/table, Stop and warm chat, hidden unread/draft, and reopened wakeup. Only after the last scenario, create `$OVRCR_OPERATOR_GUI_HOME/operator-pi/fixture-stop`, preserve the helper's redacted `fixture-requests.jsonl`, close the GUI through CUA, and record launcher cleanup.
