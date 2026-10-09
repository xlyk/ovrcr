#!/usr/bin/env python3
"""Generate the approved catalog and audited OVRCR mappings from local inputs.

Source JSON and notices are immutable transfer inputs. Existing native palettes
are read from their protected constructors, never replaced by gallery values.
"""
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs/themes/source"
MANIFEST = json.loads((SOURCE / "OVRCR-approved-palette-manifest.json").read_text())
PALETTES = MANIFEST["palettes"]
TOKENS = MANIFEST["token_order"]
PROTECTED = {
    "catppuccin-mocha": ("Dark", "dark"),
    "catppuccin-latte": ("Light", "light"),
    "tokyo-night": ("TokyoNight", "tokyo_night"),
    "dracula-theme": ("Dracula", "dracula"),
    "gruvbox-dark": ("GruvboxDark", "gruvbox_dark"),
    "gruvbox-light": ("GruvboxLight", "gruvbox_light"),
    "nord": ("Nord", "nord"),
    "ros-pine": ("RosePine", "rose_pine"),
}
NATIVE = {"ros-pine": "rose-pine", "dracula-theme": "dracula",
          "ros-pine-moon": "rose-pine-moon", "ros-pine-dawn": "rose-pine-dawn",
          "dracula-theme-soft": "dracula-soft"}
OLD_ORDER = list(PROTECTED)
ORDER = OLD_ORDER + [p["gallery_id"] for p in PALETTES if not p["already_in_ovrcr"]]
BY_ID = {p["gallery_id"]: p for p in PALETTES}
assert len(ORDER) == len(BY_ID) == 80
assert sum(p["appearance"] == "dark" for p in PALETTES) == 54
assert sum(p["appearance"] == "light" for p in PALETTES) == 26
assert {p["gallery_id"] for p in PALETTES if p["already_in_ovrcr"]} == set(PROTECTED)
for line in (SOURCE / "SHA256SUMS.txt").read_text().splitlines():
    digest, name = line.split(maxsplit=1)
    assert hashlib.sha256((SOURCE / name.lstrip("*")).read_bytes()).hexdigest() == digest


def native(gallery_id):
    return NATIVE.get(gallery_id, gallery_id)


def variant(gallery_id):
    if gallery_id in PROTECTED:
        return PROTECTED[gallery_id][0]
    return "".join(part.title() for part in native(gallery_id).split("-"))


def rgb(value):
    return tuple(int(value[i:i + 2], 16) for i in (1, 3, 5))


def hex_rgb(value):
    return "#" + "".join(f"{c:02X}" for c in value)


def luminance(value):
    channels = [v / 255 for v in value]
    linear = [v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4 for v in channels]
    return sum(v * w for v, w in zip(linear, (0.2126, 0.7152, 0.0722)))


def contrast(a, b):
    a, b = sorted((luminance(a), luminance(b)))
    return (b + 0.05) / (a + 0.05)


def blend(a, b, percent):
    return tuple(round(x + (y - x) * percent / 100) for x, y in zip(a, b))


def legible(value, backgrounds, appearance, minimum):
    target = (255, 255, 255) if appearance == "dark" else (0, 0, 0)
    for percent in range(101):
        candidate = blend(value, target, percent)
        if min(contrast(candidate, bg) for bg in backgrounds) >= minimum:
            return candidate
    raise ValueError(f"No legible mapping: {value}, {backgrounds}, {appearance}")


