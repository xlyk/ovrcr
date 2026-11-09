# Python inventory notes

Run 42429838269, job 128576086691, revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`.

## Runners and steps

| runner | ci_step | suite | rows |
|---|---|---|---|
| python-unittest | `Relocated macOS startup assets` | `native/bridge/check-installer.py::InstallerTests` | 33 |
| python-unittest | `Relocated pinned Cursor dependency and passive CLI diagnostics` | `tests/cursor_bridge_install.py::Installation` | 6 |
| python-check | `Relocated macOS startup assets` | `native/bridge/check-local-startup.py` | 17 |
| python-check | `Relocated macOS startup assets` | `native/bridge/check-packaging.py` | 63 |
| python-check | `Relocated macOS startup assets` | `native/bridge/check-startup-package.py` | 40 |
| python-check | `Relocated macOS startup assets` | `native/bridge/check-guards.py` | 80 |
| | | **total** | **239** |

## python-unittest: 39 executed methods

`unittest` sorts test methods by name, so rows are in execution order
(alphabetical), not source order. Source ranges are the `def` block.
No per-method timing is printed: the suite prints one `Ran N tests in T s`
aggregate, so every duration and attempt column is empty with
`timing_basis=unmeasured`.

- `native/bridge/check-installer.py::InstallerTests`: 33 `test_` methods, 33 name lines printed by the runner
- `tests/cursor_bridge_install.py::Installation`: 6 `test_` methods, 0 name lines printed by the runner

| suite | aggregate |
|---|---|
| `native/bridge/check-installer.py` | macOS-job.log:3631 'Ran 33 tests in 10.080s' |
| `tests/cursor_bridge_install.py` | macOS-job.log:3669 'Ran 6 tests in 2.076s' |

The two runner aggregates report 33 and 6 executed methods, matching
the enumerated [33, 6] per file.

Variant: `default` for both files. Neither invocation passes a profile
flag; `check-installer.py` derives its local fixtures from its own cases.

## python-check: 200 sequential logical checks

A row is one executed `check()`/`validate()`/`reject_validation()`/
`check_cache()` invocation, one element of a table-driven loop, or one
standalone block that ends in a check-counter increment. Assertions inside
such a block are not separate rows.

### Reconciliation (hosted total vs enumerated)

| script | variant | hosted | hosted log line | enumerated | match |
|---|---|---|---|---|---|
| `native/bridge/check-guards.py` | local-development | 40 | 3646 | 40 | yes |
| `native/bridge/check-guards.py` | production | 40 | 3636 | 40 | yes |
| `native/bridge/check-local-startup.py` | default | 17 | 3469 | 17 | yes |
| `native/bridge/check-packaging.py` | local-development | 36 | 3645 | 36 | yes |
| `native/bridge/check-packaging.py` | production | 27 | 3635 | 27 | yes |
| `native/bridge/check-startup-package.py` | local-development | 28 | 3644 | 28 | yes |
| `native/bridge/check-startup-package.py` | production | 12 | 3634 | 12 | yes |
| | | **200** | | **200** | yes |

Mode attribution follows the workflow command order: the step runs
`check-local-startup.py`, `check-installer.py`, then
`check-startup-package.py`, `check-packaging.py`, `check-guards.py`
without a flag, then the last three again with `--local-development`.
The script totals appear in the log in exactly that order, which the
enumeration asserts before pairing them.

### Per-script site map (execution order)

| script | variant | label | source | kind | rows |
|---|---|---|---|---|---|
| `check-guards.py` | production | `entitlement-pin` | 28-35 | block | 1 |
| `check-guards.py` | production | `contract-available` | 50-50 | call | 1 |
| `check-guards.py` | production | `contract-schema-newer` | 51-51 | call | 1 |
| `check-guards.py` | production | `contract-wire-newer` | 52-52 | call | 1 |
| `check-guards.py` | production | `contract-argv` | 53-55 | for | 4 |
| `check-guards.py` | production | `client-extra-argv` | 56-56 | call | 1 |
| `check-guards.py` | production | `client-input` | 57-58 | for | 6 |
| `check-guards.py` | production | `client-metadata` | 59-64 | for | 4 |
| `check-guards.py` | production | `client-op` | 65-70 | for | 5 |
| `check-guards.py` | production | `client-sound` | 71-75 | for | 6 |
| `check-guards.py` | production | `open-stdin-timeout` | 78-94 | block | 1 |
| `check-guards.py` | production | `opposite-profile-metadata` | 97-97 | call | 1 |
| `check-guards.py` | production | `opposite-profile-extra-argv` | 98-98 | call | 1 |
| `check-guards.py` | production | `wrong-profile-open-stdin-exits` | 100-116 | block | 1 |
| `check-guards.py` | production | `guard-marker` | 138-141 | for | 4 |
| `check-guards.py` | production | `bundle-id-local` | 142-144 | call | 1 |
| `check-guards.py` | production | `bundle-id-private-unmarked` | 145-147 | call | 1 |
| `check-guards.py` | local-development | `entitlement-pin` | 28-35 | block | 1 |
| `check-guards.py` | local-development | `contract-available` | 50-50 | call | 1 |
| `check-guards.py` | local-development | `contract-schema-newer` | 51-51 | call | 1 |
| `check-guards.py` | local-development | `contract-wire-newer` | 52-52 | call | 1 |
| `check-guards.py` | local-development | `contract-argv` | 53-55 | for | 4 |
| `check-guards.py` | local-development | `client-extra-argv` | 56-56 | call | 1 |
| `check-guards.py` | local-development | `client-input` | 57-58 | for | 6 |
| `check-guards.py` | local-development | `client-metadata` | 59-64 | for | 4 |
| `check-guards.py` | local-development | `client-op` | 65-70 | for | 5 |
| `check-guards.py` | local-development | `client-sound` | 71-75 | for | 6 |
| `check-guards.py` | local-development | `open-stdin-timeout` | 78-94 | block | 1 |
| `check-guards.py` | local-development | `opposite-profile-metadata` | 97-97 | call | 1 |
| `check-guards.py` | local-development | `opposite-profile-extra-argv` | 98-98 | call | 1 |
| `check-guards.py` | local-development | `wrong-profile-open-stdin-exits` | 100-116 | block | 1 |
| `check-guards.py` | local-development | `local-metadata-mutation` | 124-136 | for | 6 |
| `check-packaging.py` | production | `entitlement-pin` | 25-32 | block | 1 |
| `check-packaging.py` | production | `source-accepted` | 44-44 | call | 1 |
| `check-packaging.py` | production | `bundle-copy-rejected` | 47-99 | for | 22 |
| `check-packaging.py` | production | `source-symlink-rejected` | 100-102 | call | 1 |
| `check-packaging.py` | production | `fixture-identity-rejected` | 109-109 | call | 1 |
| `check-packaging.py` | production | `fixture-identity-accepted-production` | 114-114 | call | 1 |
| `check-packaging.py` | local-development | `entitlement-pin` | 25-32 | block | 1 |
| `check-packaging.py` | local-development | `source-accepted` | 44-44 | call | 1 |
| `check-packaging.py` | local-development | `bundle-copy-rejected` | 47-99 | for | 22 |
| `check-packaging.py` | local-development | `source-symlink-rejected` | 100-102 | call | 1 |
| `check-packaging.py` | local-development | `fixture-identity-rejected` | 109-109 | call | 1 |
| `check-packaging.py` | local-development | `fixture-identity-accepted-production` | 114-114 | call | 1 |
| `check-packaging.py` | local-development | `local-source-production-profile-rejected` | 116-116 | call | 1 |
| `check-packaging.py` | local-development | `local-source-production-identity-rejected` | 117-117 | call | 1 |
| `check-packaging.py` | local-development | `local-source-identity-argument-rejected` | 118-118 | call | 1 |
| `check-packaging.py` | local-development | `local-marker-rejected` | 119-129 | for | 4 |
| `check-packaging.py` | local-development | `production-profile-from-local-rejected` | 137-137 | call | 1 |
| `check-packaging.py` | local-development | `production-profile-accepted` | 138-138 | call | 1 |
| `check-startup-package.py` | production | `relocated-validator-accepts` | 32-33 | block | 1 |
| `check-startup-package.py` | production | `changed-helper-rejected` | 41-42 | block | 1 |
| `check-startup-package.py` | production | `entitlement-pin` | 45-53 | block | 1 |
| `check-startup-package.py` | production | `missing-entitlements-rejected` | 54-57 | block | 1 |
| `check-startup-package.py` | production | `entitlement-layout-rejected` | 59-75 | for | 5 |
| `check-startup-package.py` | production | `changed-purpose-rejected` | 82-86 | block | 1 |
| `check-startup-package.py` | production | `changed-sound-rejected` | 88-92 | block | 1 |
| `check-startup-package.py` | production | `install-refuses-ad-hoc` | 93-94 | block | 1 |
| `check-startup-package.py` | local-development | `relocated-validator-accepts` | 32-33 | block | 1 |
| `check-startup-package.py` | local-development | `changed-helper-rejected` | 41-42 | block | 1 |
| `check-startup-package.py` | local-development | `entitlement-pin` | 45-53 | block | 1 |
| `check-startup-package.py` | local-development | `missing-entitlements-rejected` | 54-57 | block | 1 |
| `check-startup-package.py` | local-development | `entitlement-layout-rejected` | 59-75 | for | 5 |
| `check-startup-package.py` | local-development | `changed-purpose-rejected` | 82-86 | block | 1 |
| `check-startup-package.py` | local-development | `changed-sound-rejected` | 88-92 | block | 1 |
| `check-startup-package.py` | local-development | `install-refuses-ad-hoc` | 93-94 | block | 1 |
| `check-startup-package.py` | local-development | `local-production-validation-rejected` | 108-109 | call | 1 |
| `check-startup-package.py` | local-development | `local-identity-arguments-rejected` | 110-111 | call | 1 |
| `check-startup-package.py` | local-development | `local-contract-profile-pin-rejected` | 116-117 | call | 1 |
| `check-startup-package.py` | local-development | `local-production-metadata-rejected` | 118-118 | call | 1 |
| `check-startup-package.py` | local-development | `contract-field-rejected` | 120-124 | for | 4 |
| `check-startup-package.py` | local-development | `local-marker-rejected` | 126-133 | for | 4 |
| `check-startup-package.py` | local-development | `cache-hit-local-profile` | 221-221 | call | 1 |
| `check-startup-package.py` | local-development | `converted-production-metadata-valid` | 225-233 | block | 1 |
| `check-startup-package.py` | local-development | `cache-miss-production-profile` | 234-234 | call | 1 |
| `check-startup-package.py` | local-development | `cache-miss-relocated-profile-pin` | 239-239 | call | 1 |
| `check-local-startup.py` | default | `run-recipe-production` | 120-131 | block | 1 |
| `check-local-startup.py` | default | `run-recipe-local-marker` | 134-144 | block | 1 |
| `check-local-startup.py` | default | `run-recipe-explicit-opt-out` | 147-152 | for | 2 |
| `check-local-startup.py` | default | `run-recipe-invalid-opt-in` | 156-161 | for | 4 |
| `check-local-startup.py` | default | `unmanaged-local-assets-preserved` | 163-171 | block | 1 |
| `check-local-startup.py` | default | `symlinked-local-assets-refused` | 173-181 | block | 1 |
| `check-local-startup.py` | default | `linux-kernel-build-path` | 183-188 | block | 1 |
| `check-local-startup.py` | default | `install-assets-production` | 190-206 | block | 1 |
| `check-local-startup.py` | default | `install-assets-local` | 212-219 | block | 1 |
| `check-local-startup.py` | default | `install-assets-refusal` | 221-225 | for | 3 |
| `check-local-startup.py` | default | `unmanaged-release-assets-preserved` | 227-233 | block | 1 |

### Notes on specific counters

**check-guards.py** keeps one `checks` counter and increments it for
every `check()` call plus the two standalone stdin-pipe blocks and the
entitlement-pin block: 34 increments before the metadata mutation, then
6 more in either branch (4 marker values + 2 bundle ids in production,
6 metadata keys in local development). 40 per mode is exact.

**check-packaging.py** counts the entitlement-pin block, `validate()`
calls and each of the 22 mutated bundle copies: 27 in production, plus 9
local-profile checks (3 identity refusals, 4 marker values, 1 production
profile refusal, 1 explicit production profile acceptance) = 36.

**check-startup-package.py** sets `checks = 6 + layout_checks` after the
relocated validations. The six base checks are the pristine acceptance,
the changed-helper refusal, the pinned-entitlement block, the changed
purpose, the changed sound and the `install-bridge.sh` non-ad-hoc refusal;
`layout_checks` starts at 1 (missing entitlements) and grows by the five
entitlement-layout cases, so 6 + 6 = 12. Local development adds 16: four
`reject_validation` calls, four changed contract fields, four invalid
markers, the converted production metadata acceptance and three
`check_cache` runs = 28. The pin block is counted once, in the base six;
the six `layout_checks` units are the missing-entitlement refusal plus the
five loop cases, so no block is counted twice.

**check-local-startup.py** has no profile flag of its own: the workflow
invokes it bare, and the script drives both the production and the
local-development just recipes inside its private fixture, so the same 17
checks cover both profiles. Its rows therefore carry `variant=default` 
(no profile argument on the command line) rather than `production` or
`local-development`, which would misdescribe half of what each row runs.
Each row's label names the recipe path it exercises
(`run-recipe-production`, `run-recipe-local-marker`, `install-assets-*`).

### Scope

Only commands of job `128576086691` (macOS checks) are inventoried. The sibling job
`macOS offline Cursor preflight` runs
`python3 -I -m unittest discover -s experiments/cursor-cloud -p test_client.py -v`,
which is a different job and therefore outside these rows; so is the
`linux` job's copy of the same command.

Neither check script is imported by this enumeration: they mutate the
filesystem at import time (`__main__` bodies run immediately), so the
enumeration reads them with `ast.parse` and matches the printed totals.

### Timing

Every python row leaves `first_attempt_seconds`, `total_attempt_seconds`,
`attempt_count` and `failed_attempt_count` empty with
`timing_basis=unmeasured`. The scripts print only one summary line each,
and the buffered CI timestamps are not attributable to a single check, so
no per-check time is derived from a suite duration or from timestamp gaps.

### Label construction

Labels are frozen `script-local-qualified` names. Loop-generated checks
carry the iteration element and the loop's line as
`label[<unparsed element>]:ln<anchor>`, so the mapping back to source is
deterministic. Non-loop checks carry `label:ln<anchor>`. `source_start`..
`source_end` is the loop statement for loop-generated checks, the call
statement for single calls, and the enclosing block for standalone checks.
