//! Typed settings and the report the Server's settings loader produces.
//!
//! The loader lives in `ovrcr-runtime`; these types are shared so the CLI and
//! the Dashboard read the same reading. See ADR 0007.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// The four approved native notification sounds; never a caller-selected path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadySoundChoice {
    #[default]
    Default,
    Tap,
    Chime,
    Rise,
}

impl ReadySoundChoice {
    pub const KEY: &'static str = "ready_sound_choice";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Tap => "tap",
            Self::Chime => "chime",
            Self::Rise => "rise",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "System default",
            Self::Tap => "Tap",
            Self::Chime => "Chime",
            Self::Rise => "Rise",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "default" => Some(Self::Default),
            "tap" => Some(Self::Tap),
            "chime" => Some(Self::Chime),
            "rise" => Some(Self::Rise),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Default => Self::Tap,
            Self::Tap => Self::Chime,
            Self::Chime => Self::Rise,
            Self::Rise => Self::Default,
        }
    }
}

/// Dashboard visual theme (Appearance catalog).
///
/// `Dark` / `Light` keep wire indices 0 / 1 and remain aliases of Catppuccin
/// Mocha / Latte (`dark` / `light` still parse). Canonical pick keys are the
/// kebab-case names below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeId {
    #[default]
    #[serde(rename = "catppuccin-mocha", alias = "dark")]
    Dark,
    #[serde(rename = "catppuccin-latte", alias = "light")]
    Light,
    #[serde(rename = "tokyo-night")]
    TokyoNight,
    #[serde(rename = "dracula")]
    Dracula,
    #[serde(rename = "gruvbox-dark")]
    GruvboxDark,
    #[serde(rename = "gruvbox-light")]
    GruvboxLight,
    #[serde(rename = "nord")]
    Nord,
    #[serde(rename = "rose-pine")]
    RosePine,
    #[serde(rename = "catppuccin-macchiato")]
    CatppuccinMacchiato,
    #[serde(rename = "catppuccin-frappe")]
    CatppuccinFrappe,
    #[serde(rename = "rose-pine-moon", alias = "ros-pine-moon")]
    RosePineMoon,
    #[serde(rename = "rose-pine-dawn", alias = "ros-pine-dawn")]
    RosePineDawn,
    #[serde(rename = "kanagawa-wave")]
    KanagawaWave,
    #[serde(rename = "kanagawa-dragon")]
    KanagawaDragon,
    #[serde(rename = "kanagawa-lotus")]
    KanagawaLotus,
    #[serde(rename = "everforest-dark-hard")]
    EverforestDarkHard,
    #[serde(rename = "everforest-dark-medium")]
    EverforestDarkMedium,
    #[serde(rename = "everforest-dark-soft")]
    EverforestDarkSoft,
    #[serde(rename = "everforest-light-hard")]
    EverforestLightHard,
    #[serde(rename = "everforest-light-medium")]
    EverforestLightMedium,
    #[serde(rename = "everforest-light-soft")]
    EverforestLightSoft,
    #[serde(rename = "nightfox")]
    Nightfox,
    #[serde(rename = "dayfox")]
    Dayfox,
    #[serde(rename = "dawnfox")]
    Dawnfox,
    #[serde(rename = "duskfox")]
    Duskfox,
    #[serde(rename = "nordfox")]
    Nordfox,
    #[serde(rename = "terafox")]
    Terafox,
    #[serde(rename = "carbonfox")]
    Carbonfox,
    #[serde(rename = "zenbones-dark")]
    ZenbonesDark,
    #[serde(rename = "zenbones-light")]
    ZenbonesLight,
    #[serde(rename = "zenwritten-dark")]
    ZenwrittenDark,
    #[serde(rename = "zenwritten-light")]
    ZenwrittenLight,
    #[serde(rename = "flexoki-dark")]
    FlexokiDark,
    #[serde(rename = "flexoki-light")]
    FlexokiLight,
    #[serde(rename = "synthwave-84")]
    Synthwave84,
    #[serde(rename = "snazzy")]
    Snazzy,
    #[serde(rename = "oxocarbon-dark")]
    OxocarbonDark,
    #[serde(rename = "oxocarbon-light")]
    OxocarbonLight,
    #[serde(rename = "poimandres")]
    Poimandres,
    #[serde(rename = "poimandres-storm")]
    PoimandresStorm,
    #[serde(rename = "horizon")]
    Horizon,
    #[serde(rename = "horizon-bright")]
    HorizonBright,
    #[serde(rename = "andromeda")]
    Andromeda,
    #[serde(rename = "andromeda-bordered")]
    AndromedaBordered,
    #[serde(rename = "vesper")]
    Vesper,
    #[serde(rename = "vs-code-dark")]
    VsCodeDark,
    #[serde(rename = "vs-code-light")]
    VsCodeLight,
    #[serde(rename = "vs-code-dark-high-contrast")]
    VsCodeDarkHighContrast,
    #[serde(rename = "vs-code-light-high-contrast")]
    VsCodeLightHighContrast,
    #[serde(rename = "github-dark-default")]
    GithubDarkDefault,
    #[serde(rename = "github-light-default")]
    GithubLightDefault,
    #[serde(rename = "github-dark-dimmed")]
    GithubDarkDimmed,
    #[serde(rename = "github-dark-high-contrast")]
    GithubDarkHighContrast,
    #[serde(rename = "github-light-high-contrast")]
    GithubLightHighContrast,
    #[serde(rename = "github-dark-colorblind-beta")]
    GithubDarkColorblindBeta,
    #[serde(rename = "github-light-colorblind-beta")]
    GithubLightColorblindBeta,
    #[serde(rename = "one-dark-pro")]
    OneDarkPro,
    #[serde(rename = "one-dark-pro-darker")]
    OneDarkProDarker,
    #[serde(rename = "one-dark-pro-night-flat")]
    OneDarkProNightFlat,
    #[serde(rename = "monokai")]
    Monokai,
    #[serde(rename = "solarized-dark")]
    SolarizedDark,
    #[serde(rename = "solarized-light")]
    SolarizedLight,
    #[serde(rename = "night-owl")]
    NightOwl,
    #[serde(rename = "night-owl-light")]
    NightOwlLight,
    #[serde(rename = "ayu-dark")]
    AyuDark,
    #[serde(rename = "ayu-mirage")]
    AyuMirage,
    #[serde(rename = "ayu-light")]
    AyuLight,
    #[serde(rename = "ayu-dark-unbordered")]
    AyuDarkUnbordered,
    #[serde(rename = "ayu-mirage-unbordered")]
    AyuMirageUnbordered,
    #[serde(rename = "ayu-light-unbordered")]
    AyuLightUnbordered,
    #[serde(rename = "cobalt2")]
    Cobalt2,
    #[serde(rename = "palenight-theme")]
    PalenightTheme,
    #[serde(rename = "palenight-mild-contrast")]
    PalenightMildContrast,
    #[serde(rename = "tokyo-night-storm")]
    TokyoNightStorm,
    #[serde(rename = "tokyo-night-light")]
    TokyoNightLight,
    #[serde(rename = "dracula-soft", alias = "dracula-theme-soft")]
    DraculaSoft,
    #[serde(rename = "gruvbox-dark-hard")]
    GruvboxDarkHard,
    #[serde(rename = "gruvbox-dark-soft")]
    GruvboxDarkSoft,
    #[serde(rename = "gruvbox-light-hard")]
    GruvboxLightHard,
    #[serde(rename = "gruvbox-light-soft")]
    GruvboxLightSoft,
}

/// Source-declared canvas appearance used to group Dashboard themes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeAppearance {
    Dark,
    Light,
}

impl ThemeId {
    pub const KEY: &'static str = "theme";

