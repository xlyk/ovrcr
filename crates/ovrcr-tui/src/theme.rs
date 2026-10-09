//! Shared Dashboard and startup palette.
//!
//! Tokens are selected by [`ovrcr_protocol::ThemeId`] and cached on the
//! Dashboard. Draw entry points install the active tokens for the duration of
//! a frame so render helpers can read them without every signature carrying a
//! `&ThemeTokens` (see [`ThemeScope`]).
pub use ratatui::style::Color;

use ovrcr_protocol::{ThemeAppearance, ThemeId};
use std::cell::Cell;

mod catalog;

/// The fifteen Design-locked palette tokens shared by every built-in theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
    appearance: ThemeAppearance,
    pub base: Color,
    pub crust: Color,
    pub text: Color,
    pub subtext: Color,
    pub muted: Color,
    pub mauve: Color,
    pub peach: Color,
    pub green: Color,
    pub teal: Color,
    pub blue: Color,
    pub sky: Color,
    pub yellow: Color,
    pub red: Color,
    pub surface0: Color,
    pub surface2: Color,
}

impl ThemeTokens {
    /// Catppuccin Mocha — product default / README brand (`ThemeId::Dark`).
    pub const fn dark() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            base: Color::Rgb(30, 30, 46),
            crust: Color::Rgb(17, 17, 27),
            text: Color::Rgb(205, 214, 244),
            subtext: Color::Rgb(166, 173, 200),
            muted: Color::Rgb(108, 112, 134),
            mauve: Color::Rgb(203, 166, 247),
            peach: Color::Rgb(250, 179, 135),
            green: Color::Rgb(166, 227, 161),
            teal: Color::Rgb(148, 226, 213),
            blue: Color::Rgb(137, 180, 250),
            sky: Color::Rgb(137, 220, 235),
            yellow: Color::Rgb(249, 226, 175),
            red: Color::Rgb(243, 139, 168),
            surface0: Color::Rgb(49, 50, 68),
            surface2: Color::Rgb(88, 91, 112),
        }
    }

    /// Catppuccin Latte (`ThemeId::Light`).
    pub const fn light() -> Self {
        Self {
            appearance: ThemeAppearance::Light,
            base: Color::Rgb(239, 241, 245),
            crust: Color::Rgb(220, 224, 232),
            text: Color::Rgb(76, 79, 105),
            subtext: Color::Rgb(108, 111, 133),
            muted: Color::Rgb(156, 160, 176),
            mauve: Color::Rgb(136, 57, 239),
            peach: Color::Rgb(254, 100, 11),
            green: Color::Rgb(64, 160, 43),
            teal: Color::Rgb(23, 146, 153),
            blue: Color::Rgb(30, 102, 245),
            sky: Color::Rgb(4, 165, 229),
            yellow: Color::Rgb(223, 142, 29),
            red: Color::Rgb(210, 15, 57),
            surface0: Color::Rgb(204, 208, 218),
            surface2: Color::Rgb(172, 176, 190),
        }
    }

    pub const fn tokyo_night() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            base: Color::Rgb(26, 27, 38),
            crust: Color::Rgb(22, 22, 30),
            text: Color::Rgb(192, 202, 245),
            subtext: Color::Rgb(169, 177, 214),
            muted: Color::Rgb(86, 95, 137),
            mauve: Color::Rgb(187, 154, 247),
            peach: Color::Rgb(255, 158, 100),
            green: Color::Rgb(158, 206, 106),
            teal: Color::Rgb(26, 188, 156),
            blue: Color::Rgb(122, 162, 247),
            sky: Color::Rgb(125, 207, 255),
            yellow: Color::Rgb(224, 175, 104),
            red: Color::Rgb(247, 118, 142),
            surface0: Color::Rgb(41, 46, 66),
            surface2: Color::Rgb(59, 66, 97),
        }
    }

    pub const fn dracula() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            base: Color::Rgb(40, 42, 54),
            crust: Color::Rgb(33, 34, 44),
            text: Color::Rgb(248, 248, 242),
            subtext: Color::Rgb(226, 228, 232),
            muted: Color::Rgb(98, 114, 164),
            mauve: Color::Rgb(189, 147, 249),
            peach: Color::Rgb(255, 184, 108),
            green: Color::Rgb(80, 250, 123),
            teal: Color::Rgb(139, 233, 253),
            blue: Color::Rgb(139, 233, 253),
            sky: Color::Rgb(255, 121, 198),
            yellow: Color::Rgb(241, 250, 140),
            red: Color::Rgb(255, 85, 85),
            surface0: Color::Rgb(68, 71, 90),
            surface2: Color::Rgb(98, 114, 164),
        }
    }

    pub const fn gruvbox_dark() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            base: Color::Rgb(40, 40, 40),
            crust: Color::Rgb(29, 32, 33),
            text: Color::Rgb(235, 219, 178),
            subtext: Color::Rgb(213, 196, 161),
            muted: Color::Rgb(146, 131, 116),
            mauve: Color::Rgb(211, 134, 155),
            peach: Color::Rgb(254, 128, 25),
            green: Color::Rgb(184, 187, 38),
            teal: Color::Rgb(142, 192, 124),
            blue: Color::Rgb(131, 165, 152),
            sky: Color::Rgb(131, 165, 152),
            yellow: Color::Rgb(250, 189, 47),
            red: Color::Rgb(251, 73, 52),
            surface0: Color::Rgb(60, 56, 54),
            surface2: Color::Rgb(102, 92, 84),
        }
    }

    pub const fn gruvbox_light() -> Self {
        Self {
            appearance: ThemeAppearance::Light,
            base: Color::Rgb(251, 241, 199),
            crust: Color::Rgb(242, 229, 188),
            text: Color::Rgb(60, 56, 54),
            subtext: Color::Rgb(80, 73, 69),
            muted: Color::Rgb(146, 131, 116),
            mauve: Color::Rgb(143, 63, 113),
            peach: Color::Rgb(175, 58, 3),
            green: Color::Rgb(121, 116, 14),
            teal: Color::Rgb(66, 123, 88),
            blue: Color::Rgb(7, 102, 120),
            sky: Color::Rgb(66, 123, 88),
            yellow: Color::Rgb(181, 118, 20),
            red: Color::Rgb(157, 0, 6),
            surface0: Color::Rgb(235, 219, 178),
            surface2: Color::Rgb(189, 174, 147),
        }
    }

    pub const fn nord() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            base: Color::Rgb(46, 52, 64),
            crust: Color::Rgb(36, 41, 51),
            text: Color::Rgb(236, 239, 244),
            subtext: Color::Rgb(216, 222, 233),
            muted: Color::Rgb(76, 86, 106),
            mauve: Color::Rgb(180, 142, 173),
            peach: Color::Rgb(208, 135, 112),
            green: Color::Rgb(163, 190, 140),
            teal: Color::Rgb(143, 188, 187),
            blue: Color::Rgb(94, 129, 172),
            sky: Color::Rgb(136, 192, 208),
            yellow: Color::Rgb(235, 203, 139),
            red: Color::Rgb(191, 97, 106),
            surface0: Color::Rgb(59, 66, 82),
            surface2: Color::Rgb(67, 76, 94),
        }
    }

    pub const fn rose_pine() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            base: Color::Rgb(25, 23, 36),
            crust: Color::Rgb(25, 23, 36),
            text: Color::Rgb(224, 222, 244),
            subtext: Color::Rgb(144, 140, 170),
            muted: Color::Rgb(110, 106, 134),
            mauve: Color::Rgb(196, 167, 231),
            peach: Color::Rgb(235, 188, 186),
            green: Color::Rgb(49, 116, 143),
            teal: Color::Rgb(156, 207, 216),
            blue: Color::Rgb(49, 116, 143),
            sky: Color::Rgb(156, 207, 216),
            yellow: Color::Rgb(246, 193, 119),
            red: Color::Rgb(235, 111, 146),
            surface0: Color::Rgb(31, 29, 46),
            surface2: Color::Rgb(38, 35, 58),
        }
    }

    /// Source-declared canvas appearance, independent of RGB heuristics.
    pub fn is_light(self) -> bool {
        self.appearance == ThemeAppearance::Light
    }

    pub const fn for_id(id: ThemeId) -> Self {
        match id {
            ThemeId::Dark => Self::dark(),
            ThemeId::Light => Self::light(),
            ThemeId::TokyoNight => Self::tokyo_night(),
            ThemeId::Dracula => Self::dracula(),
            ThemeId::GruvboxDark => Self::gruvbox_dark(),
            ThemeId::GruvboxLight => Self::gruvbox_light(),
            ThemeId::Nord => Self::nord(),
            ThemeId::RosePine => Self::rose_pine(),
            ThemeId::CatppuccinMacchiato => catalog::CATPPUCCIN_MACCHIATO,
            ThemeId::CatppuccinFrappe => catalog::CATPPUCCIN_FRAPPE,
            ThemeId::RosePineMoon => catalog::ROSE_PINE_MOON,
            ThemeId::RosePineDawn => catalog::ROSE_PINE_DAWN,
            ThemeId::KanagawaWave => catalog::KANAGAWA_WAVE,
            ThemeId::KanagawaDragon => catalog::KANAGAWA_DRAGON,
            ThemeId::KanagawaLotus => catalog::KANAGAWA_LOTUS,
            ThemeId::EverforestDarkHard => catalog::EVERFOREST_DARK_HARD,
            ThemeId::EverforestDarkMedium => catalog::EVERFOREST_DARK_MEDIUM,
            ThemeId::EverforestDarkSoft => catalog::EVERFOREST_DARK_SOFT,
            ThemeId::EverforestLightHard => catalog::EVERFOREST_LIGHT_HARD,
            ThemeId::EverforestLightMedium => catalog::EVERFOREST_LIGHT_MEDIUM,
            ThemeId::EverforestLightSoft => catalog::EVERFOREST_LIGHT_SOFT,
            ThemeId::Nightfox => catalog::NIGHTFOX,
            ThemeId::Dayfox => catalog::DAYFOX,
            ThemeId::Dawnfox => catalog::DAWNFOX,
            ThemeId::Duskfox => catalog::DUSKFOX,
            ThemeId::Nordfox => catalog::NORDFOX,
            ThemeId::Terafox => catalog::TERAFOX,
            ThemeId::Carbonfox => catalog::CARBONFOX,
            ThemeId::ZenbonesDark => catalog::ZENBONES_DARK,
            ThemeId::ZenbonesLight => catalog::ZENBONES_LIGHT,
            ThemeId::ZenwrittenDark => catalog::ZENWRITTEN_DARK,
            ThemeId::ZenwrittenLight => catalog::ZENWRITTEN_LIGHT,
            ThemeId::FlexokiDark => catalog::FLEXOKI_DARK,
            ThemeId::FlexokiLight => catalog::FLEXOKI_LIGHT,
            ThemeId::Synthwave84 => catalog::SYNTHWAVE_84,
            ThemeId::Snazzy => catalog::SNAZZY,
            ThemeId::OxocarbonDark => catalog::OXOCARBON_DARK,
            ThemeId::OxocarbonLight => catalog::OXOCARBON_LIGHT,
            ThemeId::Poimandres => catalog::POIMANDRES,
            ThemeId::PoimandresStorm => catalog::POIMANDRES_STORM,
            ThemeId::Horizon => catalog::HORIZON,
            ThemeId::HorizonBright => catalog::HORIZON_BRIGHT,
            ThemeId::Andromeda => catalog::ANDROMEDA,
            ThemeId::AndromedaBordered => catalog::ANDROMEDA_BORDERED,
            ThemeId::Vesper => catalog::VESPER,
            ThemeId::VsCodeDark => catalog::VS_CODE_DARK,
            ThemeId::VsCodeLight => catalog::VS_CODE_LIGHT,
            ThemeId::VsCodeDarkHighContrast => catalog::VS_CODE_DARK_HIGH_CONTRAST,
            ThemeId::VsCodeLightHighContrast => catalog::VS_CODE_LIGHT_HIGH_CONTRAST,
            ThemeId::GithubDarkDefault => catalog::GITHUB_DARK_DEFAULT,
            ThemeId::GithubLightDefault => catalog::GITHUB_LIGHT_DEFAULT,
            ThemeId::GithubDarkDimmed => catalog::GITHUB_DARK_DIMMED,
            ThemeId::GithubDarkHighContrast => catalog::GITHUB_DARK_HIGH_CONTRAST,
            ThemeId::GithubLightHighContrast => catalog::GITHUB_LIGHT_HIGH_CONTRAST,
            ThemeId::GithubDarkColorblindBeta => catalog::GITHUB_DARK_COLORBLIND_BETA,
            ThemeId::GithubLightColorblindBeta => catalog::GITHUB_LIGHT_COLORBLIND_BETA,
            ThemeId::OneDarkPro => catalog::ONE_DARK_PRO,
            ThemeId::OneDarkProDarker => catalog::ONE_DARK_PRO_DARKER,
            ThemeId::OneDarkProNightFlat => catalog::ONE_DARK_PRO_NIGHT_FLAT,
            ThemeId::Monokai => catalog::MONOKAI,
            ThemeId::SolarizedDark => catalog::SOLARIZED_DARK,
            ThemeId::SolarizedLight => catalog::SOLARIZED_LIGHT,
            ThemeId::NightOwl => catalog::NIGHT_OWL,
            ThemeId::NightOwlLight => catalog::NIGHT_OWL_LIGHT,
            ThemeId::AyuDark => catalog::AYU_DARK,
            ThemeId::AyuMirage => catalog::AYU_MIRAGE,
            ThemeId::AyuLight => catalog::AYU_LIGHT,
            ThemeId::AyuDarkUnbordered => catalog::AYU_DARK_UNBORDERED,
            ThemeId::AyuMirageUnbordered => catalog::AYU_MIRAGE_UNBORDERED,
            ThemeId::AyuLightUnbordered => catalog::AYU_LIGHT_UNBORDERED,
            ThemeId::Cobalt2 => catalog::COBALT2,
            ThemeId::PalenightTheme => catalog::PALENIGHT_THEME,
            ThemeId::PalenightMildContrast => catalog::PALENIGHT_MILD_CONTRAST,
            ThemeId::TokyoNightStorm => catalog::TOKYO_NIGHT_STORM,
            ThemeId::TokyoNightLight => catalog::TOKYO_NIGHT_LIGHT,
            ThemeId::DraculaSoft => catalog::DRACULA_SOFT,
            ThemeId::GruvboxDarkHard => catalog::GRUVBOX_DARK_HARD,
            ThemeId::GruvboxDarkSoft => catalog::GRUVBOX_DARK_SOFT,
            ThemeId::GruvboxLightHard => catalog::GRUVBOX_LIGHT_HARD,
            ThemeId::GruvboxLightSoft => catalog::GRUVBOX_LIGHT_SOFT,
        }
    }
}

