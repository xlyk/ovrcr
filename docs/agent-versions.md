# Agent version compatibility

Stable patch releases in the approved major/minor line are compatible from the minimum below. Compatibility permits the existing adapter contract; it does not certify an untested release or change provider capabilities.

| Provider | Compatible range | Recorded native reporting versions |
| --- | --- | --- |
| Claude | >=2.1.267, <2.2.0 | 2.1.267, 2.1.268 |
| Codex | >=0.153.0, <0.154.0 | 0.153.0 |
| Pi | >=0.85.1, <0.86.0 | 0.85.1 |
| Oh My Pi | >=18.2.2, <18.3.0 | 18.1.19 |

New minor/major versions require review. Prereleases, build-suffixed versions, malformed versions and versions below the floor are outside the compatible range. Historical tested versions can fall outside the current range: OMP 18.1.19 reporting evidence does not certify 18.2.2 recovery.

`agent doctor PROVIDER --json` separates `compatible_versions` and `tested_versions`, and reports `version_compatible` and `version_tested` for the observed executable. These replace the ambiguous `supported_versions` field. No successful probe proves hook trust, delivery, conversation attachment or native continuity.

Claude and Codex use the range in their existing launch admission checks. Failed probes still leave native execution available with reporting unavailable. Claude's long `--resume UUID` works throughout the range; `-r UUID` is accepted from 2.1.268 onward. Native UUID, hook authentication, ownership, history and configuration checks remain unchanged.

Pi and OMP preserve their existing capability-based managed launches: no version probe or gate is added to startup. Doctor marks versions outside the range for review (`version_gate: diagnostic_only`); runtime extension and producer validation remain authoritative. A version outside the range is not advertised as compatible, even if reporting happens to work.

Other providers have no version-gated adapter to relax. This policy adds no reporting or recovery support to them. Codex resumed reporting remains unavailable for the entire invocation; Pi Ready remains Confirmed and OMP Ready remains Observed.
