# How to install Codex readiness reporting

Use the existing hooks-only adapter from PR #56 with exact Codex CLI **0.153.0**. This procedure installs the [last observed root turn contract](codex-reporting-setup.md#what-the-indicator-means). It requires no additional reporter, transcript collector, metrics or notification configuration.

The retained acceptance at `56b84f9` used a disposable macOS fixture. A built artifact, printed configuration and passing doctor are preparation evidence. Mark a host installation ready only after its installed binary, server, effective configuration and native hook delivery pass the check below. Keep preparation, installation and native acceptance results separate.

## Prepare the exact build and paths

Record the full reviewed OVRCR commit, clean tracked worktree state, build command, artifact path and SHA-256. PR #56's preparation base for this follow-up is `86ef46b8eff3128566f76022c2cfef49714031ac`; use the actual reviewed commit for the artifact being installed. Package version alone cannot identify the revision.

Set these task-specific variables to inspected absolute paths. `OVRCR_PREP` must be a private task-owned directory. `OVRCR_INSTALL` is the proposed permanent executable, for example `$HOME/.local/bin/ovrcr` when that is the user's chosen installation. Inspect an existing file or symlink before choosing how to replace it. `CODEX_BIN` must have basename `codex` and print exactly `codex-cli 0.153.0`; select the same executable for doctor and the managed launch. A newer version is outside the accepted contract.

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

Inspect the intended Codex home, config file and any profile or project configuration that the launch will actually load. A `--settings` argument tells OVRCR which file to inspect or compose; it does not select Codex's effective configuration. Preserve authentication, permission policy and existing native trust state. Do not print the full live configuration or copy credentials into evidence.

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

Review the five synchronous `SessionStart`, `UserPromptSubmit`, `Stop`, `Interrupt` and `SessionEnd` reporters. They must directly `exec` the reviewed OVRCR executable with `report codex --stdin`. Preserve unrelated hooks and their relative order, including approval hooks, and preserve all existing configuration values. Setup appends missing reporters and retains an exact existing reporter; serialization changes comments and formatting. For a path change, remove only the exact old OVRCR reporter handlers from a private working copy before composing the replacement. Do not delete an entire event group that contains other handlers.

The candidate composition embeds `OVRCR_CANDIDATE`, because setup uses its own absolute executable path. It is a preparation draft. For the proposed permanent configuration, change only those exact newly generated reporter commands to the reviewed `OVRCR_INSTALL` path, using the shell quoting shown by setup. Retain both drafts and review the full semantic change privately. After installation, regenerate from the permanent binary and confirm its reporters match the proposed change before writing live configuration. Never leave a temporary build path in a permanent hook.

Doctor's `probe_status: supported` means the selected executable passed the exact version probe. `supplied_file_supported` means the supplied TOML contains the expected synchronous reporter commands for the OVRCR binary running doctor. Effective configuration, trust and delivery remain `unverified`. Doctor neither contacts the server nor tests a native prompt, and its accepted `release_status` is an implementation claim. A path-adjusted draft checked by the candidate binary will correctly fail its exact-path configuration check; verify it with the permanent binary after installation.

At this point the binary and configuration change are reviewable. Writing live configuration, replacing an installed binary, changing native trust or transitioning an existing server requires the operator's installation authorization. Preparation alone does not grant it.

## Transition an existing server without losing sessions

A server keeps its original executable and protocol after a build or install. Live PTYs, screens and readiness state belong to that process. They cannot be transferred to a replacement server. Detaching the dashboard preserves them; stopping the server loses them.

If live sessions remain, keep the current server and compatible client available. Finish and close sessions only with their owners' authorization, or postpone the live transition. Test the candidate with both a disposable `OVRCR_CONFIG` and `OVRCR_SOCKET`; a config override by itself is insufficient isolation. Do not remove a live socket, signal a discovered PID, or follow the protocol-mismatch message's `shutdown --kill` suggestion as an upgrade shortcut.

