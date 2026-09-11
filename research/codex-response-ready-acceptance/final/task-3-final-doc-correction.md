# Final documentation correction

Changed only `docs/codex-reporting-setup.md` and `docs/agent-reporting-support.md`.

Both now state that reporting loss marks health Unavailable while preserving the last activity as a historical observation. Retained Ready does not establish current readiness. Dashboard reconnect retains the observation while the server remains alive; new observed activity replaces it. Existing planned/pending Task 4 capability markers remain unchanged.

Reviewed the two-file diff. `rtk proxy git diff --check -- docs/codex-reporting-setup.md docs/agent-reporting-support.md` passed (exit 0, no output). No test suite was run for this prose-only correction. No production, UI, plan, staging or commit changes.
