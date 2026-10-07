# Agent versions

OVRCR does not gate any agent harness on its version. Launch admission, reporting, retention and account quota decide support from argv, hooks, conversation identity and the native protocol itself. A release that changes one of those surfaces fails on that surface (for example reporting unavailable, or quota "method not supported" / "unrecognized codex response"), never because of a version string.

Doctors still probe `--version` as a diagnostic and print it beside the releases that have recorded native evidence:

| Provider | Recorded native reporting versions |
| --- | --- |
| Claude | 2.1.267, 2.1.268 |
| Codex | 0.153.0 |
| Pi | 0.85.1 |
| Oh My Pi | 18.1.19 |
| Grok | none (history retention only) |

`agent doctor PROVIDER --json` reports `version`, `tested_versions`, `version_tested` and `version_gate: "none"`. An untested release is untested evidence, not unsupported. The former `compatible_versions` / `version_compatible` fields and the minimum-version floors (Claude >=2.1.267, Codex >=0.153.0, Pi >=0.85.1, Oh My Pi >=18.2.2, Grok >=1.0.40) were removed with the gates. No successful probe proves hook trust, delivery, conversation attachment or native continuity.

Claude accepts both `--resume UUID` and the separate-token `-r UUID` on every release. Native UUID, hook authentication, ownership, history and configuration checks remain unchanged.

Codex 0.158+ delivers hooks through a detached `app-server --managed-daemon`; managed OVRCR launches refresh that daemon so reporters inherit a live private channel. Prefer a real absolute `CODEX_HOME` (not a `/tmp` symlink). Delivery on 0.158 is not yet a recorded native acceptance row.

Account quota: Codex `app-server` `account/read` and `account/rateLimits/read` were verified on 0.155.1, 0.160.1 and 0.161.0. See [provider quota](provider-quota.md).
