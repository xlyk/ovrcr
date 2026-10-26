# ADR 0011: Cursor cloud launches use published workspace source

Status: Accepted in [issue #358](https://github.com/xlyk/ovrcr/issues/358).
Implemented boundaries: account/source/model validation preview (#372), durable
local handle preparation (#373), immutable Send and exact-run results (#374).

A Cursor cloud launch uses the selected workspace's canonical GitHub origin and
exact HEAD commit. The commit must appear in the origin's published refs. The
selected account policy must allow that repository or its owner, and Cursor's
selected-key repository catalog must confirm access. Its explicit model must
also appear in that credential's model catalog. Missing credentials, publication,
policy or model access refuse launch; no ambient credential or model fallback.

The visible preview names the account policy, repository, full SHA and model
before any prompt. Local working-tree edits are excluded. Launch never uploads,
commits or pushes local changes. A profile name or successful repository lookup
does not attest the remote billing identity.

The preview remains read-only until explicit prepare. The later creation/Send
slices retain independent durable claims and exact identity receipts. Terminal
responses, uncertain retries and active reattachment remain gated by official
capability evidence and their later sequential implementation slices.
