# Agent version compatibility

Every stable release at or above the minimum below is compatible, including later minor and major lines. Compatibility permits the existing adapter contract; it does not certify an untested release or change provider capabilities.

| Provider | Compatible range | Recorded native reporting versions |
| --- | --- | --- |
| Claude | >=2.1.267 | 2.1.267, 2.1.268 |
| Codex | >=0.153.0 | 0.153.0 |
| Pi | >=0.85.1 | 0.85.1 |
| Oh My Pi | >=18.2.2 | 18.1.19 |
| Grok | >=1.0.40 | none (history retention only) |

Codex 0.158+ delivers hooks through a detached `app-server --managed-daemon`; managed OVRCR launches refresh that daemon so reporters inherit a live private channel. Prefer a real absolute `CODEX_HOME` (not a `/tmp` symlink). Delivery on 0.158 is not yet a recorded native acceptance row.

Prereleases, build-suffixed versions, malformed versions and versions below the floor are outside the compatible range. Historical tested versions can fall outside the current range: OMP 18.1.19 reporting evidence does not certify 18.2.2 recovery.

`agent doctor PROVIDER --json` separates `compatible_versions` and `tested_versions`, and reports `version_compatible` and `version_tested` for the observed executable. These replace the ambiguous `supported_versions` field. No successful probe proves hook trust, delivery, conversation attachment or native continuity.

Claude and Codex use the range in their existing launch admission checks. Failed probes still leave native execution available with reporting unavailable. Claude's long `--resume UUID` works throughout the range; `-r UUID` is accepted from 2.1.268 onward. Native UUID, hook authentication, ownership, history and configuration checks remain unchanged.

Pi and OMP preserve their existing capability-based managed launches: no version probe or gate is added to startup. Doctor marks versions outside the range for review (`version_gate: diagnostic_only`); runtime extension and producer validation remain authoritative. A version outside the range is not advertised as compatible, even if reporting happens to work.

Grok uses the range in its managed launch admission: below it, or on a failed probe, the native command runs unchanged and no history is retained. Other providers have no version-gated adapter to relax. This policy adds no reporting or recovery support to them. Codex exact resume reports Observed Ready after certified SessionStart(source=resume); Pi Ready remains Confirmed and OMP Ready remains Observed.