    /// Catalog order; the original eight retain their wire indices.
    pub const ALL: &'static [Self] = &[
        Self::Dark,
        Self::Light,
        Self::TokyoNight,
        Self::Dracula,
        Self::GruvboxDark,
        Self::GruvboxLight,
        Self::Nord,
        Self::RosePine,
        Self::CatppuccinMacchiato,
        Self::CatppuccinFrappe,
        Self::RosePineMoon,
        Self::RosePineDawn,
        Self::KanagawaWave,
        Self::KanagawaDragon,
        Self::KanagawaLotus,
        Self::EverforestDarkHard,
        Self::EverforestDarkMedium,
        Self::EverforestDarkSoft,
        Self::EverforestLightHard,
        Self::EverforestLightMedium,
        Self::EverforestLightSoft,
        Self::Nightfox,
        Self::Dayfox,
        Self::Dawnfox,
        Self::Duskfox,
        Self::Nordfox,
        Self::Terafox,
        Self::Carbonfox,
        Self::ZenbonesDark,
        Self::ZenbonesLight,
        Self::ZenwrittenDark,
        Self::ZenwrittenLight,
        Self::FlexokiDark,
        Self::FlexokiLight,
        Self::Synthwave84,
        Self::Snazzy,
        Self::OxocarbonDark,
        Self::OxocarbonLight,
        Self::Poimandres,
        Self::PoimandresStorm,
        Self::Horizon,
        Self::HorizonBright,
        Self::Andromeda,
        Self::AndromedaBordered,
        Self::Vesper,
        Self::VsCodeDark,
        Self::VsCodeLight,
        Self::VsCodeDarkHighContrast,
        Self::VsCodeLightHighContrast,
        Self::GithubDarkDefault,
        Self::GithubLightDefault,
        Self::GithubDarkDimmed,
        Self::GithubDarkHighContrast,
        Self::GithubLightHighContrast,
        Self::GithubDarkColorblindBeta,
        Self::GithubLightColorblindBeta,
        Self::OneDarkPro,
        Self::OneDarkProDarker,
        Self::OneDarkProNightFlat,
        Self::Monokai,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::NightOwl,
        Self::NightOwlLight,
        Self::AyuDark,
        Self::AyuMirage,
        Self::AyuLight,
        Self::AyuDarkUnbordered,
        Self::AyuMirageUnbordered,
        Self::AyuLightUnbordered,
        Self::Cobalt2,
        Self::PalenightTheme,
        Self::PalenightMildContrast,
        Self::TokyoNightStorm,
        Self::TokyoNightLight,
        Self::DraculaSoft,
        Self::GruvboxDarkHard,
        Self::GruvboxDarkSoft,
        Self::GruvboxLightHard,
        Self::GruvboxLightSoft,
    ];

    pub const KEYS: &'static [&'static str] = &[
        "catppuccin-mocha",
        "catppuccin-latte",
        "tokyo-night",
        "dracula",
        "gruvbox-dark",
        "gruvbox-light",
        "nord",
        "rose-pine",
        "catppuccin-macchiato",
        "catppuccin-frappe",
        "rose-pine-moon",
        "rose-pine-dawn",
        "kanagawa-wave",
        "kanagawa-dragon",
        "kanagawa-lotus",
        "everforest-dark-hard",
        "everforest-dark-medium",
        "everforest-dark-soft",
        "everforest-light-hard",
        "everforest-light-medium",
        "everforest-light-soft",
        "nightfox",
        "dayfox",
        "dawnfox",
        "duskfox",
        "nordfox",
        "terafox",
        "carbonfox",
        "zenbones-dark",
        "zenbones-light",
        "zenwritten-dark",
        "zenwritten-light",
        "flexoki-dark",
        "flexoki-light",
        "synthwave-84",
        "snazzy",
        "oxocarbon-dark",
        "oxocarbon-light",
        "poimandres",
        "poimandres-storm",
        "horizon",
        "horizon-bright",
        "andromeda",
        "andromeda-bordered",
        "vesper",
        "vs-code-dark",
        "vs-code-light",
        "vs-code-dark-high-contrast",
        "vs-code-light-high-contrast",
        "github-dark-default",
        "github-light-default",
        "github-dark-dimmed",
        "github-dark-high-contrast",
        "github-light-high-contrast",
        "github-dark-colorblind-beta",
        "github-light-colorblind-beta",
        "one-dark-pro",
        "one-dark-pro-darker",
        "one-dark-pro-night-flat",
        "monokai",
        "solarized-dark",
        "solarized-light",
        "night-owl",
        "night-owl-light",
        "ayu-dark",
        "ayu-mirage",
        "ayu-light",
        "ayu-dark-unbordered",
        "ayu-mirage-unbordered",
        "ayu-light-unbordered",
        "cobalt2",
        "palenight-theme",
        "palenight-mild-contrast",
        "tokyo-night-storm",
        "tokyo-night-light",
        "dracula-soft",
        "gruvbox-dark-hard",
        "gruvbox-dark-soft",
        "gruvbox-light-hard",
        "gruvbox-light-soft",
    ];

    pub const EXPECTED: &'static str = concat!(
        "\"catppuccin-mocha\", \"catppuccin-latte\", \"tokyo-night\", \"dracula\", \"gruvbox-dark\", \"gruvbox-light\", \"",
        "nord\", \"rose-pine\", \"catppuccin-macchiato\", \"catppuccin-frappe\", \"rose-pine-moon\", \"rose-pine-dawn\",",
        " \"kanagawa-wave\", \"kanagawa-dragon\", \"kanagawa-lotus\", \"everforest-dark-hard\", \"everforest-dark-medi",
        "um\", \"everforest-dark-soft\", \"everforest-light-hard\", \"everforest-light-medium\", \"everforest-light-s",
        "oft\", \"nightfox\", \"dayfox\", \"dawnfox\", \"duskfox\", \"nordfox\", \"terafox\", \"carbonfox\", \"zenbones-dark\"",
        ", \"zenbones-light\", \"zenwritten-dark\", \"zenwritten-light\", \"flexoki-dark\", \"flexoki-light\", \"synthwa",
        "ve-84\", \"snazzy\", \"oxocarbon-dark\", \"oxocarbon-light\", \"poimandres\", \"poimandres-storm\", \"horizon\", ",
        "\"horizon-bright\", \"andromeda\", \"andromeda-bordered\", \"vesper\", \"vs-code-dark\", \"vs-code-light\", \"vs-",
        "code-dark-high-contrast\", \"vs-code-light-high-contrast\", \"github-dark-default\", \"github-light-defaul",
        "t\", \"github-dark-dimmed\", \"github-dark-high-contrast\", \"github-light-high-contrast\", \"github-dark-co",
        "lorblind-beta\", \"github-light-colorblind-beta\", \"one-dark-pro\", \"one-dark-pro-darker\", \"one-dark-pro",
        "-night-flat\", \"monokai\", \"solarized-dark\", \"solarized-light\", \"night-owl\", \"night-owl-light\", \"ayu-d",
        "ark\", \"ayu-mirage\", \"ayu-light\", \"ayu-dark-unbordered\", \"ayu-mirage-unbordered\", \"ayu-light-unborder",
        "ed\", \"cobalt2\", \"palenight-theme\", \"palenight-mild-contrast\", \"tokyo-night-storm\", \"tokyo-night-ligh",
        "t\", \"dracula-soft\", \"gruvbox-dark-hard\", \"gruvbox-dark-soft\", \"gruvbox-light-hard\", \"gruvbox-light-s",
        "oft\" (or aliases \"dark\", \"light\", \"ros-pine-moon\", \"ros-pine-dawn\", \"dracula-theme-soft\")",
    );

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "catppuccin-mocha",
            Self::Light => "catppuccin-latte",
            Self::TokyoNight => "tokyo-night",
            Self::Dracula => "dracula",
            Self::GruvboxDark => "gruvbox-dark",
            Self::GruvboxLight => "gruvbox-light",
            Self::Nord => "nord",
            Self::RosePine => "rose-pine",
            Self::CatppuccinMacchiato => "catppuccin-macchiato",
            Self::CatppuccinFrappe => "catppuccin-frappe",
            Self::RosePineMoon => "rose-pine-moon",
            Self::RosePineDawn => "rose-pine-dawn",
            Self::KanagawaWave => "kanagawa-wave",
            Self::KanagawaDragon => "kanagawa-dragon",
            Self::KanagawaLotus => "kanagawa-lotus",
            Self::EverforestDarkHard => "everforest-dark-hard",
            Self::EverforestDarkMedium => "everforest-dark-medium",
            Self::EverforestDarkSoft => "everforest-dark-soft",
            Self::EverforestLightHard => "everforest-light-hard",
            Self::EverforestLightMedium => "everforest-light-medium",
            Self::EverforestLightSoft => "everforest-light-soft",
            Self::Nightfox => "nightfox",
            Self::Dayfox => "dayfox",
            Self::Dawnfox => "dawnfox",
            Self::Duskfox => "duskfox",
            Self::Nordfox => "nordfox",
            Self::Terafox => "terafox",
            Self::Carbonfox => "carbonfox",
            Self::ZenbonesDark => "zenbones-dark",
            Self::ZenbonesLight => "zenbones-light",
            Self::ZenwrittenDark => "zenwritten-dark",
            Self::ZenwrittenLight => "zenwritten-light",
            Self::FlexokiDark => "flexoki-dark",
            Self::FlexokiLight => "flexoki-light",
            Self::Synthwave84 => "synthwave-84",
            Self::Snazzy => "snazzy",
            Self::OxocarbonDark => "oxocarbon-dark",
            Self::OxocarbonLight => "oxocarbon-light",
            Self::Poimandres => "poimandres",
            Self::PoimandresStorm => "poimandres-storm",
            Self::Horizon => "horizon",
            Self::HorizonBright => "horizon-bright",
            Self::Andromeda => "andromeda",
            Self::AndromedaBordered => "andromeda-bordered",
            Self::Vesper => "vesper",
            Self::VsCodeDark => "vs-code-dark",
            Self::VsCodeLight => "vs-code-light",
            Self::VsCodeDarkHighContrast => "vs-code-dark-high-contrast",
            Self::VsCodeLightHighContrast => "vs-code-light-high-contrast",
            Self::GithubDarkDefault => "github-dark-default",
            Self::GithubLightDefault => "github-light-default",
            Self::GithubDarkDimmed => "github-dark-dimmed",
            Self::GithubDarkHighContrast => "github-dark-high-contrast",
            Self::GithubLightHighContrast => "github-light-high-contrast",
            Self::GithubDarkColorblindBeta => "github-dark-colorblind-beta",
            Self::GithubLightColorblindBeta => "github-light-colorblind-beta",
            Self::OneDarkPro => "one-dark-pro",
            Self::OneDarkProDarker => "one-dark-pro-darker",
            Self::OneDarkProNightFlat => "one-dark-pro-night-flat",
            Self::Monokai => "monokai",
            Self::SolarizedDark => "solarized-dark",
            Self::SolarizedLight => "solarized-light",
            Self::NightOwl => "night-owl",
            Self::NightOwlLight => "night-owl-light",
            Self::AyuDark => "ayu-dark",
            Self::AyuMirage => "ayu-mirage",
            Self::AyuLight => "ayu-light",
            Self::AyuDarkUnbordered => "ayu-dark-unbordered",
            Self::AyuMirageUnbordered => "ayu-mirage-unbordered",
            Self::AyuLightUnbordered => "ayu-light-unbordered",
            Self::Cobalt2 => "cobalt2",
            Self::PalenightTheme => "palenight-theme",
            Self::PalenightMildContrast => "palenight-mild-contrast",
            Self::TokyoNightStorm => "tokyo-night-storm",
            Self::TokyoNightLight => "tokyo-night-light",
            Self::DraculaSoft => "dracula-soft",
            Self::GruvboxDarkHard => "gruvbox-dark-hard",
            Self::GruvboxDarkSoft => "gruvbox-dark-soft",
            Self::GruvboxLightHard => "gruvbox-light-hard",
            Self::GruvboxLightSoft => "gruvbox-light-soft",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Catppuccin Mocha",
            Self::Light => "Catppuccin Latte",
            Self::TokyoNight => "Tokyo Night",
            Self::Dracula => "Dracula",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::Nord => "Nord",
            Self::RosePine => "Rosé Pine",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::RosePineMoon => "Rosé Pine Moon",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::KanagawaWave => "Kanagawa Wave",
            Self::KanagawaDragon => "Kanagawa Dragon",
            Self::KanagawaLotus => "Kanagawa Lotus",
            Self::EverforestDarkHard => "Everforest Dark Hard",
            Self::EverforestDarkMedium => "Everforest Dark Medium",
            Self::EverforestDarkSoft => "Everforest Dark Soft",
            Self::EverforestLightHard => "Everforest Light Hard",
            Self::EverforestLightMedium => "Everforest Light Medium",
            Self::EverforestLightSoft => "Everforest Light Soft",
            Self::Nightfox => "Nightfox",
            Self::Dayfox => "Dayfox",
            Self::Dawnfox => "Dawnfox",
            Self::Duskfox => "Duskfox",
            Self::Nordfox => "Nordfox",
            Self::Terafox => "Terafox",
            Self::Carbonfox => "Carbonfox",
            Self::ZenbonesDark => "Zenbones Dark",
            Self::ZenbonesLight => "Zenbones Light",
            Self::ZenwrittenDark => "Zenwritten Dark",
            Self::ZenwrittenLight => "Zenwritten Light",
            Self::FlexokiDark => "Flexoki Dark",
            Self::FlexokiLight => "Flexoki Light",
            Self::Synthwave84 => "SynthWave ’84",
            Self::Snazzy => "Snazzy",
            Self::OxocarbonDark => "Oxocarbon Dark",
            Self::OxocarbonLight => "Oxocarbon Light",
            Self::Poimandres => "Poimandres",
            Self::PoimandresStorm => "Poimandres Storm",
            Self::Horizon => "Horizon",
            Self::HorizonBright => "Horizon Bright",
            Self::Andromeda => "Andromeda",
            Self::AndromedaBordered => "Andromeda Bordered",
            Self::Vesper => "Vesper",
            Self::VsCodeDark => "VS Code Dark+",
            Self::VsCodeLight => "VS Code Light+",
            Self::VsCodeDarkHighContrast => "VS Code Dark High Contrast",
            Self::VsCodeLightHighContrast => "VS Code Light High Contrast",
            Self::GithubDarkDefault => "GitHub Dark Default",
            Self::GithubLightDefault => "GitHub Light Default",
            Self::GithubDarkDimmed => "GitHub Dark Dimmed",
            Self::GithubDarkHighContrast => "GitHub Dark High Contrast",
            Self::GithubLightHighContrast => "GitHub Light High Contrast",
            Self::GithubDarkColorblindBeta => "GitHub Dark Colorblind (Beta)",
            Self::GithubLightColorblindBeta => "GitHub Light Colorblind (Beta)",
            Self::OneDarkPro => "One Dark Pro",
            Self::OneDarkProDarker => "One Dark Pro Darker",
            Self::OneDarkProNightFlat => "One Dark Pro Night Flat",
            Self::Monokai => "Monokai",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::NightOwl => "Night Owl",
            Self::NightOwlLight => "Night Owl Light",
            Self::AyuDark => "Ayu Dark",
            Self::AyuMirage => "Ayu Mirage",
            Self::AyuLight => "Ayu Light",
            Self::AyuDarkUnbordered => "Ayu Dark Unbordered",
            Self::AyuMirageUnbordered => "Ayu Mirage Unbordered",
            Self::AyuLightUnbordered => "Ayu Light Unbordered",
            Self::Cobalt2 => "Cobalt2",
            Self::PalenightTheme => "Palenight Theme",
            Self::PalenightMildContrast => "Palenight (Mild Contrast)",
            Self::TokyoNightStorm => "Tokyo Night Storm",
            Self::TokyoNightLight => "Tokyo Night Light",
            Self::DraculaSoft => "Dracula Theme Soft",
            Self::GruvboxDarkHard => "Gruvbox Dark Hard",
            Self::GruvboxDarkSoft => "Gruvbox Dark Soft",
            Self::GruvboxLightHard => "Gruvbox Light Hard",
            Self::GruvboxLightSoft => "Gruvbox Light Soft",
        }
    }

    pub const fn gallery_id(self) -> &'static str {
        match self {
            Self::Dark => "catppuccin-mocha",
            Self::Light => "catppuccin-latte",
            Self::TokyoNight => "tokyo-night",
            Self::Dracula => "dracula-theme",
            Self::GruvboxDark => "gruvbox-dark",
            Self::GruvboxLight => "gruvbox-light",
            Self::Nord => "nord",
            Self::RosePine => "ros-pine",
            Self::CatppuccinMacchiato => "catppuccin-macchiato",
            Self::CatppuccinFrappe => "catppuccin-frappe",
            Self::RosePineMoon => "ros-pine-moon",
            Self::RosePineDawn => "ros-pine-dawn",
            Self::KanagawaWave => "kanagawa-wave",
            Self::KanagawaDragon => "kanagawa-dragon",
            Self::KanagawaLotus => "kanagawa-lotus",
            Self::EverforestDarkHard => "everforest-dark-hard",
            Self::EverforestDarkMedium => "everforest-dark-medium",
            Self::EverforestDarkSoft => "everforest-dark-soft",
            Self::EverforestLightHard => "everforest-light-hard",
            Self::EverforestLightMedium => "everforest-light-medium",
            Self::EverforestLightSoft => "everforest-light-soft",
            Self::Nightfox => "nightfox",
            Self::Dayfox => "dayfox",
            Self::Dawnfox => "dawnfox",
            Self::Duskfox => "duskfox",
            Self::Nordfox => "nordfox",
            Self::Terafox => "terafox",
            Self::Carbonfox => "carbonfox",
            Self::ZenbonesDark => "zenbones-dark",
            Self::ZenbonesLight => "zenbones-light",
            Self::ZenwrittenDark => "zenwritten-dark",
            Self::ZenwrittenLight => "zenwritten-light",
            Self::FlexokiDark => "flexoki-dark",
            Self::FlexokiLight => "flexoki-light",
            Self::Synthwave84 => "synthwave-84",
            Self::Snazzy => "snazzy",
            Self::OxocarbonDark => "oxocarbon-dark",
            Self::OxocarbonLight => "oxocarbon-light",
            Self::Poimandres => "poimandres",
            Self::PoimandresStorm => "poimandres-storm",
            Self::Horizon => "horizon",
            Self::HorizonBright => "horizon-bright",
            Self::Andromeda => "andromeda",
            Self::AndromedaBordered => "andromeda-bordered",
            Self::Vesper => "vesper",
            Self::VsCodeDark => "vs-code-dark",
            Self::VsCodeLight => "vs-code-light",
            Self::VsCodeDarkHighContrast => "vs-code-dark-high-contrast",
            Self::VsCodeLightHighContrast => "vs-code-light-high-contrast",
            Self::GithubDarkDefault => "github-dark-default",
            Self::GithubLightDefault => "github-light-default",
            Self::GithubDarkDimmed => "github-dark-dimmed",
            Self::GithubDarkHighContrast => "github-dark-high-contrast",
            Self::GithubLightHighContrast => "github-light-high-contrast",
            Self::GithubDarkColorblindBeta => "github-dark-colorblind-beta",
            Self::GithubLightColorblindBeta => "github-light-colorblind-beta",
            Self::OneDarkPro => "one-dark-pro",
            Self::OneDarkProDarker => "one-dark-pro-darker",
            Self::OneDarkProNightFlat => "one-dark-pro-night-flat",
            Self::Monokai => "monokai",
            Self::SolarizedDark => "solarized-dark",
            Self::SolarizedLight => "solarized-light",
            Self::NightOwl => "night-owl",
            Self::NightOwlLight => "night-owl-light",
            Self::AyuDark => "ayu-dark",
            Self::AyuMirage => "ayu-mirage",
            Self::AyuLight => "ayu-light",
            Self::AyuDarkUnbordered => "ayu-dark-unbordered",
            Self::AyuMirageUnbordered => "ayu-mirage-unbordered",
            Self::AyuLightUnbordered => "ayu-light-unbordered",
            Self::Cobalt2 => "cobalt2",
            Self::PalenightTheme => "palenight-theme",
            Self::PalenightMildContrast => "palenight-mild-contrast",
            Self::TokyoNightStorm => "tokyo-night-storm",
            Self::TokyoNightLight => "tokyo-night-light",
            Self::DraculaSoft => "dracula-theme-soft",
            Self::GruvboxDarkHard => "gruvbox-dark-hard",
            Self::GruvboxDarkSoft => "gruvbox-dark-soft",
            Self::GruvboxLightHard => "gruvbox-light-hard",
            Self::GruvboxLightSoft => "gruvbox-light-soft",
        }
    }

    pub const fn gallery_label(self) -> &'static str {
        match self {
            Self::Dark => "Catppuccin Mocha",
            Self::Light => "Catppuccin Latte",
            Self::TokyoNight => "Tokyo Night",
            Self::Dracula => "Dracula Theme",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::Nord => "Nord",
            Self::RosePine => "Rosé Pine",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::RosePineMoon => "Rosé Pine Moon",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::KanagawaWave => "Kanagawa Wave",
            Self::KanagawaDragon => "Kanagawa Dragon",
            Self::KanagawaLotus => "Kanagawa Lotus",
            Self::EverforestDarkHard => "Everforest Dark Hard",
            Self::EverforestDarkMedium => "Everforest Dark Medium",
            Self::EverforestDarkSoft => "Everforest Dark Soft",
            Self::EverforestLightHard => "Everforest Light Hard",
            Self::EverforestLightMedium => "Everforest Light Medium",
            Self::EverforestLightSoft => "Everforest Light Soft",
            Self::Nightfox => "Nightfox",
            Self::Dayfox => "Dayfox",
            Self::Dawnfox => "Dawnfox",
            Self::Duskfox => "Duskfox",
            Self::Nordfox => "Nordfox",
            Self::Terafox => "Terafox",
            Self::Carbonfox => "Carbonfox",
            Self::ZenbonesDark => "Zenbones Dark",
            Self::ZenbonesLight => "Zenbones Light",
            Self::ZenwrittenDark => "Zenwritten Dark",
            Self::ZenwrittenLight => "Zenwritten Light",
            Self::FlexokiDark => "Flexoki Dark",
            Self::FlexokiLight => "Flexoki Light",
            Self::Synthwave84 => "SynthWave ’84",
            Self::Snazzy => "Snazzy",
            Self::OxocarbonDark => "Oxocarbon Dark",
            Self::OxocarbonLight => "Oxocarbon Light",
            Self::Poimandres => "Poimandres",
            Self::PoimandresStorm => "Poimandres Storm",
            Self::Horizon => "Horizon",
            Self::HorizonBright => "Horizon Bright",
            Self::Andromeda => "Andromeda",
            Self::AndromedaBordered => "Andromeda Bordered",
            Self::Vesper => "Vesper",
            Self::VsCodeDark => "VS Code Dark+",
            Self::VsCodeLight => "VS Code Light+",
            Self::VsCodeDarkHighContrast => "VS Code Dark High Contrast",
            Self::VsCodeLightHighContrast => "VS Code Light High Contrast",
            Self::GithubDarkDefault => "GitHub Dark Default",
            Self::GithubLightDefault => "GitHub Light Default",
            Self::GithubDarkDimmed => "GitHub Dark Dimmed",
            Self::GithubDarkHighContrast => "GitHub Dark High Contrast",
            Self::GithubLightHighContrast => "GitHub Light High Contrast",
            Self::GithubDarkColorblindBeta => "GitHub Dark Colorblind (Beta)",
            Self::GithubLightColorblindBeta => "GitHub Light Colorblind (Beta)",
            Self::OneDarkPro => "One Dark Pro",
            Self::OneDarkProDarker => "One Dark Pro Darker",
            Self::OneDarkProNightFlat => "One Dark Pro Night Flat",
            Self::Monokai => "Monokai",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::NightOwl => "Night Owl",
            Self::NightOwlLight => "Night Owl Light",
            Self::AyuDark => "Ayu Dark",
            Self::AyuMirage => "Ayu Mirage",
            Self::AyuLight => "Ayu Light",
            Self::AyuDarkUnbordered => "Ayu Dark Unbordered",
            Self::AyuMirageUnbordered => "Ayu Mirage Unbordered",
            Self::AyuLightUnbordered => "Ayu Light Unbordered",
            Self::Cobalt2 => "Cobalt2",
            Self::PalenightTheme => "Palenight Theme",
            Self::PalenightMildContrast => "Palenight (Mild Contrast)",
            Self::TokyoNightStorm => "Tokyo Night Storm",
            Self::TokyoNightLight => "Tokyo Night Light",
            Self::DraculaSoft => "Dracula Theme Soft",
            Self::GruvboxDarkHard => "Gruvbox Dark Hard",
            Self::GruvboxDarkSoft => "Gruvbox Dark Soft",
            Self::GruvboxLightHard => "Gruvbox Light Hard",
            Self::GruvboxLightSoft => "Gruvbox Light Soft",
        }
    }

    pub const fn family(self) -> &'static str {
        match self {
            Self::Dark => "Catppuccin",
            Self::Light => "Catppuccin",
            Self::TokyoNight => "Tokyo Night",
            Self::Dracula => "Dracula",
            Self::GruvboxDark => "Gruvbox",
            Self::GruvboxLight => "Gruvbox",
            Self::Nord => "Nord",
            Self::RosePine => "Rosé Pine",
            Self::CatppuccinMacchiato => "Catppuccin",
            Self::CatppuccinFrappe => "Catppuccin",
            Self::RosePineMoon => "Rosé Pine",
            Self::RosePineDawn => "Rosé Pine",
            Self::KanagawaWave => "Kanagawa",
            Self::KanagawaDragon => "Kanagawa",
            Self::KanagawaLotus => "Kanagawa",
            Self::EverforestDarkHard => "Everforest",
            Self::EverforestDarkMedium => "Everforest",
            Self::EverforestDarkSoft => "Everforest",
            Self::EverforestLightHard => "Everforest",
            Self::EverforestLightMedium => "Everforest",
            Self::EverforestLightSoft => "Everforest",
            Self::Nightfox => "Nightfox",
            Self::Dayfox => "Nightfox",
            Self::Dawnfox => "Nightfox",
            Self::Duskfox => "Nightfox",
            Self::Nordfox => "Nightfox",
            Self::Terafox => "Nightfox",
            Self::Carbonfox => "Nightfox",
            Self::ZenbonesDark => "Zenbones / Zenwritten",
            Self::ZenbonesLight => "Zenbones / Zenwritten",
            Self::ZenwrittenDark => "Zenbones / Zenwritten",
            Self::ZenwrittenLight => "Zenbones / Zenwritten",
            Self::FlexokiDark => "Flexoki",
            Self::FlexokiLight => "Flexoki",
            Self::Synthwave84 => "SynthWave ’84",
            Self::Snazzy => "Snazzy",
            Self::OxocarbonDark => "Oxocarbon",
            Self::OxocarbonLight => "Oxocarbon",
            Self::Poimandres => "Poimandres",
            Self::PoimandresStorm => "Poimandres",
            Self::Horizon => "Horizon",
            Self::HorizonBright => "Horizon",
            Self::Andromeda => "Andromeda",
            Self::AndromedaBordered => "Andromeda",
            Self::Vesper => "Vesper",
            Self::VsCodeDark => "VS Code Defaults",
            Self::VsCodeLight => "VS Code Defaults",
            Self::VsCodeDarkHighContrast => "VS Code Defaults",
            Self::VsCodeLightHighContrast => "VS Code Defaults",
            Self::GithubDarkDefault => "GitHub",
            Self::GithubLightDefault => "GitHub",
            Self::GithubDarkDimmed => "GitHub",
            Self::GithubDarkHighContrast => "GitHub",
            Self::GithubLightHighContrast => "GitHub",
            Self::GithubDarkColorblindBeta => "GitHub",
            Self::GithubLightColorblindBeta => "GitHub",
            Self::OneDarkPro => "One Dark Pro / Atom One",
            Self::OneDarkProDarker => "One Dark Pro / Atom One",
            Self::OneDarkProNightFlat => "One Dark Pro / Atom One",
            Self::Monokai => "Monokai Classic",
            Self::SolarizedDark => "Solarized",
            Self::SolarizedLight => "Solarized",
            Self::NightOwl => "Night Owl",
            Self::NightOwlLight => "Night Owl",
            Self::AyuDark => "Ayu",
            Self::AyuMirage => "Ayu",
            Self::AyuLight => "Ayu",
            Self::AyuDarkUnbordered => "Ayu",
            Self::AyuMirageUnbordered => "Ayu",
            Self::AyuLightUnbordered => "Ayu",
            Self::Cobalt2 => "Cobalt2",
            Self::PalenightTheme => "Palenight",
            Self::PalenightMildContrast => "Palenight",
            Self::TokyoNightStorm => "Tokyo Night",
            Self::TokyoNightLight => "Tokyo Night",
            Self::DraculaSoft => "Dracula",
            Self::GruvboxDarkHard => "Gruvbox",
            Self::GruvboxDarkSoft => "Gruvbox",
            Self::GruvboxLightHard => "Gruvbox",
            Self::GruvboxLightSoft => "Gruvbox",
        }
    }

    pub const fn source_url(self) -> &'static str {
        match self {
            Self::Dark => "https://github.com/catppuccin/palette/blob/main/palette.json",
            Self::Light => "https://github.com/catppuccin/palette/blob/main/palette.json",
            Self::TokyoNight => {
                "https://raw.githubusercontent.com/tokyo-night/tokyo-night-vscode-theme/master/themes/tokyo-night-color-theme.json"
            }
            Self::Dracula => {
                "https://dracula-theme.gallery.vsassets.io/_apis/public/gallery/publisher/dracula-theme/extension/theme-dracula/2.25.1/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GruvboxDark => {
                "https://raw.githubusercontent.com/morhetz/gruvbox/master/colors/gruvbox.vim"
            }
            Self::GruvboxLight => {
                "https://raw.githubusercontent.com/morhetz/gruvbox/master/colors/gruvbox.vim"
            }
            Self::Nord => {
                "https://raw.githubusercontent.com/nordtheme/visual-studio-code/develop/themes/nord-color-theme.json"
            }
            Self::RosePine => {
                "https://github.com/rose-pine/neovim/blob/main/lua/rose-pine/palette.lua"
            }
            Self::CatppuccinMacchiato => {
                "https://github.com/catppuccin/palette/blob/main/palette.json"
            }
            Self::CatppuccinFrappe => {
                "https://github.com/catppuccin/palette/blob/main/palette.json"
            }
            Self::RosePineMoon => {
                "https://github.com/rose-pine/neovim/blob/main/lua/rose-pine/palette.lua"
            }
            Self::RosePineDawn => {
                "https://github.com/rose-pine/neovim/blob/main/lua/rose-pine/palette.lua"
            }
            Self::KanagawaWave => {
                "https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty/kanagawa_wave.toml"
            }
            Self::KanagawaDragon => {
                "https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty/kanagawa_dragon.toml"
            }
            Self::KanagawaLotus => {
                "https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty/kanagawa_lotus.toml"
            }
            Self::EverforestDarkHard => {
                "https://github.com/sainnhe/everforest/blob/master/palette.md"
            }
            Self::EverforestDarkMedium => {
                "https://github.com/sainnhe/everforest/blob/master/palette.md"
            }
            Self::EverforestDarkSoft => {
                "https://github.com/sainnhe/everforest/blob/master/palette.md"
            }
            Self::EverforestLightHard => {
                "https://github.com/sainnhe/everforest/blob/master/palette.md"
            }
            Self::EverforestLightMedium => {
                "https://github.com/sainnhe/everforest/blob/master/palette.md"
            }
            Self::EverforestLightSoft => {
                "https://github.com/sainnhe/everforest/blob/master/palette.md"
            }
            Self::Nightfox => {
                "https://github.com/EdenEast/nightfox.nvim/blob/main/extra/nightfox/alacritty.toml"
            }
            Self::Dayfox => {
                "https://github.com/EdenEast/nightfox.nvim/blob/main/extra/dayfox/alacritty.toml"
            }
            Self::Dawnfox => {
                "https://github.com/EdenEast/nightfox.nvim/blob/main/extra/dawnfox/alacritty.toml"
            }
            Self::Duskfox => {
                "https://github.com/EdenEast/nightfox.nvim/blob/main/extra/duskfox/alacritty.toml"
            }
            Self::Nordfox => {
                "https://github.com/EdenEast/nightfox.nvim/blob/main/extra/nordfox/alacritty.toml"
            }
            Self::Terafox => {
                "https://github.com/EdenEast/nightfox.nvim/blob/main/extra/terafox/alacritty.toml"
            }
            Self::Carbonfox => {
                "https://github.com/EdenEast/nightfox.nvim/blob/main/extra/carbonfox/alacritty.toml"
            }
            Self::ZenbonesDark => {
                "https://github.com/zenbones-theme/zenbones.nvim/blob/main/extras/alacritty/zenbones_dark.toml"
            }
            Self::ZenbonesLight => {
                "https://github.com/zenbones-theme/zenbones.nvim/blob/main/extras/alacritty/zenbones_light.toml"
            }
            Self::ZenwrittenDark => {
                "https://github.com/zenbones-theme/zenbones.nvim/blob/main/extras/alacritty/zenwritten_dark.toml"
            }
            Self::ZenwrittenLight => {
                "https://github.com/zenbones-theme/zenbones.nvim/blob/main/extras/alacritty/zenwritten_light.toml"
            }
            Self::FlexokiDark => "https://stephango.com/flexoki",
            Self::FlexokiLight => "https://stephango.com/flexoki",
            Self::Synthwave84 => {
                "https://github.com/robb0wen/synthwave-vscode/blob/master/themes/synthwave-color-theme.json"
            }
            Self::Snazzy => "https://github.com/sindresorhus/hyper-snazzy/blob/main/index.js",
            Self::OxocarbonDark => {
                "https://github.com/nyoom-engineering/oxocarbon.nvim/blob/main/lua/oxocarbon/init.lua"
            }
            Self::OxocarbonLight => {
                "https://github.com/nyoom-engineering/oxocarbon.nvim/blob/main/lua/oxocarbon/init.lua"
            }
            Self::Poimandres => {
                "https://github.com/drcmda/poimandres-theme/blob/main/themes/poimandres-color-theme.json"
            }
            Self::PoimandresStorm => {
                "https://github.com/drcmda/poimandres-theme/blob/main/themes/poimandres-color-theme-storm.json"
            }
            Self::Horizon => {
                "https://github.com/jolaleye/horizon-theme-vscode/blob/master/themes/horizon.json"
            }
            Self::HorizonBright => {
                "https://github.com/jolaleye/horizon-theme-vscode/blob/master/themes/horizon-bright.json"
            }
            Self::Andromeda => {
                "https://github.com/EliverLara/Andromeda/blob/master/themes/Andromeda-color-theme.json"
            }
            Self::AndromedaBordered => {
                "https://github.com/EliverLara/Andromeda/blob/master/themes/Andromeda-color-theme-bordered.json"
            }
            Self::Vesper => {
                "https://github.com/raunofreiberg/vesper/blob/main/themes/Vesper-dark-color-theme.json"
            }
            Self::VsCodeDark => {
                "https://github.com/microsoft/vscode/blob/main/extensions/theme-defaults/themes/dark_plus.json"
            }
            Self::VsCodeLight => {
                "https://github.com/microsoft/vscode/blob/main/extensions/theme-defaults/themes/light_plus.json"
            }
            Self::VsCodeDarkHighContrast => {
                "https://github.com/microsoft/vscode/blob/main/extensions/theme-defaults/themes/hc_black.json"
            }
            Self::VsCodeLightHighContrast => {
                "https://github.com/microsoft/vscode/blob/main/extensions/theme-defaults/themes/hc_light.json"
            }
            Self::GithubDarkDefault => {
                "https://GitHub.gallery.vsassets.io/_apis/public/gallery/publisher/GitHub/extension/github-vscode-theme/6.3.5/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GithubLightDefault => {
                "https://GitHub.gallery.vsassets.io/_apis/public/gallery/publisher/GitHub/extension/github-vscode-theme/6.3.5/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GithubDarkDimmed => {
                "https://GitHub.gallery.vsassets.io/_apis/public/gallery/publisher/GitHub/extension/github-vscode-theme/6.3.5/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GithubDarkHighContrast => {
                "https://GitHub.gallery.vsassets.io/_apis/public/gallery/publisher/GitHub/extension/github-vscode-theme/6.3.5/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GithubLightHighContrast => {
                "https://GitHub.gallery.vsassets.io/_apis/public/gallery/publisher/GitHub/extension/github-vscode-theme/6.3.5/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GithubDarkColorblindBeta => {
                "https://GitHub.gallery.vsassets.io/_apis/public/gallery/publisher/GitHub/extension/github-vscode-theme/6.3.5/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GithubLightColorblindBeta => {
                "https://GitHub.gallery.vsassets.io/_apis/public/gallery/publisher/GitHub/extension/github-vscode-theme/6.3.5/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::OneDarkPro => {
                "https://raw.githubusercontent.com/Binaryify/OneDark-Pro/master/themes/OneDark-Pro.json"
            }
            Self::OneDarkProDarker => {
                "https://raw.githubusercontent.com/Binaryify/OneDark-Pro/master/themes/OneDark-Pro-darker.json"
            }
            Self::OneDarkProNightFlat => {
                "https://raw.githubusercontent.com/Binaryify/OneDark-Pro/master/themes/OneDark-Pro-night-flat.json"
            }
            Self::Monokai => {
                "https://raw.githubusercontent.com/microsoft/vscode/main/extensions/theme-monokai/themes/monokai-color-theme.json"
            }
            Self::SolarizedDark => {
                "https://raw.githubusercontent.com/microsoft/vscode/main/extensions/theme-solarized-dark/themes/solarized-dark-color-theme.json"
            }
            Self::SolarizedLight => {
                "https://raw.githubusercontent.com/microsoft/vscode/main/extensions/theme-solarized-light/themes/solarized-light-color-theme.json"
            }
            Self::NightOwl => {
                "https://raw.githubusercontent.com/sdras/night-owl-vscode-theme/main/themes/Night%20Owl-color-theme.json"
            }
            Self::NightOwlLight => {
                "https://raw.githubusercontent.com/sdras/night-owl-vscode-theme/main/themes/Night%20Owl-Light-color-theme.json"
            }
            Self::AyuDark => {
                "https://raw.githubusercontent.com/ayu-theme/vscode-ayu/master/ayu-dark.json"
            }
            Self::AyuMirage => {
                "https://raw.githubusercontent.com/ayu-theme/vscode-ayu/master/ayu-mirage.json"
            }
            Self::AyuLight => {
                "https://raw.githubusercontent.com/ayu-theme/vscode-ayu/master/ayu-light.json"
            }
            Self::AyuDarkUnbordered => {
                "https://raw.githubusercontent.com/ayu-theme/vscode-ayu/master/ayu-dark-unbordered.json"
            }
            Self::AyuMirageUnbordered => {
                "https://raw.githubusercontent.com/ayu-theme/vscode-ayu/master/ayu-mirage-unbordered.json"
            }
            Self::AyuLightUnbordered => {
                "https://raw.githubusercontent.com/ayu-theme/vscode-ayu/master/ayu-light-unbordered.json"
            }
            Self::Cobalt2 => {
                "https://raw.githubusercontent.com/wesbos/cobalt2-vscode/master/theme/cobalt2.json"
            }
            Self::PalenightTheme => {
                "https://raw.githubusercontent.com/whizkydee/vscode-palenight-theme/master/themes/palenight.json"
            }
            Self::PalenightMildContrast => {
                "https://raw.githubusercontent.com/whizkydee/vscode-palenight-theme/master/themes/palenight-mild-contrast.json"
            }
            Self::TokyoNightStorm => {
                "https://raw.githubusercontent.com/tokyo-night/tokyo-night-vscode-theme/master/themes/tokyo-night-storm-color-theme.json"
            }
            Self::TokyoNightLight => {
                "https://raw.githubusercontent.com/tokyo-night/tokyo-night-vscode-theme/master/themes/tokyo-night-light-color-theme.json"
            }
            Self::DraculaSoft => {
                "https://dracula-theme.gallery.vsassets.io/_apis/public/gallery/publisher/dracula-theme/extension/theme-dracula/2.25.1/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
            }
            Self::GruvboxDarkHard => {
                "https://raw.githubusercontent.com/morhetz/gruvbox/master/colors/gruvbox.vim"
            }
            Self::GruvboxDarkSoft => {
                "https://raw.githubusercontent.com/morhetz/gruvbox/master/colors/gruvbox.vim"
            }
            Self::GruvboxLightHard => {
                "https://raw.githubusercontent.com/morhetz/gruvbox/master/colors/gruvbox.vim"
            }
            Self::GruvboxLightSoft => {
                "https://raw.githubusercontent.com/morhetz/gruvbox/master/colors/gruvbox.vim"
            }
        }
    }

    pub const fn appearance(self) -> ThemeAppearance {
        match self {
            Self::Dark => ThemeAppearance::Dark,
            Self::Light => ThemeAppearance::Light,
            Self::TokyoNight => ThemeAppearance::Dark,
            Self::Dracula => ThemeAppearance::Dark,
            Self::GruvboxDark => ThemeAppearance::Dark,
            Self::GruvboxLight => ThemeAppearance::Light,
            Self::Nord => ThemeAppearance::Dark,
            Self::RosePine => ThemeAppearance::Dark,
            Self::CatppuccinMacchiato => ThemeAppearance::Dark,
            Self::CatppuccinFrappe => ThemeAppearance::Dark,
            Self::RosePineMoon => ThemeAppearance::Dark,
            Self::RosePineDawn => ThemeAppearance::Light,
            Self::KanagawaWave => ThemeAppearance::Dark,
            Self::KanagawaDragon => ThemeAppearance::Dark,
            Self::KanagawaLotus => ThemeAppearance::Light,
            Self::EverforestDarkHard => ThemeAppearance::Dark,
            Self::EverforestDarkMedium => ThemeAppearance::Dark,
            Self::EverforestDarkSoft => ThemeAppearance::Dark,
            Self::EverforestLightHard => ThemeAppearance::Light,
            Self::EverforestLightMedium => ThemeAppearance::Light,
            Self::EverforestLightSoft => ThemeAppearance::Light,
            Self::Nightfox => ThemeAppearance::Dark,
            Self::Dayfox => ThemeAppearance::Light,
            Self::Dawnfox => ThemeAppearance::Light,
            Self::Duskfox => ThemeAppearance::Dark,
            Self::Nordfox => ThemeAppearance::Dark,
            Self::Terafox => ThemeAppearance::Dark,
            Self::Carbonfox => ThemeAppearance::Dark,
            Self::ZenbonesDark => ThemeAppearance::Dark,
            Self::ZenbonesLight => ThemeAppearance::Light,
            Self::ZenwrittenDark => ThemeAppearance::Dark,
            Self::ZenwrittenLight => ThemeAppearance::Light,
            Self::FlexokiDark => ThemeAppearance::Dark,
            Self::FlexokiLight => ThemeAppearance::Light,
            Self::Synthwave84 => ThemeAppearance::Dark,
            Self::Snazzy => ThemeAppearance::Dark,
            Self::OxocarbonDark => ThemeAppearance::Dark,
            Self::OxocarbonLight => ThemeAppearance::Light,
            Self::Poimandres => ThemeAppearance::Dark,
            Self::PoimandresStorm => ThemeAppearance::Dark,
            Self::Horizon => ThemeAppearance::Dark,
            Self::HorizonBright => ThemeAppearance::Light,
            Self::Andromeda => ThemeAppearance::Dark,
            Self::AndromedaBordered => ThemeAppearance::Dark,
            Self::Vesper => ThemeAppearance::Dark,
            Self::VsCodeDark => ThemeAppearance::Dark,
            Self::VsCodeLight => ThemeAppearance::Light,
            Self::VsCodeDarkHighContrast => ThemeAppearance::Dark,
            Self::VsCodeLightHighContrast => ThemeAppearance::Light,
            Self::GithubDarkDefault => ThemeAppearance::Dark,
            Self::GithubLightDefault => ThemeAppearance::Light,
            Self::GithubDarkDimmed => ThemeAppearance::Dark,
            Self::GithubDarkHighContrast => ThemeAppearance::Dark,
            Self::GithubLightHighContrast => ThemeAppearance::Light,
            Self::GithubDarkColorblindBeta => ThemeAppearance::Dark,
            Self::GithubLightColorblindBeta => ThemeAppearance::Light,
            Self::OneDarkPro => ThemeAppearance::Dark,
            Self::OneDarkProDarker => ThemeAppearance::Dark,
            Self::OneDarkProNightFlat => ThemeAppearance::Dark,
            Self::Monokai => ThemeAppearance::Dark,
            Self::SolarizedDark => ThemeAppearance::Dark,
            Self::SolarizedLight => ThemeAppearance::Light,
            Self::NightOwl => ThemeAppearance::Dark,
            Self::NightOwlLight => ThemeAppearance::Light,
            Self::AyuDark => ThemeAppearance::Dark,
            Self::AyuMirage => ThemeAppearance::Dark,
            Self::AyuLight => ThemeAppearance::Light,
            Self::AyuDarkUnbordered => ThemeAppearance::Dark,
            Self::AyuMirageUnbordered => ThemeAppearance::Dark,
            Self::AyuLightUnbordered => ThemeAppearance::Light,
            Self::Cobalt2 => ThemeAppearance::Dark,
            Self::PalenightTheme => ThemeAppearance::Dark,
            Self::PalenightMildContrast => ThemeAppearance::Dark,
            Self::TokyoNightStorm => ThemeAppearance::Dark,
            Self::TokyoNightLight => ThemeAppearance::Light,
            Self::DraculaSoft => ThemeAppearance::Dark,
            Self::GruvboxDarkHard => ThemeAppearance::Dark,
            Self::GruvboxDarkSoft => ThemeAppearance::Dark,
            Self::GruvboxLightHard => ThemeAppearance::Light,
            Self::GruvboxLightSoft => ThemeAppearance::Light,
        }
    }

    pub const fn is_high_contrast_variant(self) -> bool {
        match self {
            Self::Dark => false,
            Self::Light => false,
            Self::TokyoNight => false,
            Self::Dracula => false,
            Self::GruvboxDark => false,
            Self::GruvboxLight => false,
            Self::Nord => false,
            Self::RosePine => false,
            Self::CatppuccinMacchiato => false,
            Self::CatppuccinFrappe => false,
            Self::RosePineMoon => false,
            Self::RosePineDawn => false,
            Self::KanagawaWave => false,
            Self::KanagawaDragon => false,
            Self::KanagawaLotus => false,
            Self::EverforestDarkHard => false,
            Self::EverforestDarkMedium => false,
            Self::EverforestDarkSoft => false,
            Self::EverforestLightHard => false,
            Self::EverforestLightMedium => false,
            Self::EverforestLightSoft => false,
            Self::Nightfox => false,
            Self::Dayfox => false,
            Self::Dawnfox => false,
            Self::Duskfox => false,
            Self::Nordfox => false,
            Self::Terafox => false,
            Self::Carbonfox => false,
            Self::ZenbonesDark => false,
            Self::ZenbonesLight => false,
            Self::ZenwrittenDark => false,
            Self::ZenwrittenLight => false,
            Self::FlexokiDark => false,
            Self::FlexokiLight => false,
            Self::Synthwave84 => false,
            Self::Snazzy => false,
            Self::OxocarbonDark => false,
            Self::OxocarbonLight => false,
            Self::Poimandres => false,
            Self::PoimandresStorm => false,
            Self::Horizon => false,
            Self::HorizonBright => false,
            Self::Andromeda => false,
            Self::AndromedaBordered => false,
            Self::Vesper => false,
            Self::VsCodeDark => false,
            Self::VsCodeLight => false,
            Self::VsCodeDarkHighContrast => true,
            Self::VsCodeLightHighContrast => true,
            Self::GithubDarkDefault => false,
            Self::GithubLightDefault => false,
            Self::GithubDarkDimmed => false,
            Self::GithubDarkHighContrast => true,
            Self::GithubLightHighContrast => true,
            Self::GithubDarkColorblindBeta => false,
            Self::GithubLightColorblindBeta => false,
            Self::OneDarkPro => false,
            Self::OneDarkProDarker => false,
            Self::OneDarkProNightFlat => false,
            Self::Monokai => false,
            Self::SolarizedDark => false,
            Self::SolarizedLight => false,
            Self::NightOwl => false,
            Self::NightOwlLight => false,
            Self::AyuDark => false,
            Self::AyuMirage => false,
            Self::AyuLight => false,
            Self::AyuDarkUnbordered => false,
            Self::AyuMirageUnbordered => false,
            Self::AyuLightUnbordered => false,
            Self::Cobalt2 => false,
            Self::PalenightTheme => false,
            Self::PalenightMildContrast => false,
            Self::TokyoNightStorm => false,
            Self::TokyoNightLight => false,
            Self::DraculaSoft => false,
            Self::GruvboxDarkHard => false,
            Self::GruvboxDarkSoft => false,
            Self::GruvboxLightHard => false,
            Self::GruvboxLightSoft => false,
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "catppuccin-mocha" | "dark" => Some(Self::Dark),
            "catppuccin-latte" | "light" => Some(Self::Light),
            "tokyo-night" => Some(Self::TokyoNight),
            "dracula" => Some(Self::Dracula),
            "gruvbox-dark" => Some(Self::GruvboxDark),
            "gruvbox-light" => Some(Self::GruvboxLight),
            "nord" => Some(Self::Nord),
            "rose-pine" => Some(Self::RosePine),
            "catppuccin-macchiato" => Some(Self::CatppuccinMacchiato),
            "catppuccin-frappe" => Some(Self::CatppuccinFrappe),
            "rose-pine-moon" | "ros-pine-moon" => Some(Self::RosePineMoon),
            "rose-pine-dawn" | "ros-pine-dawn" => Some(Self::RosePineDawn),
            "kanagawa-wave" => Some(Self::KanagawaWave),
            "kanagawa-dragon" => Some(Self::KanagawaDragon),
            "kanagawa-lotus" => Some(Self::KanagawaLotus),
            "everforest-dark-hard" => Some(Self::EverforestDarkHard),
            "everforest-dark-medium" => Some(Self::EverforestDarkMedium),
            "everforest-dark-soft" => Some(Self::EverforestDarkSoft),
            "everforest-light-hard" => Some(Self::EverforestLightHard),
            "everforest-light-medium" => Some(Self::EverforestLightMedium),
            "everforest-light-soft" => Some(Self::EverforestLightSoft),
            "nightfox" => Some(Self::Nightfox),
            "dayfox" => Some(Self::Dayfox),
            "dawnfox" => Some(Self::Dawnfox),
            "duskfox" => Some(Self::Duskfox),
            "nordfox" => Some(Self::Nordfox),
            "terafox" => Some(Self::Terafox),
            "carbonfox" => Some(Self::Carbonfox),
            "zenbones-dark" => Some(Self::ZenbonesDark),
            "zenbones-light" => Some(Self::ZenbonesLight),
            "zenwritten-dark" => Some(Self::ZenwrittenDark),
            "zenwritten-light" => Some(Self::ZenwrittenLight),
            "flexoki-dark" => Some(Self::FlexokiDark),
            "flexoki-light" => Some(Self::FlexokiLight),
            "synthwave-84" => Some(Self::Synthwave84),
            "snazzy" => Some(Self::Snazzy),
            "oxocarbon-dark" => Some(Self::OxocarbonDark),
            "oxocarbon-light" => Some(Self::OxocarbonLight),
            "poimandres" => Some(Self::Poimandres),
            "poimandres-storm" => Some(Self::PoimandresStorm),
            "horizon" => Some(Self::Horizon),
            "horizon-bright" => Some(Self::HorizonBright),
            "andromeda" => Some(Self::Andromeda),
            "andromeda-bordered" => Some(Self::AndromedaBordered),
            "vesper" => Some(Self::Vesper),
            "vs-code-dark" => Some(Self::VsCodeDark),
            "vs-code-light" => Some(Self::VsCodeLight),
            "vs-code-dark-high-contrast" => Some(Self::VsCodeDarkHighContrast),
            "vs-code-light-high-contrast" => Some(Self::VsCodeLightHighContrast),
            "github-dark-default" => Some(Self::GithubDarkDefault),
            "github-light-default" => Some(Self::GithubLightDefault),
            "github-dark-dimmed" => Some(Self::GithubDarkDimmed),
            "github-dark-high-contrast" => Some(Self::GithubDarkHighContrast),
            "github-light-high-contrast" => Some(Self::GithubLightHighContrast),
            "github-dark-colorblind-beta" => Some(Self::GithubDarkColorblindBeta),
            "github-light-colorblind-beta" => Some(Self::GithubLightColorblindBeta),
            "one-dark-pro" => Some(Self::OneDarkPro),
            "one-dark-pro-darker" => Some(Self::OneDarkProDarker),
            "one-dark-pro-night-flat" => Some(Self::OneDarkProNightFlat),
            "monokai" => Some(Self::Monokai),
            "solarized-dark" => Some(Self::SolarizedDark),
            "solarized-light" => Some(Self::SolarizedLight),
            "night-owl" => Some(Self::NightOwl),
            "night-owl-light" => Some(Self::NightOwlLight),
            "ayu-dark" => Some(Self::AyuDark),
            "ayu-mirage" => Some(Self::AyuMirage),
            "ayu-light" => Some(Self::AyuLight),
            "ayu-dark-unbordered" => Some(Self::AyuDarkUnbordered),
            "ayu-mirage-unbordered" => Some(Self::AyuMirageUnbordered),
            "ayu-light-unbordered" => Some(Self::AyuLightUnbordered),
            "cobalt2" => Some(Self::Cobalt2),
            "palenight-theme" => Some(Self::PalenightTheme),
            "palenight-mild-contrast" => Some(Self::PalenightMildContrast),
            "tokyo-night-storm" => Some(Self::TokyoNightStorm),
            "tokyo-night-light" => Some(Self::TokyoNightLight),
            "dracula-soft" | "dracula-theme-soft" => Some(Self::DraculaSoft),
            "gruvbox-dark-hard" => Some(Self::GruvboxDarkHard),
            "gruvbox-dark-soft" => Some(Self::GruvboxDarkSoft),
            "gruvbox-light-hard" => Some(Self::GruvboxLightHard),
            "gruvbox-light-soft" => Some(Self::GruvboxLightSoft),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|theme| *theme == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

impl fmt::Display for ThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// When OVRCR automatically creates a terminal named `local`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutomaticLocalTerminals {
    /// Every newly provisioned workspace gets a local terminal.
    On,
    /// Never create local terminals automatically.
    Off,
    /// Only the project's detected default-branch (root) workspace gets one.
    #[default]
    DefaultBranchOnly,
}

impl AutomaticLocalTerminals {
    pub const KEY: &'static str = "automatic_local_terminals";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::DefaultBranchOnly => "default_branch_only",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::DefaultBranchOnly => "default branch only",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            "default_branch_only" | "default branch only" => Some(Self::DefaultBranchOnly),
            _ => None,
        }
    }

    /// Cycle for the Dashboard preference control.
    pub fn next(self) -> Self {
        match self {
            Self::DefaultBranchOnly => Self::On,
            Self::On => Self::Off,
            Self::Off => Self::DefaultBranchOnly,
        }
    }

    /// Whether automatic provisioning should create a `local` terminal.
    ///
    /// `is_default_branch_workspace` is true only for the protected repository-root
    /// workspace (the detected default-branch checkout), never a feature worktree.
    pub fn should_create(self, is_default_branch_workspace: bool) -> bool {
        match self {
            Self::On => true,
            Self::Off => false,
            Self::DefaultBranchOnly => is_default_branch_workspace,
        }
    }
}

