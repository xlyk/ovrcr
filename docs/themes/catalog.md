# Dashboard theme catalog

The catalog has all 80 approved gallery variants: 54 Dark and 26 Light across 27 families. Eight native definitions are preserved; 72 are additions. Appearance is explicit source metadata.

The first eight enum variants and wire indices, IDs, labels, historical `dark`/`light` aliases, default, and all fifteen native RGB tokens are preserved. Gallery IDs map explicitly below; they do not replace native IDs. New Rosé Pine and Dracula spellings also accept their gallery IDs as aliases.

## Semantic mapping

The exact source palettes, gallery reference tokens, classification evidence, upstream copyright/license notices, and original contrast report are retained in [source](source/TRANSFER-README.md). [semantic-mappings.json](semantic-mappings.json) records every implemented token, source reference, adaptation, and actual contrast pair. Regenerate tables with `python3 scripts/generate_theme_catalog.py` and then `cargo fmt --all`.

For new themes, BASE and CRUST retain the approved gallery canvases. TEXT, SUBTEXT, MUTED, and all accent/status slots are adjusted only when needed to reach 4.5:1 on BASE, CRUST, and SURFACE0. SURFACE2 borders reach 3:1. Adjustments blend the gallery role toward white on dark canvases or black on light canvases; they are OVRCR adaptations, never represented as canonical upstream colors. This also checks CRUST/MAUVE focus-label inversion and BASE/YELLOW warning inversion used by the app.

Selected rows share TEXT with ordinary rows. If upstream SURFACE0 requires an inverted selection foreground or is indistinguishable from BASE, OVRCR uses a neutral surface with readable TEXT and at least 1.2:1 separation from BASE. This handles high-contrast editor selection mappings deliberately; the source selection pair is kept in canonical-source-palettes.json. A high-contrast variant name identifies the upstream variant and is not an accessibility certification.

Vesper supplies no canonical blue or purple; OVRCR BLUE/MAUVE use its sourced warm accent. Oxocarbon supplies no yellow; YELLOW/PEACH reuse the sourced cyan accent. These fallbacks retain the theme's palette and existing visible labels/symbols for warning, error, success, focus, and selection. Hues alone do not encode status. Solarized Light's primary text is darkened for the OVRCR small-text role because the canonical pair is approximately 4.13:1.

Protected existing themes retain some below-reference pairs for compatibility; the audit reports those ratios without claiming a blanket accessibility pass. No agent ANSI/RGB palette, terminal renderer, Settings interaction, or notification code is changed by this catalog.

## Exact native/approved roster