theme_path = ROOT / "crates/ovrcr-tui/src/theme.rs"
theme = theme_path.read_text()
AUDIT = []
NEW = []
for gallery_id in ORDER:
    p = BY_ID[gallery_id]
    original = {k: rgb(p["preview_tokens"][k]) for k in TOKENS}
    notes = {}
    if gallery_id in PROTECTED:
        function = PROTECTED[gallery_id][1]
        block = theme.split(f"pub const fn {function}() -> Self {{", 1)[1].split("\n    }", 1)[0]
        mapped = {k: tuple(map(int, values.split(","))) for k, values in
                  re.findall(r"(\w+): Color::Rgb\(([^)]+)\)", block)}
        assert set(mapped) == set(TOKENS)
        disposition = "protected_existing_native_definition"
    else:
        mapped = dict(original)
        appearance = p["appearance"]
        mapped["text"] = legible(mapped["text"], [mapped["base"], mapped["crust"]], appearance, 4.55)
        # OVRCR shares TEXT between normal and selected rows. Upstream editors
        # can invert the selection foreground independently; that does not fit
        # this contract. Retain their selection in source JSON, then use a
        # neutral selected surface when the pair is unsuitable or invisible.
        selected = mapped["surface0"]
        if contrast(mapped["text"], selected) < 4.55 or contrast(selected, mapped["base"]) < 1.18:
            # Leave enough primary-text headroom for a visibly selected row.
            mapped["text"] = legible(mapped["text"], [mapped["base"], mapped["crust"]], appearance, 5.55)
            for percent in range(1, 101):
                candidate = blend(mapped["base"], mapped["text"], percent)
                if contrast(candidate, mapped["base"]) >= 1.20 and contrast(mapped["text"], candidate) >= 4.55:
                    mapped["surface0"] = candidate
                    notes["surface0"] = "Neutral selected-row surface; shared TEXT must remain readable. Source selection foreground/background are preserved separately."
                    break
            else:
                raise ValueError(f"No selected surface for {gallery_id}")
        backgrounds = [mapped[k] for k in ("base", "crust", "surface0")]
        for token in TOKENS:
            if token in ("base", "crust", "surface0"):
                continue
            minimum = 3.05 if token == "surface2" else 4.55
            mapped[token] = legible(mapped[token], backgrounds, appearance, minimum)
            if mapped[token] != original[token]:
                notes[token] = (f"OVRCR contrast adaptation toward {'white' if appearance == 'dark' else 'black'}; "
                                f"minimum {minimum}:1 against body, sidebar, and selected-row surfaces. "
                                "A derived OVRCR role, not a new upstream palette value.")
        disposition = "added_with_reviewed_semantic_mapping"
        NEW.append((gallery_id, mapped))
    pairs = {f"{fg}_on_{bg}": round(contrast(mapped[fg], mapped[bg]), 4)
             for fg in ("text", "subtext", "muted", "mauve", "peach", "green", "teal", "blue", "sky", "yellow", "red", "surface2")
             for bg in ("base", "crust", "surface0")}
    AUDIT.append({"native_id": native(gallery_id), "gallery_id": gallery_id,
                  "appearance": p["appearance"], "disposition": disposition,
                  "source_ref": p["source_ref"], "source_url": p["source_url"],
                  "gallery_mapping_note": p["mapping_note"],
                  "implementation_hazards": p["implementation_hazards"],
                  "tokens": {k: {"value": hex_rgb(mapped[k]), "preview_value": hex_rgb(original[k]),
                                 "decision": notes.get(k, "Preserve native token unchanged." if gallery_id in PROTECTED
                                                       else "Retain approved gallery mapping; source provenance remains in manifest.")}
                             for k in TOKENS}, "contrast_pairs": pairs})

settings_path = ROOT / "crates/ovrcr-protocol/src/settings.rs"
settings = settings_path.read_text()
start = settings.index("#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]\npub enum ThemeId")
end = settings.index("\nimpl fmt::Display for ThemeId", start)
lines = ["#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]", "pub enum ThemeId {"]
aliases = {}
for g in ORDER:
    attrs = [f'rename = "{native(g)}"']
    aliases[g] = ["dark"] if g == "catppuccin-mocha" else ["light"] if g == "catppuccin-latte" else [g] if g not in PROTECTED and native(g) != g else []
    attrs.extend(f'alias = "{alias}"' for alias in aliases[g])
    if g == "catppuccin-mocha": lines.append("    #[default]")
    lines += [f"    #[serde({', '.join(attrs)})]", f"    {variant(g)},"]
lines += ["}", "", "/// Source-declared canvas appearance used to group Dashboard themes.",
          "#[derive(Clone, Copy, Debug, PartialEq, Eq)]", "pub enum ThemeAppearance {", "    Dark,", "    Light,", "}", "", "impl ThemeId {",
          '    pub const KEY: &\'static str = "theme";', "", "    /// Catalog order; the original eight retain their wire indices.",
          "    pub const ALL: &'static [Self] = &[", *[f"        Self::{variant(g)}," for g in ORDER], "    ];",
          "", "    pub const KEYS: &'static [&'static str] = &[", *[f'        "{native(g)}",' for g in ORDER], "    ];",
          "", "    pub const EXPECTED: &'static str = concat!("]
