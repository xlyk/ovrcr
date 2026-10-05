# Server decisions are recorded as durable events

Decided 2026-10-02, from the event-log grilling (item 6).

Quota readings and settings readings are not persisted (ADR 0007, `provider-usage-spec.md`): they describe accounts and documents OVRCR does not own, and a restart must not resurrect them. Events are different. An Event is OVRCR's own one-line account of a decision the Server made, written by OVRCR, bounded, and free of prompts, transcripts, credentials, account identifiers and native bodies by the same rule that governs a Quota reason. Recording them durably costs nothing the privacy rulings protect and gives a post-mortem for a title that never appeared or a quota row that went dark.

So the Server keeps a bounded in-memory ring it publishes to the Dashboard, and appends every event to `events.jsonl` in the instance directory, one JSON line each, mode 0600, size-capped with one rotation. stderr keeps its lines. The Dashboard popup and `ovrcr events` read the ring through the Server; the file is for people and other tools.

Rejected: tailing `server.log` (unstructured, and the installed service writes it elsewhere); a table in the registry database (every event a transaction and a schema bump); a file beside the socket (lost with the runtime directory).
