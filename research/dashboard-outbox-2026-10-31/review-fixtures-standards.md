# Scoped fixture Standards review

**Verdict: PASS — no new blocking findings, documented-standard breaches, or material loss of coverage found.**

Scope: exact committed diff `4cb02011febebf7b2da46620d80acd309a294ffa...8a58800084ca32b9e98c083a52829e0a74c26184`, limited to `tests/pi_delegation.rs` and `tests/provider_quota.rs`. Production code is unchanged. This does not replace the earlier whole-branch review.

- `tests/pi_delegation.rs:908`: the local content wait uses the existing 20-second budget and 10-ms cadence. It observes the actual completed child receipt instead of shell-redirection file creation. The exact `delivered` assertion remains, `pasted` gains an exact assertion, the blocked-lifecycle assertion remains, and one-second request bounds remain. Shared `wait_file` retains existence semantics for intentional empty barriers such as `git-entered`. This follows `docs/development/testing.md`: synchronize on the disputed boundary, preserve existing assertions, and verify actual child output.
- `tests/provider_quota.rs:1491`: the local greeting loop retains pre-Hierarchy QuotaChanged snapshots, requires request ID 1 for Hierarchy/Conflict, keeps the three-second conflict retry bound and five-second greeting budget, and restores the existing socket timeout. All retained snapshots undergo the same reason-redaction assertions as `quota_until`; the first qualifying snapshot still receives the original exact Unavailable state and `claude not found on PATH` reason assertions. The unchanged fallback retains its ten-second budget. This preserves observation evidence rather than discarding early frames, consistent with the same testing rules.
- Producer inspection confirms Dashboard claim precedes Hierarchy enqueue (`crates/ovrcr-runtime/src/server/connections.rs:218`–249), making the recorded early event possible. The shared `attach` remains unchanged for callers asserting first Checking snapshots and collector counts. Direct frame reads are appropriate for this Dashboard-role test under `docs/development/architecture.md`.

The original failure bodies in log 39 show an empty marker and the early Unavailable quota event, matching the corrected observation boundaries. No new baseline smell warrants a separate finding in this bounded diff.

Skipped: tests, builds, UI, network, and the Spec axis, as assigned. Root-owned final workspace and Linux lint gates remain unverified here; this review does not certify final delivery readiness.
