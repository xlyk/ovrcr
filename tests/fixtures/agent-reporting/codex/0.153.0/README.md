# Codex 0.153.0 source fixtures

These are allowlisted public schema fields and CLI help, not captured native events or an executable support allowlist. The adapter is not certified. No conversation bodies, credentials, live configuration, or user session data are retained.

`InitializeParams`, `ThreadReadParams`, and the two notification schemas are verbatim generated files. `request-methods.json` extracts every method enum from generated `ClientRequest.json`; the other two JSON fragments select identity/path fields and the resume contract. Fragments are documentary, not standalone schemas. `provenance.json` records SHA-256 hashes and pinned source correspondence. The locally installed package and public tag agree on 0.153.0; reproducible binary-to-source equivalence was not proved.

See [source decision](../../../../../research/codex-reporting-acceptance/source-contract.md). No synthetic numeric rows are represented as native evidence.
