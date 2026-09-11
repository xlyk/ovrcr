# Domain docs

OVRCR uses a single-context layout:

- `CONTEXT.md` at the repository root defines domain vocabulary.
- `docs/adr/` holds architectural decision records.

Before exploring an area, read the domain context and relevant ADRs,
alongside the development guides required by AGENTS.md.

If context or ADR files do not exist, proceed silently. Do not create
placeholders. Domain-modeling work creates them when terminology or
decisions are actually resolved.

Use glossary terminology consistently in specifications, issues,
implementation and tests. Identify genuine terminology gaps without
inventing competing names.

Surface conflicts with existing ADRs explicitly. Do not silently
override agreed architecture.