impl fmt::Display for AutomaticLocalTerminals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOverride {
    pub name: String,
    pub argv: Vec<String>,
}

/// A plain enum on the wire: bincode cannot decode serde-tagged enums. The
/// settings loader reads the document's `kind`/`preset` form itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchChoice {
    Terminal,
    Agent(String),
}

/// One native CLI the Server runs for account quota.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCommand {
    pub command: PathBuf,
    pub home: Option<PathBuf>,
}

/// Experimental personal Cursor dashboard reader; no credentials in settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorQuotaSettings {
    pub dashboard: bool,
    /// Absolute SQLite state.vscdb path, or the platform default when absent.
    pub state_db: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaSettings {
    /// Codex, Grok and opt-in Cursor account collection. On by default while a Dashboard is
    /// attached. `false` is the explicit off switch. The readers do not rewrite
    /// auth files.
    pub enabled: bool,
    /// Consent setting: a hidden Claude probe may spend allowance to read it.
    pub claude_probe: bool,
    pub codex: NativeCommand,
    pub grok: NativeCommand,
    pub cursor: CursorQuotaSettings,
}

impl Default for QuotaSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            cursor: CursorQuotaSettings::default(),
            claude_probe: false,
            codex: NativeCommand {
                command: "codex".into(),
                home: None,
            },
            grok: NativeCommand {
                command: "grok".into(),
                home: None,
            },
        }
    }
}

