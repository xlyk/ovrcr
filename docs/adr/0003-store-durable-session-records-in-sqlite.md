# Store durable session records in SQLite

Provisional storage direction, 2026-09-15. Interview defaults below are recorded for comparison; final commitment is deferred pending research into Superset's restoration behavior. Implementation and migration mechanics are pending.

Use SQLite for projects, workspaces and durable session records, including provider conversation references and session metadata. Sessions now outlive the server and can be archived independently of their processes, making a transactional store appropriate; agent conversation history remains owned by each provider rather than copied into this database.

Migrate the existing project/workspace registry into SQLite so related records share one durable authority. User preferences remain in TOML, and existing scheduled-task storage remains unchanged.

This supplies the concrete database requirement for the architecture guide. The synchronous server and single-owner model remain unchanged.
