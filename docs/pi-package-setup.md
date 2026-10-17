# How to install the local Pi orchestration package

Use the repository package to discover the OVRCR orchestrator skill beside the
managed extension. Keep the checkout present: Pi loads a local package in place.
Use a build of OVRCR from the same reviewed revision as the package.

## Install in the intended profile

Choose an absolute checkout and the Pi profile you intend to change. For an
isolated trial, create a fresh private profile and keep its path for removal:

```sh
mkdir -m 700 /absolute/private/pi-profile
PI_CODING_AGENT_DIR=/absolute/private/pi-profile pi install /absolute/ovrcr
```

This installs from local files; no npm publication is required. Pi's normal
personal install writes its selected profile's settings. If choosing project-local
installation with pi install --local instead, follow Pi's project-trust decision
for that project. Use the same profile on later launches. Do not install into an
unrelated profile to repair a capability gap.

## Use direct delegation

Inside the intended OVRCR terminal, launch a managed Pi parent:

```sh
PI_CODING_AGENT_DIR=/absolute/private/pi-profile ovrcr agent run pi -- pi
```

The existing Dashboard Pi picker is another managed-launch route. Normal reporting
injection remains enabled alongside package discovery. The package and injected
entry share one runtime initializer; discovery does not create a second Reporter
or source credential. A plain Pi process has no OVRCR delegation authority.

Ask for bounded research, a review or coding directly. Pocock stage skills, a
specification and tracker tickets are unnecessary for those requests. An explicit
full-workflow request uses its installed stage procedures and their actual gates.
The parent checks ovrcr_capabilities for available/allowed selections before
starting. A missing requested provider/model is reported rather than substituted.

Read [assignments](pi-research-assignments.md) for the public tools and
[terminal fallback](pi-terminal-fallback.md) for missing-result recovery. Collection,
Ready and recovery publication leave acceptance pending. Coding needs the
ordinary authenticated child candidate/checks; parent fallback cannot accept it.
Required independent review remains a requirement when its procedure is unavailable.
No task record grants commit, publication, push or merge permission.

## Resolve an unavailable capability

If package discovery fails, check the configured local path still exists and the
same profile is selected. If tools are absent/inactive or the selected managed
entry did not load, preserve Pi's reported load error and launch through the
matching OVRCR build. A selected-entry reference is diagnostic metadata, not proof
a factory ran or a source was admitted. The package never chooses another OVRCR
binary from PATH to create authority.

Use ovrcr agent doctor pi --json for tested/untested/unknown version and reporting
diagnostics. An untested version is unverified evidence; it is not automatically
unsupported. Managed launches do not perform a version probe or enforce a version
allowlist. Server refusals and unavailable requested provider/model/limits stay
visible. An absent optional Pi keybinding API keeps reporting operational and
native interruption unavailable; it does not justify a guessed key or signal.

For a surviving workflow, reconcile through ovrcr_recover before further input;
keep original handles after unknown/partial outcomes. Save a concise checkpoint
index pointing to the one workflow record, continuation note and original evidence.
Archive only exact owned runs after a current eligible delivery. Evidence/worktrees
survive archival; worktree removal follows separate repository policy.

## Remove the package

Remove only the source from the same profile used for installation:

```sh
PI_CODING_AGENT_DIR=/absolute/private/pi-profile pi remove /absolute/ovrcr
```

This removes package discovery from that profile. Existing managed reporting
injection remains the ordinary OVRCR launch behavior. Keep task evidence and any
profile/session resources whose retention is still required; stop or remove only
resources whose ownership is known. See [reporting setup](pi-reporting-setup.md)
for the existing reporting route and diagnostics.

## Verify the installed loader

The dedicated loader gate uses `@earendil-works/pi-coding-agent@0.87.1` and
Node 22.19 or later. CI provisions that test host under its runner temporary
directory and retains the resolved npm lockfile and test log. The host is a test
dependency; local package installation requires no npm publication.

With that host already installed, run:

```sh
OVRCR_TEST_PI_PACKAGE_ROOT=/absolute/path/to/node_modules/@earendil-works/pi-coding-agent \
  cargo test -p ovrcr --test pi_package --features acceptance-diagnostics \
  production_materializer_loads_local_package_with_one_injected_owner \
  -- --ignored --exact --nocapture --test-threads=1
```

This explicit gate runs one Rust fixture and requires two passing installed SDK
cases. It is excluded from ordinary tests because it requires a provisioned host;
the dedicated command fails if the host is missing or has another version.
The fixture starts the compiled CLI in a managed PTY, receives the actual private
materialized entry, and keeps that invocation alive throughout SDK loading.
It verifies local install/remove, promoted skill discovery, both discovered
entries, and one registration/lifecycle owner across reload. It records exact
process and asset receipts and checks cleanup on failure as well as success.
Authenticated provider work, source admission, and native rendering require
separate acceptance evidence.
