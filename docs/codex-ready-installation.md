# How to install Codex readiness reporting

Use the existing hooks-only adapter from PR #56 with stable Codex CLI **>=0.153.0, <0.154.0**. This procedure installs the [last observed root turn contract](codex-reporting-setup.md#what-the-indicator-means). It requires no additional reporter, transcript collector, metrics or notification configuration.

The retained acceptance at `56b84f9` used a disposable macOS fixture. A built artifact, printed configuration and passing doctor are preparation evidence. Mark a host installation ready only after its installed binary, server, effective configuration and native hook delivery pass the check below. Keep preparation, installation and native acceptance results separate.

## Prepare the exact build and paths

Record the full reviewed OVRCR commit, clean tracked worktree state, build command, artifact path and SHA-256. PR #56 merged as `cadb7a3174701e1666fa0562ebb714593f9d84f3`; use the actual reviewed commit for the artifact being installed and record any source-tree equivalence separately. Package version alone cannot identify the revision.

Set these task-specific variables to inspected absolute paths. `OVRCR_PREP` must be a private task-owned directory. `OVRCR_INSTALL` is the proposed permanent executable, for example `$HOME/.local/bin/ovrcr` when that is the user's chosen installation. Inspect an existing file or symlink before choosing how to replace it. `CODEX_BIN` must have basename `codex` and print a stable `codex-cli 0.153.x` version; select the same executable for doctor and the managed launch. Later patches are compatible by policy; native acceptance evidence remains specific to tested versions.

```sh
OVRCR_CHECKOUT='/absolute/path/to/reviewed/ovrcr'
OVRCR_PREP='/absolute/path/to/private/preparation'
OVRCR_INSTALL='/absolute/path/to/installed/ovrcr'
CODEX_BIN='/absolute/path/to/codex'
CODEX_SETTINGS='/absolute/path/to/intended/codex/config.toml'

git -C "$OVRCR_CHECKOUT" rev-parse HEAD
git -C "$OVRCR_CHECKOUT" status --short
"$CODEX_BIN" --version
cargo build --manifest-path "$OVRCR_CHECKOUT/Cargo.toml" \
  --locked -p ovrcr --release --target-dir "$OVRCR_PREP/target"
OVRCR_CANDIDATE="$OVRCR_PREP/target/release/ovrcr"
"$OVRCR_CANDIDATE" --version
shasum -a 256 "$OVRCR_CANDIDATE"
```

Use `sha256sum` on a Linux host without `shasum`. The PR #56 implementation uses wire protocol **6**. Record the artifact's actual `--version` output; the CLI prints package and protocol versions, not a Git commit. Run the repository's required verification for the reviewed revision and retain failures as well as passes.

Inspect the intended Codex home, config file, `hooks.json` and any profile or project configuration that the launch will actually load. Codex discovers `hooks.json` from each configuration layer's hook directory; `hooks.state` stores per-handler trust and enablement rather than file-inclusion directives. Keep private backups of these existing hook surfaces and preserve their handlers and declaration order. A `--settings` argument tells OVRCR which TOML file to inspect or compose; it does not select Codex's effective configuration or inspect `hooks.json`. Preserve authentication, permission policy and existing native trust state. Do not print the full live configuration or copy credentials into evidence.

Inventory any existing OVRCR client, server executable, socket, registry, service definition and live sessions before proposing a change. Inspect the actual selected `OVRCR_CONFIG`, `OVRCR_SOCKET` and any `OVRCR_SERVER_EXECUTABLE` override; a PATH lookup alone does not establish which server owns the sessions. Record absent installations as absent rather than assuming a default path. Use the existing compatible client for read-only session inventory. Do not treat a new client's protocol mismatch as an empty server.

## Prepare and review the hook composition

For an existing settings file, keep a private backup with its original permissions, then compose to a different file. If replacing old reporter paths, remove only those exact handlers from a private working copy first and set `CODEX_COMPOSE_INPUT` to that copy:

```sh
cp -p "$CODEX_SETTINGS" "$OVRCR_PREP/config.before.toml"
CODEX_COMPOSE_INPUT="$CODEX_SETTINGS"
"$OVRCR_CANDIDATE" agent setup codex --print \
  --settings "$CODEX_COMPOSE_INPUT" > "$OVRCR_PREP/config.candidate.toml"
"$OVRCR_CANDIDATE" agent doctor codex --json \
  --executable "$CODEX_BIN" --settings "$OVRCR_PREP/config.candidate.toml"
```

If there is no settings file, record its absence and omit `--settings` from setup. Keep the draft in the private preparation directory. Setup never writes the intended settings file or native trust. Never redirect onto its input file.

Review the five synchronous `SessionStart`, `UserPromptSubmit`, `Stop`, `Interrupt` and `SessionEnd` reporters. They must directly `exec` the reviewed OVRCR executable with `report codex --stdin`. Preserve unrelated hooks and their relative declaration order, including approval hooks, and preserve all existing configuration values. Setup appends missing reporters and retains an exact existing reporter; serialization changes comments and formatting. For a path change, remove only the exact old OVRCR reporter handlers from a private working copy before composing the replacement. Do not delete an entire event group that contains other handlers.

For exact Codex 0.153.0, keep existing `hooks.json` bytes unchanged and add only the generated reporters to TOML. The [pinned source](../research/issue-58-ready-install/completion/native-hook-source.md) loads configuration layers from low to high and appends JSON then TOML declarations within each layer. Both sources load even though Codex warns that a single representation is preferred. Their handlers run concurrently when synchronous; results are collected back into configured order. Preserve declarations and native concurrency, without inserting wrappers or requiring one handler to finish before another.

The candidate composition embeds `OVRCR_CANDIDATE`, because setup uses its own absolute executable path. It is a preparation draft. For the proposed permanent configuration, change only those exact newly generated reporter commands to the reviewed `OVRCR_INSTALL` path, using the shell quoting shown by setup. Retain both drafts and review the full semantic change privately. After installation, regenerate from the permanent binary and confirm its reporters match the proposed change before writing live configuration. Never leave a temporary build path in a permanent hook.

Doctor's `probe_status: supported` means the selected executable passed the exact version probe. `supplied_file_supported` means the supplied TOML contains the expected synchronous reporter commands for the OVRCR binary running doctor. Effective configuration, trust and delivery remain `unverified`. Doctor does not inspect the effective hook list or establish that any handler executed. Verify native source paths, declaration order and trust status separately from dispatch. Doctor neither contacts the server nor tests a native prompt, and its accepted `release_status` is an implementation claim. A path-adjusted draft checked by the candidate binary will correctly fail its exact-path configuration check; verify it with the permanent binary after installation.

At this point the binary and configuration change are reviewable. Writing live configuration, replacing an installed binary, changing native trust or transitioning an existing server requires the operator's installation authorization. Preparation alone does not grant it.

## Install the reviewed binary and configuration

After authorization, retain the prior executable or symlink and its target for rollback. If the selected path is new, record that it was absent. Install the reviewed candidate at the proposed permanent path using the host's chosen installation method, then compare its SHA-256 to the candidate and run its `--version`. The [standard local install](../README.md#install) copies the release binary with executable permissions. Keep a known compatible old client until the server transition and rollback checks are complete.

Regenerate the composition with the installed binary into another private file. If replacing old reporter paths, supply the reviewed private working copy from which only those old reporters were removed:

```sh
"$OVRCR_INSTALL" agent setup codex --print \
  --settings "$CODEX_COMPOSE_INPUT" > "$OVRCR_PREP/config.installed-path.toml"
"$OVRCR_INSTALL" agent doctor codex --json \
  --executable "$CODEX_BIN" --settings "$OVRCR_PREP/config.installed-path.toml"
```

Omit `--settings` if the file was absent. Compare this output with the approved draft and verify that the source configuration has not changed since preparation. Merge the reviewed change into the intended file, retaining its permissions and any intervening unrelated edits. Do not blindly replace a newer live config with an old composed file. Run doctor again against the resulting live file from the installed binary; retain a redacted result.

Installation does not upgrade a running server. Perform the following native check with a fresh isolated server before scheduling any normal-server transition. Existing user sessions can continue on their original server and protocol throughout this check.