/// Every effective value. `picker_roots` is already `~`-expanded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub desktop_notifications: bool,
    pub ready_sound: bool,
    /// Absent document choice is `Some(Default)`; an explicit invalid value is
    /// `None` with a finding, so consumers suppress sound without changing flags.
    pub ready_sound_choice: Option<ReadySoundChoice>,
    /// Dashboard palette: Dark (Mocha) or Light (Latte). Default Dark.
    pub theme: ThemeId,
    /// Separate consent for exact existing-iTerm-session focus; OS permission
    /// is requested only by the explicit Dashboard setup action.
    pub iterm_focus: bool,
    pub automatic_local_terminals: AutomaticLocalTerminals,
    /// `provider/model`, validated; `None` means titles are off.
    pub title_model: Option<String>,
    pub branch_prefix: String,
    pub picker_roots: Vec<PathBuf>,
    pub agents: Vec<AgentOverride>,
    pub launch_choices: BTreeMap<String, LaunchChoice>,
    pub quota: QuotaSettings,
    /// Consent setting, off until set. When on, a dirty feature worktree is
    /// pushed to `origin/wip/<branch>` when that workspace is removed and when
    /// the server shuts down, without a prompt. The checkout's own branch is
    /// not moved. The root workspace is never saved.
    #[serde(default)]
    pub save_uncommitted_work: bool,
}