Once all live sessions have been deliberately closed, use the matching old client and the discovered registry/socket to request `shutdown` **without** `--kill`. This request refuses while sessions remain. If it refuses, preserve the server and resolve the remaining sessions with their owners. Confirm the identified old server has exited and its socket is gone before starting its replacement. A mismatched new client cannot safely issue this shutdown to the old protocol.

If an OS service owns the server, retain its definition, executable target and environment-file path in the rollback record. After the empty server has stopped, update that existing service through its documented installation flow, preserving its settings. Replacing a PATH binary does not update a service pointing elsewhere. `service stop` and `service uninstall` can terminate managed sessions; they are not substitutes for the empty-server check. Service reinstallation and any restart are part of the authorized transition. See [service installation](scheduled-tasks.md#run-the-scheduler-at-login).

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

Start the replacement server from the installed executable through the selected normal launch or service path. Confirm its actual executable and successful protocol handshake, then create a fresh OVRCR terminal for the native check. An old server with the same package version is not evidence that the reviewed code is running.

## Verify the installed path natively

In that OVRCR terminal, run the exact installed wrapper and native executable without an initial prompt:

```sh
"$OVRCR_INSTALL" agent run codex -- "$CODEX_BIN"
```

Use literal absolute paths if those shell variables are not set in the new terminal. A raw `codex` command never creates managed reporting. A wrapper outside an OVRCR PTY, unsupported version or unsupported launch form runs native Codex with reporting unavailable. See the [accepted launch grammar](codex-reporting-setup.md).

Review the five command hooks through Codex's native UI and trust only the reviewed commands and intended workspace. Keep the existing approval policy and other hooks. Do not copy a trust hash from an example or edit trust state to bypass the native prompt. Submit the first tracked prompt only after native hook review finishes; an initial prompt submitted during review can complete untracked.

Use a bounded, non-sensitive prompt with a distinct response marker. Verify the marker is actual assistant output, then retain screenshots and relevant accessibility text showing these outcomes:

1. The root prompt produces Busy / Observed, and its matching Stop produces Ready / Observed while the native process is still running. Ready describes that observed response; it does not certify task success.
2. A second prompt clears Ready to Busy and produces a new Ready observation. Inspect `"$OVRCR_INSTALL" session usage ID` for the same session's binding, activity and reporting health. Codex metrics remain null.
3. Detach and reattach the one dashboard while preserving the server and provider PID. The last Ready observation remains visible with its reporting health.
4. During a bounded active native turn, press Ctrl-C and verify Idle / Observed without Ready for that interrupted turn. Submit another marker prompt and verify a fresh Busy then Ready before testing exit.
5. On normal native exit, reporting becomes Unavailable. Any retained Ready is historical; it does not establish current readiness.

Record the installed executable hash, source revision, exact Codex version, server identity, session ID, observed transitions and any failed attempts. Never record hook tokens, credentials, full configuration or sensitive conversation content. Keep an unavailable or unrun native check explicit; passing doctor does not discharge it. Remove only disposable resources owned by the check, preserving the user's sessions and installation.

## Roll back the installation

Remove only the exact reporter handlers added by this installation, preserving unrelated handlers and order. Restore the entire private settings backup only if the live file has no later changes; otherwise reverse the reviewed changes manually. If the file was originally absent, remove it only when it contains solely this installation's additions. Handle any related hook trust change through native Codex and preserve unrelated trust decisions. Existing managed invocations may become unavailable; close them deliberately and use a fresh launch after correcting configuration.

Restore the saved executable or symlink only after deciding how to handle any sessions on the replacement server. Reverting the client alone cannot downgrade a running server. Apply the same owner-approved drain and matching-client `shutdown` procedure before restoring the prior server and service definition. Retain the registry, workspaces, task data, Codex authentication and the rollback record. No rollback recovers PTYs or readiness state already lost by stopping a server.