expected = ", ".join(f'"{native(g)}"' for g in ORDER) + ' (or aliases "dark", "light", "ros-pine-moon", "ros-pine-dawn", "dracula-theme-soft")'
lines += ["        " + json.dumps(expected[i:i + 100]) + "," for i in range(0, len(expected), 100)]
lines += ["    );"]
for method, values, return_type in [
    ("as_str", {g: native(g) for g in ORDER}, "&'static str"),
    ("label", {g: "Dracula" if g == "dracula-theme" else BY_ID[g]["gallery_name"] for g in ORDER}, "&'static str"),
    ("gallery_id", {g: g for g in ORDER}, "&'static str"),
    ("gallery_label", {g: BY_ID[g]["gallery_name"] for g in ORDER}, "&'static str"),
    ("family", {g: BY_ID[g]["family_name"] for g in ORDER}, "&'static str"),
    ("source_url", {g: BY_ID[g]["source_url"] for g in ORDER}, "&'static str"),
    ("appearance", {g: "ThemeAppearance::" + BY_ID[g]["appearance"].title() for g in ORDER}, "ThemeAppearance"),
    ("is_high_contrast_variant", {g: str(BY_ID[g]["high_contrast_variant"]).lower() for g in ORDER}, "bool"),
]:
    lines += ["", f"    pub const fn {method}(self) -> {return_type} {{", "        match self {"]
    for g in ORDER:
        value = json.dumps(values[g], ensure_ascii=False) if return_type == "&'static str" else values[g]
        lines.append(f"            Self::{variant(g)} => {value},")
    lines += ["        }", "    }"]
lines += ["", "    pub fn parse(raw: &str) -> Option<Self> {", "        match raw {"]
for g in ORDER:
    matches = " | ".join(json.dumps(s) for s in [native(g)] + aliases[g])
    lines.append(f"            {matches} => Some(Self::{variant(g)}),")
lines += ["            _ => None,", "        }", "    }", "", "    pub fn next(self) -> Self {",
          "        let index = Self::ALL.iter().position(|theme| *theme == self).unwrap_or(0);",
          "        Self::ALL[(index + 1) % Self::ALL.len()]", "    }", "}"]
settings_path.write_text(settings[:start] + "\n".join(lines) + "\n" + settings[end:])

catalog_path = ROOT / "crates/ovrcr-tui/src/theme/catalog.rs"
catalog_path.parent.mkdir(exist_ok=True)
lines = ["//! Generated by scripts/generate_theme_catalog.py; see docs/themes/semantic-mappings.json.",
         "use super::{Color, ThemeTokens};", "use ovrcr_protocol::ThemeAppearance;", ""]
for g, mapped in NEW:
    name = native(g).replace("-", "_").upper()
    lines += [f"pub(super) const {name}: ThemeTokens = ThemeTokens {{",
              f"    appearance: ThemeAppearance::{BY_ID[g]['appearance'].title()},"]
    lines += [f"    {k}: Color::Rgb{mapped[k]}," for k in TOKENS]
    lines += ["};", ""]
catalog_path.write_text("\n".join(lines))
theme = theme.replace("use ovrcr_protocol::ThemeId;", "use ovrcr_protocol::{ThemeAppearance, ThemeId};")
if "mod catalog;" not in theme: theme = theme.replace("use std::cell::Cell;", "use std::cell::Cell;\n\nmod catalog;")
if "appearance: ThemeAppearance," not in theme:
    theme = theme.replace("pub struct ThemeTokens {", "pub struct ThemeTokens {\n    appearance: ThemeAppearance,")
for g, (_, function) in PROTECTED.items():
    pattern = f"pub const fn {function}() -> Self {{\n        Self {{"
    if pattern + "\n            appearance:" not in theme:
        theme = theme.replace(pattern, pattern + f"\n            appearance: ThemeAppearance::{BY_ID[g]['appearance'].title()},")
theme = re.sub(r"    /// Light-ambiance canvases.*?\n    }\n\n    pub const fn for_id", "    /// Source-declared canvas appearance, independent of RGB heuristics.\n    pub fn is_light(self) -> bool {\n        self.appearance == ThemeAppearance::Light\n    }\n\n    pub const fn for_id", theme, flags=re.S)
dispatch = ["    pub const fn for_id(id: ThemeId) -> Self {", "        match id {"]
for g in ORDER:
    value = f"Self::{PROTECTED[g][1]}()" if g in PROTECTED else "catalog::" + native(g).replace("-", "_").upper()
    dispatch.append(f"            ThemeId::{variant(g)} => {value},")
