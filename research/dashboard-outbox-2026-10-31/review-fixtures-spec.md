# Scoped fixture Spec review

**Spec verdict: PASS. Both observed failures are addressed; no new serious breakage or scope violation found.**

Scope: `4cb02011...8a58800084ca32b9e98c083a52829e0a74c26184`, only `tests/pi_delegation.rs` and `tests/provider_quota.rs`. The earlier whole-branch verdict stands.

- **Marker completion — addressed** (`tests/pi_delegation.rs:907–918, 992–1015`). Log 39 shows the old exact assertion reading an empty file after creation. The local poll now waits for exact completed contents, retaining the existing 20-second budget and 10-ms cadence. The original delivered assertion remains, pasted contents gain an exact assertion, and the blocked-lifecycle check and one-second request deadlines remain. Shared existence-only barriers retain their semantics.
- **Quota before Hello — addressed** (`tests/provider_quota.rs:1489–1562`). Log 39 shows a correct no-Claude Unavailable event arriving before Hierarchy. The local loop accepts only request-ID-1 Hierarchy, retains early quota evidence, checks retained reasons for native-output leakage, and applies the original exact state/reason assertions to the first matching snapshot. Conflict retries remain bounded; unrelated frames still fail. A five-second greeting deadline prevents an event stream from indefinitely postponing the expected response. The unchanged shared helper preserves other tests' first-snapshot observations.

The scope judgment is sound: these are bounded observation corrections needed to evaluate the required final gates, not unrelated product behavior corrections. They align with ticket line 35, “no assertions are weakened to make the migration pass,” and line 39's final regression/platform requirement. Neither change alters production code or suppresses the actual behavioral checks. The quota loop retains early evidence rather than merely discarding it.

Inspected original RED excerpts and logs 42–45: each corresponding focused case passed once on macOS and once on Linux. Those results establish focused recovery only; they do not certify full suites or final gates.

No tests, builds, UI, network, source edits, or delegation were performed by this reviewer. Full workspace/platform acceptance remains parent-owned and pending; no final-gate completion is claimed.