thread_local! {
    static ACTIVE: Cell<ThemeTokens> = const { Cell::new(ThemeTokens::dark()) };
}

/// Installs `tokens` as the active palette until dropped (restores the prior set).
pub struct ThemeScope {
    previous: ThemeTokens,
}

impl ThemeScope {
    pub fn enter(tokens: ThemeTokens) -> Self {
        let previous = ACTIVE.get();
        ACTIVE.set(tokens);
        Self { previous }
    }
}

impl Drop for ThemeScope {
    fn drop(&mut self) {
        ACTIVE.set(self.previous);
    }
}

/// Tokens installed by the innermost [`ThemeScope`], or Dark when none is active.
#[inline]
pub fn active() -> ThemeTokens {
    ACTIVE.get()
}

// Design token accessors keep UPPERCASE names matching the locked palette.
#[allow(non_snake_case)]
#[inline]
pub fn BASE() -> Color {
    active().base
}
#[allow(non_snake_case)]
#[inline]
pub fn CRUST() -> Color {
    active().crust
}
#[allow(non_snake_case)]
#[inline]
pub fn TEXT() -> Color {
    active().text
}
#[allow(non_snake_case)]
#[inline]
pub fn SUBTEXT() -> Color {
    active().subtext
}
#[allow(non_snake_case)]
#[inline]
pub fn MUTED() -> Color {
    active().muted
}
#[allow(non_snake_case)]
#[inline]
pub fn MAUVE() -> Color {
    active().mauve
}
#[allow(non_snake_case)]
#[inline]
pub fn PEACH() -> Color {
    active().peach
}
#[allow(non_snake_case)]
#[inline]
pub fn GREEN() -> Color {
    active().green
}
#[allow(non_snake_case)]
#[inline]
pub fn TEAL() -> Color {
    active().teal
}
#[allow(non_snake_case)]
#[inline]
pub fn BLUE() -> Color {
    active().blue
}
#[allow(non_snake_case)]
#[inline]
pub fn SKY() -> Color {
    active().sky
}
#[allow(non_snake_case)]
#[inline]
pub fn YELLOW() -> Color {
    active().yellow
}
#[allow(non_snake_case)]
#[inline]
pub fn RED() -> Color {
    active().red
}
#[allow(non_snake_case)]
#[inline]
pub fn SURFACE0() -> Color {
    active().surface0
}
#[allow(non_snake_case)]
#[inline]
pub fn SURFACE2() -> Color {
    active().surface2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_tokens(t: ThemeTokens, rgb: [(u8, u8, u8); 15]) {
        let colors = [
            t.base, t.crust, t.text, t.subtext, t.muted, t.mauve, t.peach, t.green, t.teal, t.blue,
            t.sky, t.yellow, t.red, t.surface0, t.surface2,
        ];
        for (got, (r, g, b)) in colors.into_iter().zip(rgb) {
            assert_eq!(got, Color::Rgb(r, g, b));
        }
    }

    #[test]
    fn dark_matches_design_lock_mocha() {
        assert_tokens(
            ThemeTokens::dark(),
            [
                (30, 30, 46),
                (17, 17, 27),
                (205, 214, 244),
                (166, 173, 200),
                (108, 112, 134),
                (203, 166, 247),
                (250, 179, 135),
                (166, 227, 161),
                (148, 226, 213),
                (137, 180, 250),
                (137, 220, 235),
                (249, 226, 175),
                (243, 139, 168),
                (49, 50, 68),
                (88, 91, 112),
            ],
        );
    }

    #[test]
    fn light_matches_design_lock_latte() {
        assert_tokens(
            ThemeTokens::light(),
            [
                (239, 241, 245),
                (220, 224, 232),
                (76, 79, 105),
                (108, 111, 133),
                (156, 160, 176),
                (136, 57, 239),
                (254, 100, 11),
                (64, 160, 43),
                (23, 146, 153),
                (30, 102, 245),
                (4, 165, 229),
                (223, 142, 29),
                (210, 15, 57),
                (204, 208, 218),
                (172, 176, 190),
            ],
        );
    }

    #[test]
    fn is_light_flags_light_canvases() {
        assert!(ThemeTokens::light().is_light());
        assert!(ThemeTokens::gruvbox_light().is_light());
        assert!(!ThemeTokens::dark().is_light());
        assert!(!ThemeTokens::tokyo_night().is_light());
        assert!(!ThemeTokens::dracula().is_light());
        assert!(!ThemeTokens::nord().is_light());
        assert!(!ThemeTokens::rose_pine().is_light());
        assert!(!ThemeTokens::gruvbox_dark().is_light());
    }

    #[test]
    fn for_id_resolves_every_theme() {
        let cases = [
            (ThemeId::Dark, ThemeTokens::dark()),
            (ThemeId::Light, ThemeTokens::light()),
            (ThemeId::TokyoNight, ThemeTokens::tokyo_night()),
            (ThemeId::Dracula, ThemeTokens::dracula()),
            (ThemeId::GruvboxDark, ThemeTokens::gruvbox_dark()),
            (ThemeId::GruvboxLight, ThemeTokens::gruvbox_light()),
            (ThemeId::Nord, ThemeTokens::nord()),
            (ThemeId::RosePine, ThemeTokens::rose_pine()),
        ];
        for (id, tokens) in cases {
            assert_eq!(ThemeTokens::for_id(id), tokens);
        }
    }

    #[test]
    fn design_locked_popular_themes() {
        assert_tokens(
            ThemeTokens::tokyo_night(),
            [
                (26, 27, 38),
                (22, 22, 30),
                (192, 202, 245),
                (169, 177, 214),
                (86, 95, 137),
                (187, 154, 247),
                (255, 158, 100),
                (158, 206, 106),
                (26, 188, 156),
                (122, 162, 247),
                (125, 207, 255),
                (224, 175, 104),
                (247, 118, 142),
                (41, 46, 66),
                (59, 66, 97),
            ],
        );
        assert_tokens(
            ThemeTokens::dracula(),
            [
                (40, 42, 54),
                (33, 34, 44),
                (248, 248, 242),
                (226, 228, 232),
                (98, 114, 164),
                (189, 147, 249),
                (255, 184, 108),
                (80, 250, 123),
                (139, 233, 253),
                (139, 233, 253),
                (255, 121, 198),
                (241, 250, 140),
                (255, 85, 85),
                (68, 71, 90),
                (98, 114, 164),
            ],
        );
        assert_tokens(
            ThemeTokens::gruvbox_dark(),
            [
                (40, 40, 40),
                (29, 32, 33),
                (235, 219, 178),
                (213, 196, 161),
                (146, 131, 116),
                (211, 134, 155),
                (254, 128, 25),
                (184, 187, 38),
                (142, 192, 124),
                (131, 165, 152),
                (131, 165, 152),
                (250, 189, 47),
                (251, 73, 52),
                (60, 56, 54),
                (102, 92, 84),
            ],
        );
        assert_tokens(
            ThemeTokens::gruvbox_light(),
            [
                (251, 241, 199),
                (242, 229, 188),
                (60, 56, 54),
                (80, 73, 69),
                (146, 131, 116),
                (143, 63, 113),
                (175, 58, 3),
                (121, 116, 14),
                (66, 123, 88),
                (7, 102, 120),
                (66, 123, 88),
                (181, 118, 20),
                (157, 0, 6),
                (235, 219, 178),
                (189, 174, 147),
            ],
        );
        assert_tokens(
            ThemeTokens::nord(),
            [
                (46, 52, 64),
                (36, 41, 51),
                (236, 239, 244),
                (216, 222, 233),
                (76, 86, 106),
                (180, 142, 173),
                (208, 135, 112),
                (163, 190, 140),
                (143, 188, 187),
                (94, 129, 172),
                (136, 192, 208),
                (235, 203, 139),
                (191, 97, 106),
                (59, 66, 82),
                (67, 76, 94),
            ],
        );
        assert_tokens(
            ThemeTokens::rose_pine(),
            [
                (25, 23, 36),
                (25, 23, 36),
                (224, 222, 244),
                (144, 140, 170),
                (110, 106, 134),
                (196, 167, 231),
                (235, 188, 186),
                (49, 116, 143),
                (156, 207, 216),
                (49, 116, 143),
                (156, 207, 216),
                (246, 193, 119),
                (235, 111, 146),
                (31, 29, 46),
                (38, 35, 58),
            ],
        );
    }

    #[test]
    fn theme_scope_installs_and_restores() {
        assert_eq!(active(), ThemeTokens::dark());
        {
            let _scope = ThemeScope::enter(ThemeTokens::light());
            assert_eq!(active(), ThemeTokens::light());
            assert_eq!(BASE(), ThemeTokens::light().base);
            assert_eq!(TEXT(), ThemeTokens::light().text);
        }
        assert_eq!(active(), ThemeTokens::dark());
    }

    fn luminance(color: Color) -> f64 {
        let Color::Rgb(r, g, b) = color else {
            panic!("catalog tokens must be RGB")
        };
        let linear = |value: u8| {
            let channel = f64::from(value) / 255.0;
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn all_approved_themes_have_explicit_appearance_and_audited_tokens() {
        let audit: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/themes/semantic-mappings.json"))
                .unwrap();
        for id in ThemeId::ALL.iter().copied() {
            let tokens = ThemeTokens::for_id(id);
            assert_eq!(
                tokens.is_light(),
                id.appearance() == ThemeAppearance::Light,
                "{id}"
            );
            let row = audit["themes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["native_id"] == id.as_str())
                .unwrap();
            for (name, color) in [
                ("base", tokens.base),
                ("crust", tokens.crust),
                ("text", tokens.text),
                ("subtext", tokens.subtext),
                ("muted", tokens.muted),
                ("mauve", tokens.mauve),
                ("peach", tokens.peach),
                ("green", tokens.green),
                ("teal", tokens.teal),
                ("blue", tokens.blue),
                ("sky", tokens.sky),
                ("yellow", tokens.yellow),
                ("red", tokens.red),
                ("surface0", tokens.surface0),
                ("surface2", tokens.surface2),
            ] {
                let Color::Rgb(r, g, b) = color else {
                    panic!("{id}: {name}")
                };
                assert_eq!(
                    row["tokens"][name]["value"],
                    format!("#{r:02X}{g:02X}{b:02X}"),
                    "{id}: {name}"
                );
            }
        }
    }

    #[test]
    fn added_theme_text_focus_status_and_selection_pairs_are_legible() {
        // Original palettes remain protected by their RGB snapshot tests.
        for id in ThemeId::ALL[8..].iter().copied() {
            let t = ThemeTokens::for_id(id);
            for (role, color) in [
                ("primary", t.text),
                ("secondary", t.subtext),
                ("muted labels", t.muted),
                ("focus and accent", t.mauve),
                ("attention", t.peach),
                ("success", t.green),
                ("teal labels", t.teal),
                ("blue labels", t.blue),
                ("sky labels", t.sky),
                ("warning", t.yellow),
                ("error", t.red),
            ] {
                for (surface, bg) in [
                    ("body", t.base),
                    ("sidebar", t.crust),
                    ("selection", t.surface0),
                ] {
                    let ratio = contrast(color, bg);
                    assert!(ratio >= 4.5, "{id}: {role} on {surface}: {ratio:.3}:1");
                }
            }
            for bg in [t.base, t.crust, t.surface0] {
                assert!(contrast(t.surface2, bg) >= 3.0, "{id}: border");
            }
            assert!(
                contrast(t.surface0, t.base) >= 1.18,
                "{id}: visible selection"
            );
            assert!(
                contrast(t.crust, t.mauve) >= 4.5,
                "{id}: inverted focus label"
            );
            assert!(
                contrast(t.base, t.yellow) >= 4.5,
                "{id}: inverted warning label"
            );
        }
    }

    #[test]
    fn every_catalog_theme_hot_switches_through_the_dashboard_draw_path() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut dashboard =
            crate::Dashboard::new(ovrcr_protocol::TerminalSize { rows: 12, cols: 40 });
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        for id in ThemeId::ALL.iter().copied() {
            dashboard.install_settings(ovrcr_protocol::Settings {
                theme: id,
                ..Default::default()
            });
            terminal
                .draw(|frame| crate::draw_dashboard_at(frame, &dashboard, 0))
                .unwrap();
            assert_eq!(
                terminal.backend().buffer()[(0, 1)].bg,
                ThemeTokens::for_id(id).base,
                "{id}"
            );
            assert_eq!(active(), ThemeTokens::dark(), "draw must restore scope");
        }
    }

    #[test]
    fn catalog_switches_preserve_agent_indexed_and_truecolor_cells() {
        use ratatui::{Terminal, backend::TestBackend, layout::Rect};
        let mut parser = ovrcr_terminal::vt100::Parser::new(1, 4, 0);
        parser.process(b"\x1b[31mR\x1b[38;2;11;22;33mX");
        for id in ThemeId::ALL.iter().copied() {
            let mut terminal = Terminal::new(TestBackend::new(4, 1)).unwrap();
            let _scope = ThemeScope::enter(ThemeTokens::for_id(id));
            terminal
                .draw(|frame| {
                    crate::render_terminal(frame, Rect::new(0, 0, 4, 1), parser.screen(), true)
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(0, 0)].symbol(), "R");
            assert_eq!(buffer[(0, 0)].fg, Color::Indexed(1), "{id}");
            assert_eq!(buffer[(1, 0)].fg, Color::Rgb(11, 22, 33), "{id}");
        }
    }
}
