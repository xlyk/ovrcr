//! Shared Dashboard and startup palette.
//!
//! Tokens are selected by [`ovrcr_protocol::ThemeId`] and cached on the
//! Dashboard. Draw entry points install the active tokens for the duration of
//! a frame so render helpers can read them without every signature carrying a
//! `&ThemeTokens` (see [`ThemeScope`]).
pub use ratatui::style::Color;

use ovrcr_protocol::ThemeId;
use std::cell::Cell;

/// The fifteen Design-locked palette tokens shared by Dark (Mocha) and Light (Latte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
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
    /// Catppuccin Mocha — product default / README brand.
    pub const fn dark() -> Self {
        Self {
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

    /// Catppuccin Latte — Design-locked light theme.
    pub const fn light() -> Self {
        Self {
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

    pub const fn for_id(id: ThemeId) -> Self {
        match id {
            ThemeId::Dark => Self::dark(),
            ThemeId::Light => Self::light(),
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

    #[test]
    fn dark_matches_design_lock_mocha() {
        let t = ThemeTokens::dark();
        assert_eq!(t.base, Color::Rgb(30, 30, 46));
        assert_eq!(t.crust, Color::Rgb(17, 17, 27));
        assert_eq!(t.text, Color::Rgb(205, 214, 244));
        assert_eq!(t.subtext, Color::Rgb(166, 173, 200));
        assert_eq!(t.muted, Color::Rgb(108, 112, 134));
        assert_eq!(t.mauve, Color::Rgb(203, 166, 247));
        assert_eq!(t.peach, Color::Rgb(250, 179, 135));
        assert_eq!(t.green, Color::Rgb(166, 227, 161));
        assert_eq!(t.teal, Color::Rgb(148, 226, 213));
        assert_eq!(t.blue, Color::Rgb(137, 180, 250));
        assert_eq!(t.sky, Color::Rgb(137, 220, 235));
        assert_eq!(t.yellow, Color::Rgb(249, 226, 175));
        assert_eq!(t.red, Color::Rgb(243, 139, 168));
        assert_eq!(t.surface0, Color::Rgb(49, 50, 68));
        assert_eq!(t.surface2, Color::Rgb(88, 91, 112));
    }

    #[test]
    fn light_matches_design_lock_latte() {
        let t = ThemeTokens::light();
        assert_eq!(t.base, Color::Rgb(239, 241, 245));
        assert_eq!(t.crust, Color::Rgb(220, 224, 232));
        assert_eq!(t.text, Color::Rgb(76, 79, 105));
        assert_eq!(t.subtext, Color::Rgb(108, 111, 133));
        assert_eq!(t.muted, Color::Rgb(156, 160, 176));
        assert_eq!(t.mauve, Color::Rgb(136, 57, 239));
        assert_eq!(t.peach, Color::Rgb(254, 100, 11));
        assert_eq!(t.green, Color::Rgb(64, 160, 43));
        assert_eq!(t.teal, Color::Rgb(23, 146, 153));
        assert_eq!(t.blue, Color::Rgb(30, 102, 245));
        assert_eq!(t.sky, Color::Rgb(4, 165, 229));
        assert_eq!(t.yellow, Color::Rgb(223, 142, 29));
        assert_eq!(t.red, Color::Rgb(210, 15, 57));
        assert_eq!(t.surface0, Color::Rgb(204, 208, 218));
        assert_eq!(t.surface2, Color::Rgb(172, 176, 190));
    }

    #[test]
    fn for_id_resolves_both_themes() {
        assert_eq!(ThemeTokens::for_id(ThemeId::Dark), ThemeTokens::dark());
        assert_eq!(ThemeTokens::for_id(ThemeId::Light), ThemeTokens::light());
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
}
