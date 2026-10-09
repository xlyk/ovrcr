//! All approved choices use the one production settings loader and writer.
use ovrcr_protocol::ThemeId;
use ovrcr_runtime::settings;

#[test]
fn all_80_themes_persist_reload_and_reject_unknown_near_misses() {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, "# keep my config\nready_sound = false\n").unwrap();
    for theme in ThemeId::ALL.iter().copied() {
        settings::set(&document, ThemeId::KEY, Some(theme.as_str())).unwrap();
        let report = settings::load_document(root.path(), &document);
        assert_eq!(report.settings.theme, theme);
        assert!(report.findings.is_empty(), "{theme}: {:?}", report.findings);
        let before = std::fs::read_to_string(&document).unwrap();
        assert!(before.contains("# keep my config"));
        assert!(before.contains("ready_sound = false"));
        assert!(before.contains(&format!("theme = \"{}\"", theme.as_str())));
        let near_miss = format!("{}-not-a-variant", theme.as_str());
        assert!(settings::set(&document, ThemeId::KEY, Some(&near_miss)).is_err());
        assert_eq!(std::fs::read_to_string(&document).unwrap(), before);
    }
    settings::set(&document, ThemeId::KEY, None).unwrap();
    assert_eq!(
        settings::load_document(root.path(), &document)
            .settings
            .theme,
        ThemeId::Dark
    );
}

#[test]
fn compatible_aliases_load_without_rewriting_existing_documents() {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    for (alias, theme) in [
        ("dark", ThemeId::Dark),
        ("light", ThemeId::Light),
        ("ros-pine-moon", ThemeId::RosePineMoon),
        ("ros-pine-dawn", ThemeId::RosePineDawn),
        ("dracula-theme-soft", ThemeId::DraculaSoft),
    ] {
        let original = format!("# historical spelling\ntheme = '{alias}'\n");
        std::fs::write(&document, &original).unwrap();
        let report = settings::load_document(root.path(), &document);
        assert_eq!(report.settings.theme, theme);
        assert!(report.findings.is_empty());
        assert_eq!(std::fs::read_to_string(&document).unwrap(), original);
    }
}

#[test]
fn research_only_families_remain_invalid_and_do_not_replace_saved_theme() {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, "theme = 'catppuccin-mocha'\n").unwrap();
    for name in ["material", "vira", "monokai-pro", "shades-of-purple"] {
        assert!(settings::set(&document, ThemeId::KEY, Some(name)).is_err());
        assert_eq!(
            std::fs::read_to_string(&document).unwrap(),
            "theme = 'catppuccin-mocha'\n"
        );
    }
}
