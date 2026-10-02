//! The Dashboard's settings writer. Reading belongs to the Server, which
//! publishes its reading as a `SettingsReport`; the Dashboard has no parser.
//! Until edits move through the Server, the writer edits the document path
//! that report names.
pub use ovrcr_protocol::{AgentOverride, AutomaticLocalTerminals, LaunchChoice, Settings};
use ovrcr_protocol::{SettingOwner, SettingSource, SettingsReport};
use std::path::Path;

/// The footer line for a reading with `count` findings.
pub(super) fn findings_notice(count: usize) -> String {
    match count {
        0 => "No settings findings".into(),
        1 => "1 settings finding; see Settings".into(),
        count => format!("{count} settings findings; see Settings"),
    }
}

/// The Settings popup: the document and when the Server read it, findings
/// first, then every setting in the order the Server published.
pub(super) fn report_lines(report: Option<&SettingsReport>, now: u64) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Waiting for the Server's settings reading.".into()];
    };
    let read = ovrcr_protocol::freshness::age_ms(report.read_unix_ms, now)
        .map(|age| format!("{}s ago", age / 1_000))
        .unwrap_or_else(|| "at an unverifiable time".into());
    let mut lines = vec![
        format!("Document: {}", report.path.display()),
        format!("Read by the Server {read}"),
        String::new(),
        format!("Findings: {}", report.findings.len()),
    ];
    for finding in &report.findings {
        let line = finding
            .line
            .map(|line| format!("line {line}, "))
            .unwrap_or_default();
        let key = finding.key.as_deref().unwrap_or("document");
        lines.push(format!("  {line}{key}: {}", finding.message));
    }
    lines.push(String::new());
    for row in &report.rows {
        let owner = match row.owner {
            SettingOwner::Server => "Server",
            SettingOwner::Dashboard => "Dashboard",
        };
        let source = match row.source {
            SettingSource::Default => "default",
            SettingSource::Document => "document",
        };
        let value = row.value.as_deref().unwrap_or("unset");
        lines.push(format!("{} = {value}  ({owner}, {source})", row.key));
        if let Some(off) = &row.off_state {
            lines.push(format!("  {off}"));
        }
    }
    lines
}

/// Update only the selected preference, keeping other configuration values.
/// Publish with one rename so readers never see a partially written document.
pub(super) fn save_alert_setting(path: &Path, key: &str, enabled: bool) -> anyhow::Result<()> {
    save_settings(path, |document| {
        set_value(&mut document[key], enabled.into());
        Ok(())
    })
}

pub(super) fn save_automatic_local_terminals(
    path: &Path,
    policy: AutomaticLocalTerminals,
) -> anyhow::Result<()> {
    save_settings(path, |document| {
        set_value(
            &mut document[AutomaticLocalTerminals::KEY],
            policy.as_str().into(),
        );
        Ok(())
    })
}

fn set_value(item: &mut toml_edit::Item, mut value: toml_edit::Value) {
    if let Some(existing) = item.as_value() {
        *value.decor_mut() = existing.decor().clone();
    }
    *item = toml_edit::Item::Value(value);
}