## Verify the installed path natively

Use a dedicated native terminal window for the fixture so these exports do not replace the normal server selection in other shells. This example creates a temporary repository and workspace using the existing CLI. Set both the isolated registry and socket before any OVRCR command; also select the installed server executable explicitly:

```sh
OVRCR_NATIVE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/ovrcr-codex-ready.XXXXXX")
export OVRCR_CONFIG="$OVRCR_NATIVE_ROOT/config.toml"
export OVRCR_SOCKET="$OVRCR_NATIVE_ROOT/server.sock"
export OVRCR_DASHBOARD_CONFIG="$OVRCR_NATIVE_ROOT/dashboard.toml"
export OVRCR_SERVER_EXECUTABLE="$OVRCR_INSTALL"

mkdir "$OVRCR_NATIVE_ROOT/repo"
git -C "$OVRCR_NATIVE_ROOT/repo" init -b main
git -C "$OVRCR_NATIVE_ROOT/repo" -c user.name=OVRCR \
  -c user.email=ovrcr@example.invalid commit --allow-empty -m 'Native readiness fixture'
"$OVRCR_INSTALL" project add ready-check "$OVRCR_NATIVE_ROOT/repo" \
  --workspace-root "$OVRCR_NATIVE_ROOT/workspaces"
"$OVRCR_INSTALL" workspace create --project ready-check \
  --new-branch codex/ready-install-check --base main
OVRCR_NATIVE_SESSION=$("$OVRCR_INSTALL" new --project ready-check \
  --workspace codex/ready-install-check --name codex-ready -- sh)
"$OVRCR_INSTALL"
```

Confirm the fixture server runs the installed executable and uses only the recorded temporary config, socket and workspace. Select `ready-check / codex/ready-install-check / codex-ready` in the dashboard and press Enter. In that OVRCR terminal, verify its inherited `OVRCR_CONFIG` and `OVRCR_SOCKET` match the fixture, then run the installed wrapper and exact native executable without an initial prompt:

```sh
"$OVRCR_INSTALL" agent run codex -- "$CODEX_BIN"
```

Use literal absolute paths if those shell variables are not set in the new terminal. Keep Codex on the intended authorized home and configuration, including its existing `hooks.json`; the OVRCR fixture isolates sessions, not provider settings. If an alternate `CODEX_HOME` is authorized for a separate probe, record it explicitly and label that probe as alternate-home evidence. It cannot establish delivery from the intended normal Codex configuration. A raw `codex` command never creates managed reporting. A wrapper outside an OVRCR PTY, unsupported version or unsupported launch form runs native Codex with reporting unavailable. See the [accepted launch grammar](codex-reporting-setup.md).

Use native `/hooks` to review the effective list, including existing JSON and plugin hooks. Within each layer, verify JSON declarations appear before TOML declarations for the same event. Preserve original handler keys, contents and relative declaration order, and record each handler's trust and enablement. The list's display order and the collected-result order do not establish execution or completion order; synchronous hooks can run concurrently.

Individually trust only the five reviewed OVRCR reporters through the native UI, and trust the temporary workspace as authorized. Preserve the existing approval policy and unrelated hook state. Existing JSON or plugin hooks may still need review; leave those decisions unchanged and report their status rather than claiming they dispatched. Do not use a blanket trust action or copy a trust hash from an example. Submit the first tracked prompt only after review of the OVRCR reporters finishes; an initial prompt submitted during review can complete untracked.

Use a bounded, non-sensitive prompt with a distinct response marker. Verify the marker is actual assistant output, then retain screenshots and relevant accessibility text showing these outcomes:

1. The root prompt produces Busy / Observed, and its matching Stop produces Ready / Observed while the native process is still running. Ready describes that observed response; it does not certify task success.
2. A second prompt clears Ready to Busy and produces a new Ready observation. Inspect `"$OVRCR_INSTALL" session usage "$OVRCR_NATIVE_SESSION"` from a shell with the same fixture config and socket for its binding, activity and reporting health. Codex metrics remain null.
3. Detach and reattach the one dashboard while preserving the server and provider PID. The last Ready observation remains visible with its reporting health.
4. During a bounded active native turn, press Ctrl-C and verify Idle / Observed without Ready for that interrupted turn. Submit another marker prompt and verify a fresh Busy then Ready before testing exit.
5. On normal native exit, reporting becomes Unavailable. Any retained Ready is historical; it does not establish current readiness.

