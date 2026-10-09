# OVRCR palette implementation transfer

## Contents
- OVRCR-approved-palette-manifest.json: authoritative transfer roster, 80 unique preview IDs, exact gallery tokens, source provenance, protections, and pair-level contrast checks.
- canonical-source-palettes.json: original sourced palette values, including alpha channels and intentionally absent hues.
- contrast-review.json: all pair-level flags.
- LICENSE-NOTICES.txt: upstream notices.
- SHA256SUMS.txt: integrity hashes for the package contents.

## Required handoff checks
1. Verify the hashes and manifest totals: 27 families, 80 variants, 72 additions, 8 existing protected entries.
2. Group the Settings browser using explicit appearance/appearance_group metadata (Dark/Light), backed by appearance_evidence. Do not infer grouping from RGB brightness. Inspect the current OVRCR theme registry before choosing implementation IDs. Preserve the existing 8 definitions and every historical alias. The gallery ID is a transfer identifier, not an existing OVRCR enum/serialized ID.
3. Use gallery tokens as the approved visual reference, not a guarantee of implementation readiness. The source data remains separate from derived semantic mapping. Review the full contrast report and provenance before importing.
4. Preserve visible differentiation among focus, selection, errors, warnings, and success across all themes; ensure meaning is also conveyed without color. If a palette requires role remapping for legibility, document the change rather than representing it as canonical upstream color data.
5. Implement the Settings browse-preview flow using the local app's conventions. Cancel must preserve the prior saved theme; applying must persist. Do not recolor agent PTY content.
6. Carry applicable upstream attribution and license notices into the repository. No OVRCR code, proprietary theme sources, credentials, or personal data is included in this archive.

## Exclusions
Shades of Purple, Material Theme/Vira lineage, and Monokai Pro are research-only entries and are excluded from these 80 previews and from implementation approval in this package.

## Existing theme fidelity
Mocha and Latte preview tokens matched the inspected OVRCR commit c9f5cc903c0dffa94d37e83fb20c3cadfc2ab1c3. The other six existing preview entries use upstream adaptations. Preserve all current native definitions rather than replacing them.

## Contrast caveat
Small text uses 4.5:1 as a review reference; focus rails and borders use 3:1. A failing pair is a specific implementation concern, not a whole-theme accessibility verdict. Intended editor font treatment, ANSI contrast correction, selection foregrounds, terminal gamut, and app state can change actual rendering.