/// Every edit reads the latest document before changing it. A document that is
/// not TOML is refused; a wrongly typed value elsewhere is the Server's finding
/// to report, and the edit keeps it byte for byte.
fn save_settings(
    path: &Path,
    edit: impl FnOnce(&mut toml_edit::DocumentMut) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    use std::io::Write;

    // A dangling link is not a missing config: never replace the link itself.
    let entry = match std::fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("inspect dashboard settings"),
    };
    // Follow an existing symlink rather than replacing the user's config link.
    let path = match std::fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && entry.is_none() => {
            path.to_path_buf()
        }
        Err(error) => return Err(error).context("locate dashboard settings"),
    };
    let permissions = match std::fs::metadata(&path) {
        Ok(metadata) => {
            let permissions = metadata.permissions();
            if permissions.readonly() {
                bail!("dashboard settings are read-only");
            }
            Some(permissions)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("inspect dashboard settings permissions"),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("read dashboard settings"),
    };
    let mut document: toml_edit::DocumentMut =
        contents.parse().context("parse dashboard settings")?;
    edit(&mut document)?;
    let contents = document.to_string();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context("create dashboard settings directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("create temporary dashboard settings")?;
    temporary
        .write_all(contents.as_bytes())
        .context("write dashboard settings")?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .context("preserve dashboard settings permissions")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("sync dashboard settings")?;
    temporary
        .persist(&path)
        // PersistError owns the temporary file; discard it even if the caller retains the error.
        .map_err(|error| error.error)
        .context("replace dashboard settings")?;
    Ok(())
}

pub(super) fn save_launch_choice(
    path: &Path,
    project: &str,
    choice: &LaunchChoice,
) -> Result<(), String> {
    save_settings(path, |document| {
        use anyhow::Context;
        let choices = document
            .entry("launch_choices")
            .or_insert(toml_edit::table())
            .as_table_like_mut()
            .context("launch_choices must be a table")?;
        let entry = choices
            .entry(project)
            .or_insert(toml_edit::table())
            .as_table_like_mut()
            .context("launch choice must be a table")?;
        let kind = match choice {
            LaunchChoice::Terminal => {
                entry.remove("preset");
                "Terminal"
            }
            LaunchChoice::Agent(preset) => {
                set_value(
                    entry.entry("preset").or_insert(toml_edit::Item::None),
                    preset.clone().into(),
                );
                "Agent"
            }
        };
        set_value(
            entry.entry("kind").or_insert(toml_edit::Item::None),
            kind.into(),
        );
        Ok(())
    })
    .map_err(|error| format!("{error:#}"))
}

/// The document as written, for tests that check what a save produced. The
/// Server's loader owns the meaning of these values; this only reads them back.
#[cfg(test)]
pub(super) fn written(path: &Path) -> toml::Table {
    toml::from_str(&std::fs::read_to_string(path).unwrap_or_default()).unwrap()
}

#[cfg(test)]
pub(super) fn written_choice(path: &Path, project: &str) -> Option<LaunchChoice> {
    let document = written(path);
    let entry = document.get("launch_choices")?.get(project)?;
    match entry.get("kind")?.as_str()? {
        "Terminal" => Some(LaunchChoice::Terminal),
        "Agent" => Some(LaunchChoice::Agent(
            entry.get("preset")?.as_str()?.to_owned(),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn failed_replacement_cleans_temporary_file_before_returning_error() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        // Deterministically obstruct replacement after reading, without a race or fault hook.
        let result = save_settings(&path, |document| {
            document["ready_sound"] = toml_edit::value(true);
            std::fs::create_dir(&path)?;
            Ok(())
        });
        assert!(result.is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        drop(result);
    }

    #[test]
    fn launch_edits_preserve_existing_table_styles_and_unknown_fields() {
        for original in [
            "# config\n[launch_choices.'project.with.dots'] # project\nkind = 'Agent' # kind\npreset = 'old' # preset\nextra = 42 # future\n",
            "# config\nlaunch_choices = { 'project.with.dots' = { kind = 'Agent', preset = 'old', extra = 42 } } # future\n",
            "# config\nlaunch_choices.'project.with.dots'.kind = 'Agent' # kind\nlaunch_choices.'project.with.dots'.preset = 'old' # preset\nlaunch_choices.'project.with.dots'.extra = 42 # future\n",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("dashboard.toml");
            std::fs::write(&path, original).unwrap();
            for choice in [LaunchChoice::Agent("new".into()), LaunchChoice::Terminal] {
                save_launch_choice(&path, "project.with.dots", &choice).unwrap();
                let saved = std::fs::read_to_string(&path).unwrap();
                assert!(saved.contains("# config"));
                assert!(saved.contains("# future"));
                assert!(saved.contains("extra = 42"));
                if matches!(choice, LaunchChoice::Agent(_)) {
                    assert_eq!(
                        saved,
                        original
                            .replace("'old'", "\"new\"")
                            .replace("'Agent'", "\"Agent\"")
                    );
                } else {
                    assert!(!saved.contains("preset"));
                }
                assert_eq!(written_choice(&path, "project.with.dots"), Some(choice));
            }
        }
    }

    #[test]
    fn launch_save_preserves_unrelated_settings_and_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        std::fs::write(&path, "ready_sound = true\n[future]\nvalue = 42\n").unwrap();
        let occupied = path.with_extension("toml.tmp");
        std::fs::write(&occupied, "unrelated").unwrap();
        save_launch_choice(
            &path,
            "project.with.dots",
            &LaunchChoice::Agent("fixture".into()),
        )
        .unwrap();
        let document: toml::Table =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(document["future"]["value"].as_integer(), Some(42));
        assert_eq!(std::fs::read_to_string(occupied).unwrap(), "unrelated");
        assert_eq!(document["ready_sound"].as_bool(), Some(true));
        assert_eq!(
            written_choice(&path, "project.with.dots"),
            Some(LaunchChoice::Agent("fixture".into()))
        );
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            2,
            "no disposable writer file remains"
        );
    }
}