Record the installed executable hash, source revision, exact Codex version, server identity, session ID, observed transitions and any failed attempts. Keep OVRCR reporter trust/delivery separate from preservation and dispatch of unrelated hooks. Native `/hooks` proves the listed configuration and review status; claim dispatch only with observed hook run/completion evidence. Preserve any unmet review or dispatch gate explicitly. Never record hook tokens, credentials, full configuration or sensitive conversation content. Keep an unavailable or unrun native check explicit; passing doctor does not discharge it. Remove only disposable resources owned by the check, preserving the user's sessions and installation.

After native exit and evidence capture, exit the fixture's shell and detach its dashboard. From the dedicated fixture shell, inspect `terminal list`, close only any remaining fixture terminals with `terminal close ID`, then run `"$OVRCR_INSTALL" shutdown` without `--kill`. Verify the recorded fixture processes and socket are absent before removing its temporary root. Preserve evidence outside that root first. If ownership or cleanup is uncertain, retain the directory and report the gap. This proves the installed binary against an isolated OVRCR server; it does not claim that the normal server has been upgraded.

## Transition the normal server after native acceptance

This is a separate, later operation requiring authorization for the actual normal-server transition. Return to a shell with the recorded normal registry and socket; never reuse the fixture exports for this step. A server keeps its original executable and protocol after a build or install. Live PTYs, screens and readiness state belong to that process. They cannot be transferred to a replacement server. Detaching the dashboard preserves them; stopping the server loses them.

If live sessions remain, keep the current server and compatible client available. Finish and close sessions only with their owners' authorization, or postpone the live transition. Native acceptance in the isolated fixture does not require stopping these sessions. Do not connect a protocol 6 candidate to a protocol 4 server as an installation test, remove its live socket, signal a discovered PID, or follow the protocol-mismatch message's `shutdown --kill` suggestion as an upgrade shortcut.

Once all live sessions have been deliberately closed, use the matching old client and the discovered registry/socket to request `shutdown` **without** `--kill`. This request refuses while sessions remain. If it refuses, preserve the server and resolve the remaining sessions with their owners. Confirm the identified old server has exited and its socket is gone before starting its replacement. A mismatched new client cannot safely issue this shutdown to the old protocol.

If an OS service owns the server, retain its definition, executable target and environment-file path in the rollback record. After the empty server has stopped, update that existing service through its documented installation flow, preserving its settings. Replacing a PATH binary does not update a service pointing elsewhere. `service stop` and `service uninstall` can terminate managed sessions; they are not substitutes for the empty-server check. Service reinstallation and any restart are part of the authorized transition. See [service installation](scheduled-tasks.md#run-the-scheduler-at-login).

Start the replacement through the selected normal launch or service path. Confirm its actual installed executable, selected config/socket and successful protocol handshake. Record this normal-server transition separately from the installed-native fixture result. An old server with the same package version is not evidence that the reviewed code is running.

## Roll back the installation

Remove only the exact reporter handlers added by this installation, preserving unrelated handlers and order. Restore the entire private settings backup only if the live file has no later changes; otherwise reverse the reviewed changes manually. If the file was originally absent, remove it only when it contains solely this installation's additions. Handle any related hook trust change through native Codex and preserve unrelated trust decisions. Existing managed invocations may become unavailable; close them deliberately and use a fresh launch after correcting configuration.

Restore the saved executable or symlink only after deciding how to handle any sessions on the replacement server. Reverting the client alone cannot downgrade a running server. Apply the same owner-approved drain and matching-client `shutdown` procedure before restoring the prior server and service definition. Retain the registry, workspaces, task data, Codex authentication and the rollback record. No rollback recovers PTYs or readiness state already lost by stopping a server.