/// Shown while `save_uncommitted_work` is off.
pub const SAVE_UNCOMMITTED_WORK_OFF: &str = "Uncommitted workspace work is not pushed. Set `save_uncommitted_work = true` to save it to origin/wip/<branch> when a workspace is removed or the server shuts down.";

/// Shown while `desktop_notifications` is off.
pub const DESKTOP_NOTIFICATIONS_OFF: &str = "Desktop alerts off: set `desktop_notifications = true` in dashboard.toml (needs OS notification permission)";

/// Shown while `iterm_focus` is off.
pub const ITERM_FOCUS_OFF: &str = "Exact iTerm focus off: set `iterm_focus = true`, then choose Set up iTerm focus in the Dashboard palette (separate Automation permission)";

/// Shown while `title_model` is unset.
pub const TITLES_OFF: &str =
    "Automatic titles off: set `title_model = \"provider/model\"` in dashboard.toml";

/// Every default except `picker_roots`, which is empty here: the loader
/// fills it with whichever of `~/Code`, `~/src` and `~` exist.
impl Default for Settings {
    fn default() -> Self {
        Self {
            desktop_notifications: false,
            ready_sound: false,
            ready_sound_choice: Some(ReadySoundChoice::Default),
            theme: ThemeId::Dark,
            iterm_focus: false,
            automatic_local_terminals: AutomaticLocalTerminals::default(),
            title_model: None,
            branch_prefix: "feature/".into(),
            picker_roots: Vec::new(),
            agents: Vec::new(),
            launch_choices: BTreeMap::new(),
            quota: QuotaSettings::default(),
            save_uncommitted_work: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingOwner {
    Server,
    Dashboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingSource {
    Default,
    Document,
}

/// One setting's effective value for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingRow {
    /// Dotted key path, for example `quota.codex.home`.
    pub key: String,
    pub owner: SettingOwner,
    /// Display text; `None` means unset.
    pub value: Option<String>,
    pub source: SettingSource,
    /// Display text of the default; `None` means unset by default.
    pub default: Option<String>,
    /// For a consent setting that is off: what to set to turn it on.
    pub off_state: Option<String>,
}

/// One load problem. `key` is `None` for a document-level finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsFinding {
    pub key: Option<String>,
    pub message: String,
    pub line: Option<u32>,
}

/// The Server's reading of the settings document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsReport {
    pub path: PathBuf,
    pub read_unix_ms: u64,
    pub settings: Settings,
    pub rows: Vec<SettingRow>,
    pub findings: Vec<SettingsFinding>,
    /// The document is not valid TOML: every setting is its default and the
    /// Server refuses edits until the file is fixed by hand.
    pub unparseable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_sound_choices_have_stable_ids_labels_and_default() {
        let choices = [
            (ReadySoundChoice::Default, "default", "System default"),
            (ReadySoundChoice::Tap, "tap", "Tap"),
            (ReadySoundChoice::Chime, "chime", "Chime"),
            (ReadySoundChoice::Rise, "rise", "Rise"),
        ];
        for (index, (choice, id, label)) in choices.iter().enumerate() {
            assert_eq!(choice.as_str(), *id);
            assert_eq!(choice.label(), *label);
            assert_eq!(ReadySoundChoice::parse(id), Some(*choice));
            assert_eq!(choice.next(), choices[(index + 1) % choices.len()].0);
            assert_eq!(serde_json::to_value(choice).unwrap(), *id);
            assert_eq!(
                serde_json::from_value::<ReadySoundChoice>((*id).into()).unwrap(),
                *choice
            );
        }
        let settings = Settings::default();
        assert_eq!(settings.ready_sound_choice, Some(ReadySoundChoice::Default));
        assert!(!settings.desktop_notifications);
        assert!(!settings.ready_sound);
        for invalid in ["", "Glass", "TAP", " tap", "tap ", "/tmp/tap.wav"] {
            assert_eq!(ReadySoundChoice::parse(invalid), None);
        }
    }

    #[test]
    fn settings_wire_retains_valid_and_invalid_sound_choices_without_enabling_flags() {
        for choice in [
            None,
            Some(ReadySoundChoice::Default),
            Some(ReadySoundChoice::Tap),
            Some(ReadySoundChoice::Chime),
            Some(ReadySoundChoice::Rise),
        ] {
            let settings = Settings {
                ready_sound_choice: choice,
                ..Default::default()
            };
            let encoded =
                bincode::serde::encode_to_vec(&settings, bincode::config::standard()).unwrap();
            let (decoded, consumed): (Settings, usize) =
                bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
            assert_eq!(decoded, settings);
            assert_eq!(consumed, encoded.len());
            assert!(!decoded.desktop_notifications && !decoded.ready_sound);
        }
    }

    #[test]
    fn default_is_default_branch_only() {
        assert_eq!(
            AutomaticLocalTerminals::default(),
            AutomaticLocalTerminals::DefaultBranchOnly
        );
        assert!(AutomaticLocalTerminals::DefaultBranchOnly.should_create(true));
        assert!(!AutomaticLocalTerminals::DefaultBranchOnly.should_create(false));
        assert!(AutomaticLocalTerminals::On.should_create(false));
        assert!(!AutomaticLocalTerminals::Off.should_create(true));
    }

    #[test]
    fn parse_accepts_documented_spellings() {
        assert_eq!(
            AutomaticLocalTerminals::parse("on"),
            Some(AutomaticLocalTerminals::On)
        );
        assert_eq!(
            AutomaticLocalTerminals::parse("off"),
            Some(AutomaticLocalTerminals::Off)
        );
        assert_eq!(
            AutomaticLocalTerminals::parse("default_branch_only"),
            Some(AutomaticLocalTerminals::DefaultBranchOnly)
        );
        assert_eq!(
            AutomaticLocalTerminals::parse("default branch only"),
            Some(AutomaticLocalTerminals::DefaultBranchOnly)
        );
        assert_eq!(AutomaticLocalTerminals::parse("always"), None);
        assert_eq!(AutomaticLocalTerminals::parse(""), None);
    }

    #[test]
    fn theme_ids_have_stable_ids_labels_and_default() {
        let choices = [
            (ThemeId::Dark, "catppuccin-mocha", "Catppuccin Mocha"),
            (ThemeId::Light, "catppuccin-latte", "Catppuccin Latte"),
            (ThemeId::TokyoNight, "tokyo-night", "Tokyo Night"),
            (ThemeId::Dracula, "dracula", "Dracula"),
            (ThemeId::GruvboxDark, "gruvbox-dark", "Gruvbox Dark"),
            (ThemeId::GruvboxLight, "gruvbox-light", "Gruvbox Light"),
            (ThemeId::Nord, "nord", "Nord"),
            (ThemeId::RosePine, "rose-pine", "Rosé Pine"),
        ];
        assert_eq!(ThemeId::ALL.len(), 80);
        for (index, (choice, id, label)) in choices.iter().enumerate() {
            assert_eq!(ThemeId::ALL[index], *choice);
            assert_eq!(choice.as_str(), *id);
            assert_eq!(choice.label(), *label);
            assert_eq!(ThemeId::parse(id), Some(*choice));
            assert_eq!(serde_json::to_value(choice).unwrap(), *id);
            assert_eq!(
                serde_json::from_value::<ThemeId>((*id).into()).unwrap(),
                *choice
            );
        }
        assert_eq!(ThemeId::parse("dark"), Some(ThemeId::Dark));
        assert_eq!(ThemeId::parse("light"), Some(ThemeId::Light));
        assert_eq!(
            serde_json::from_value::<ThemeId>("dark".into()).unwrap(),
            ThemeId::Dark
        );
        assert_eq!(
            serde_json::from_value::<ThemeId>("light".into()).unwrap(),
            ThemeId::Light
        );
        assert_eq!(Settings::default().theme, ThemeId::Dark);
        for invalid in [
            "",
            "Dark",
            "LIGHT",
            " dark",
            "mocha",
            "latte",
            "tokyo_night",
        ] {
            assert_eq!(ThemeId::parse(invalid), None);
        }
    }

    #[test]
    fn theme_appearance_metadata_groups_every_existing_theme() {
        let dark = [
            ThemeId::Dark,
            ThemeId::TokyoNight,
            ThemeId::Dracula,
            ThemeId::GruvboxDark,
            ThemeId::Nord,
            ThemeId::RosePine,
        ];
        let light = [ThemeId::Light, ThemeId::GruvboxLight];

        assert_eq!(dark.len() + light.len(), 8);
        for theme in dark {
            assert_eq!(theme.appearance(), ThemeAppearance::Dark, "{theme}");
        }
        for theme in light {
            assert_eq!(theme.appearance(), ThemeAppearance::Light, "{theme}");
        }
    }

    #[test]
    fn theme_catalog_matches_the_exact_approved_manifest() {
        use std::collections::HashSet;
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/themes/source/OVRCR-approved-palette-manifest.json"
        ))
        .unwrap();
        let approved = manifest["palettes"].as_array().unwrap();
        assert_eq!(approved.len(), ThemeId::ALL.len());
        assert_eq!(ThemeId::KEYS.len(), ThemeId::ALL.len());
        let mut native_ids = HashSet::new();
        let mut gallery_ids = HashSet::new();
        let mut families = HashSet::new();
        let mut dark = 0;
        let mut light = 0;
        let mut high_contrast = 0;
        for (index, theme) in ThemeId::ALL.iter().copied().enumerate() {
            assert!(native_ids.insert(theme.as_str()));
            assert!(gallery_ids.insert(theme.gallery_id()));
            families.insert(theme.family());
            assert_eq!(ThemeId::KEYS[index], theme.as_str());
            assert_eq!(ThemeId::parse(theme.as_str()), Some(theme));
            assert_eq!(serde_json::to_value(theme).unwrap(), theme.as_str());
            assert_eq!(
                serde_json::from_value::<ThemeId>(theme.as_str().into()).unwrap(),
                theme
            );
            assert_eq!(theme.next(), ThemeId::ALL[(index + 1) % ThemeId::ALL.len()]);
            let row = approved
                .iter()
                .find(|p| p["gallery_id"] == theme.gallery_id())
                .unwrap();
            assert_eq!(row["gallery_name"], theme.gallery_label());
            assert_eq!(row["family_name"], theme.family());
            assert_eq!(row["source_url"], theme.source_url());
            assert_eq!(
                row["high_contrast_variant"],
                theme.is_high_contrast_variant()
            );
            assert!(!row["appearance_evidence"].is_null());
            match theme.appearance() {
                ThemeAppearance::Dark => {
                    assert_eq!(row["appearance"], "dark");
                    dark += 1;
                }
                ThemeAppearance::Light => {
                    assert_eq!(row["appearance"], "light");
                    light += 1;
                }
            }
            high_contrast += usize::from(theme.is_high_contrast_variant());
        }
        assert_eq!(
            (dark, light, high_contrast, families.len()),
            (54, 26, 4, 27)
        );
        for excluded in ["material", "vira", "monokai-pro", "shades-of-purple"] {
            assert!(!native_ids.iter().any(|id| id.contains(excluded)));
        }
        let entry = crate::entries()
            .iter()
            .find(|entry| entry.path == ThemeId::KEY)
            .unwrap();
        let crate::Shape::Pick { options, .. } = entry.shape else {
            panic!("theme must be a pick")
        };
        assert_eq!(options, ThemeId::KEYS);
        assert!(
            entry
                .about
                .join(" ")
                .contains(&format!("{} built-in palettes", approved.len())),
            "Settings help must describe the approved catalog, not a stale eight-theme list"
        );
    }

    #[test]
    fn original_theme_wire_indices_and_compatible_aliases_are_preserved() {
        for (index, theme) in ThemeId::ALL[..8].iter().copied().enumerate() {
            assert_eq!(
                bincode::serde::encode_to_vec(theme, bincode::config::standard()).unwrap(),
                vec![index as u8]
            );
        }
        for (alias, theme) in [
            ("dark", ThemeId::Dark),
            ("light", ThemeId::Light),
            ("ros-pine-moon", ThemeId::RosePineMoon),
            ("ros-pine-dawn", ThemeId::RosePineDawn),
            ("dracula-theme-soft", ThemeId::DraculaSoft),
        ] {
            assert_eq!(ThemeId::parse(alias), Some(theme));
            assert_eq!(
                serde_json::from_value::<ThemeId>(alias.into()).unwrap(),
                theme
            );
            assert_eq!(serde_json::to_value(theme).unwrap(), theme.as_str());
        }
    }
}