dispatch += ["        }", "    }"]
theme = re.sub(r"    pub const fn for_id\(id: ThemeId\) -> Self \{.*?\n    }", "\n".join(dispatch), theme, count=1, flags=re.S)
theme_path.write_text(theme)

catalog = ROOT / "crates/ovrcr-protocol/src/catalog.rs"
text = catalog.read_text()
text = re.sub(r'(path: ThemeId::KEY,\n        shape: Shape::Pick \{\n            options:) &\[.*?\],', r'\1 ThemeId::KEYS,', text, count=1, flags=re.S)
catalog.write_text(text)
(ROOT / "docs/themes/semantic-mappings.json").write_text(json.dumps({"policy": "Existing eight tokens are unchanged. Added small-text roles meet 4.5:1 and border roles 3:1 across actual OVRCR surfaces. Original sources and selection foregrounds remain in source JSON.", "themes": AUDIT}, ensure_ascii=False, indent=2) + "\n")
doc = ["# Dashboard theme catalog", "", "The catalog has all 80 approved gallery variants: 54 Dark and 26 Light across 27 families. Eight native definitions are preserved; 72 are additions. Appearance is explicit source metadata.", "",
       "The first eight enum variants and wire indices, IDs, labels, historical `dark`/`light` aliases, default, and all fifteen native RGB tokens are preserved. Gallery IDs map explicitly below; they do not replace native IDs. New Rosé Pine and Dracula spellings also accept their gallery IDs as aliases.", "",
       "## Semantic mapping", "", "The exact source palettes, gallery reference tokens, classification evidence, upstream copyright/license notices, and original contrast report are retained in [source](source/TRANSFER-README.md). [semantic-mappings.json](semantic-mappings.json) records every implemented token, source reference, adaptation, and actual contrast pair. Regenerate tables with `python3 scripts/generate_theme_catalog.py` and then `cargo fmt --all`.", "",
       "For new themes, BASE and CRUST retain the approved gallery canvases. TEXT, SUBTEXT, MUTED, and all accent/status slots are adjusted only when needed to reach 4.5:1 on BASE, CRUST, and SURFACE0. SURFACE2 borders reach 3:1. Adjustments blend the gallery role toward white on dark canvases or black on light canvases; they are OVRCR adaptations, never represented as canonical upstream colors. This also checks CRUST/MAUVE focus-label inversion and BASE/YELLOW warning inversion used by the app.", "",
       "Selected rows share TEXT with ordinary rows. If upstream SURFACE0 requires an inverted selection foreground or is indistinguishable from BASE, OVRCR uses a neutral surface with readable TEXT and at least 1.2:1 separation from BASE. This handles high-contrast editor selection mappings deliberately; the source selection pair is kept in canonical-source-palettes.json. A high-contrast variant name identifies the upstream variant and is not an accessibility certification.", "",
       "Vesper supplies no canonical blue or purple; OVRCR BLUE/MAUVE use its sourced warm accent. Oxocarbon supplies no yellow; YELLOW/PEACH reuse the sourced cyan accent. These fallbacks retain the theme's palette and existing visible labels/symbols for warning, error, success, focus, and selection. Hues alone do not encode status. Solarized Light's primary text is darkened for the OVRCR small-text role because the canonical pair is approximately 4.13:1.", "",
       "Protected existing themes retain some below-reference pairs for compatibility; the audit reports those ratios without claiming a blanket accessibility pass. No agent ANSI/RGB palette, terminal renderer, Settings interaction, or notification code is changed by this catalog.", "",
       "## Exact native/approved roster", "", "| Native ID | Approved gallery ID | Label | Appearance | Family | Disposition |", "| --- | --- | --- | --- | --- | --- |"]
for g in ORDER:
    p = BY_ID[g]
    doc.append(f"| `{native(g)}` | `{g}` | {p['gallery_name']} | {p['appearance_group']} | {p['family_name']} | {'existing; preserved' if g in PROTECTED else 'added'}{' (upstream high contrast)' if p['high_contrast_variant'] else ''} |")
doc += ["", "Research-only Material/Vira, Monokai Pro, and Shades of Purple are excluded. No proprietary theme packages are imported.", ""]
(ROOT / "docs/themes/catalog.md").write_text("\n".join(doc))
print(f"Generated {len(ORDER)} IDs, {len(NEW)} added palettes, and per-token audit; preserved 8 native palettes.")