| Native ID | Approved gallery ID | Label | Appearance | Family | Disposition |
| --- | --- | --- | --- | --- | --- |
| `catppuccin-mocha` | `catppuccin-mocha` | Catppuccin Mocha | Dark | Catppuccin | existing; preserved |
| `catppuccin-latte` | `catppuccin-latte` | Catppuccin Latte | Light | Catppuccin | existing; preserved |
| `tokyo-night` | `tokyo-night` | Tokyo Night | Dark | Tokyo Night | existing; preserved |
| `dracula` | `dracula-theme` | Dracula Theme | Dark | Dracula | existing; preserved |
| `gruvbox-dark` | `gruvbox-dark` | Gruvbox Dark | Dark | Gruvbox | existing; preserved |
| `gruvbox-light` | `gruvbox-light` | Gruvbox Light | Light | Gruvbox | existing; preserved |
| `nord` | `nord` | Nord | Dark | Nord | existing; preserved |
| `rose-pine` | `ros-pine` | Rosé Pine | Dark | Rosé Pine | existing; preserved |
| `catppuccin-macchiato` | `catppuccin-macchiato` | Catppuccin Macchiato | Dark | Catppuccin | added |
| `catppuccin-frappe` | `catppuccin-frappe` | Catppuccin Frappé | Dark | Catppuccin | added |
| `rose-pine-moon` | `ros-pine-moon` | Rosé Pine Moon | Dark | Rosé Pine | added |
| `rose-pine-dawn` | `ros-pine-dawn` | Rosé Pine Dawn | Light | Rosé Pine | added |
| `kanagawa-wave` | `kanagawa-wave` | Kanagawa Wave | Dark | Kanagawa | added |
| `kanagawa-dragon` | `kanagawa-dragon` | Kanagawa Dragon | Dark | Kanagawa | added |
| `kanagawa-lotus` | `kanagawa-lotus` | Kanagawa Lotus | Light | Kanagawa | added |
| `everforest-dark-hard` | `everforest-dark-hard` | Everforest Dark Hard | Dark | Everforest | added |
| `everforest-dark-medium` | `everforest-dark-medium` | Everforest Dark Medium | Dark | Everforest | added |
| `everforest-dark-soft` | `everforest-dark-soft` | Everforest Dark Soft | Dark | Everforest | added |
| `everforest-light-hard` | `everforest-light-hard` | Everforest Light Hard | Light | Everforest | added |
| `everforest-light-medium` | `everforest-light-medium` | Everforest Light Medium | Light | Everforest | added |
| `everforest-light-soft` | `everforest-light-soft` | Everforest Light Soft | Light | Everforest | added |
| `nightfox` | `nightfox` | Nightfox | Dark | Nightfox | added |
| `dayfox` | `dayfox` | Dayfox | Light | Nightfox | added |
| `dawnfox` | `dawnfox` | Dawnfox | Light | Nightfox | added |
| `duskfox` | `duskfox` | Duskfox | Dark | Nightfox | added |
| `nordfox` | `nordfox` | Nordfox | Dark | Nightfox | added |
| `terafox` | `terafox` | Terafox | Dark | Nightfox | added |
| `carbonfox` | `carbonfox` | Carbonfox | Dark | Nightfox | added |
| `zenbones-dark` | `zenbones-dark` | Zenbones Dark | Dark | Zenbones / Zenwritten | added |
| `zenbones-light` | `zenbones-light` | Zenbones Light | Light | Zenbones / Zenwritten | added |
| `zenwritten-dark` | `zenwritten-dark` | Zenwritten Dark | Dark | Zenbones / Zenwritten | added |
| `zenwritten-light` | `zenwritten-light` | Zenwritten Light | Light | Zenbones / Zenwritten | added |
| `flexoki-dark` | `flexoki-dark` | Flexoki Dark | Dark | Flexoki | added |
| `flexoki-light` | `flexoki-light` | Flexoki Light | Light | Flexoki | added |
| `synthwave-84` | `synthwave-84` | SynthWave ’84 | Dark | SynthWave ’84 | added |
| `snazzy` | `snazzy` | Snazzy | Dark | Snazzy | added |
| `oxocarbon-dark` | `oxocarbon-dark` | Oxocarbon Dark | Dark | Oxocarbon | added |
| `oxocarbon-light` | `oxocarbon-light` | Oxocarbon Light | Light | Oxocarbon | added |
| `poimandres` | `poimandres` | Poimandres | Dark | Poimandres | added |
| `poimandres-storm` | `poimandres-storm` | Poimandres Storm | Dark | Poimandres | added |
| `horizon` | `horizon` | Horizon | Dark | Horizon | added |
| `horizon-bright` | `horizon-bright` | Horizon Bright | Light | Horizon | added |
| `andromeda` | `andromeda` | Andromeda | Dark | Andromeda | added |
| `andromeda-bordered` | `andromeda-bordered` | Andromeda Bordered | Dark | Andromeda | added |
| `vesper` | `vesper` | Vesper | Dark | Vesper | added |
| `vs-code-dark` | `vs-code-dark` | VS Code Dark+ | Dark | VS Code Defaults | added |
| `vs-code-light` | `vs-code-light` | VS Code Light+ | Light | VS Code Defaults | added |
| `vs-code-dark-high-contrast` | `vs-code-dark-high-contrast` | VS Code Dark High Contrast | Dark | VS Code Defaults | added (upstream high contrast) |
| `vs-code-light-high-contrast` | `vs-code-light-high-contrast` | VS Code Light High Contrast | Light | VS Code Defaults | added (upstream high contrast) |
| `github-dark-default` | `github-dark-default` | GitHub Dark Default | Dark | GitHub | added |
| `github-light-default` | `github-light-default` | GitHub Light Default | Light | GitHub | added |
| `github-dark-dimmed` | `github-dark-dimmed` | GitHub Dark Dimmed | Dark | GitHub | added |
| `github-dark-high-contrast` | `github-dark-high-contrast` | GitHub Dark High Contrast | Dark | GitHub | added (upstream high contrast) |
| `github-light-high-contrast` | `github-light-high-contrast` | GitHub Light High Contrast | Light | GitHub | added (upstream high contrast) |
| `github-dark-colorblind-beta` | `github-dark-colorblind-beta` | GitHub Dark Colorblind (Beta) | Dark | GitHub | added |
| `github-light-colorblind-beta` | `github-light-colorblind-beta` | GitHub Light Colorblind (Beta) | Light | GitHub | added |
| `one-dark-pro` | `one-dark-pro` | One Dark Pro | Dark | One Dark Pro / Atom One | added |
| `one-dark-pro-darker` | `one-dark-pro-darker` | One Dark Pro Darker | Dark | One Dark Pro / Atom One | added |
| `one-dark-pro-night-flat` | `one-dark-pro-night-flat` | One Dark Pro Night Flat | Dark | One Dark Pro / Atom One | added |
| `monokai` | `monokai` | Monokai | Dark | Monokai Classic | added |
| `solarized-dark` | `solarized-dark` | Solarized Dark | Dark | Solarized | added |
| `solarized-light` | `solarized-light` | Solarized Light | Light | Solarized | added |
| `night-owl` | `night-owl` | Night Owl | Dark | Night Owl | added |
| `night-owl-light` | `night-owl-light` | Night Owl Light | Light | Night Owl | added |
| `ayu-dark` | `ayu-dark` | Ayu Dark | Dark | Ayu | added |
| `ayu-mirage` | `ayu-mirage` | Ayu Mirage | Dark | Ayu | added |
| `ayu-light` | `ayu-light` | Ayu Light | Light | Ayu | added |
| `ayu-dark-unbordered` | `ayu-dark-unbordered` | Ayu Dark Unbordered | Dark | Ayu | added |
| `ayu-mirage-unbordered` | `ayu-mirage-unbordered` | Ayu Mirage Unbordered | Dark | Ayu | added |
| `ayu-light-unbordered` | `ayu-light-unbordered` | Ayu Light Unbordered | Light | Ayu | added |
| `cobalt2` | `cobalt2` | Cobalt2 | Dark | Cobalt2 | added |
| `palenight-theme` | `palenight-theme` | Palenight Theme | Dark | Palenight | added |
| `palenight-mild-contrast` | `palenight-mild-contrast` | Palenight (Mild Contrast) | Dark | Palenight | added |
| `tokyo-night-storm` | `tokyo-night-storm` | Tokyo Night Storm | Dark | Tokyo Night | added |
| `tokyo-night-light` | `tokyo-night-light` | Tokyo Night Light | Light | Tokyo Night | added |
| `dracula-soft` | `dracula-theme-soft` | Dracula Theme Soft | Dark | Dracula | added |
| `gruvbox-dark-hard` | `gruvbox-dark-hard` | Gruvbox Dark Hard | Dark | Gruvbox | added |
| `gruvbox-dark-soft` | `gruvbox-dark-soft` | Gruvbox Dark Soft | Dark | Gruvbox | added |
| `gruvbox-light-hard` | `gruvbox-light-hard` | Gruvbox Light Hard | Light | Gruvbox | added |
| `gruvbox-light-soft` | `gruvbox-light-soft` | Gruvbox Light Soft | Light | Gruvbox | added |

Research-only Material/Vira, Monokai Pro, and Shades of Purple are excluded. No proprietary theme packages are imported.
