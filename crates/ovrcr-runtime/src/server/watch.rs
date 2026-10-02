//! The Server's settings watcher: a 2-second stat and change-only republish.
//!
//! The title worker's poll drives [`ServerState::poll_settings`]. It reloads
//! only when the document's (or the instance identity's) length or mtime
//! changed, and replaces and republishes the stored reading only when the
//! effective reading or the findings changed.
use super::*;
use ovrcr_protocol::SettingsReport;
use std::time::SystemTime;

type FileStamp = Option<(u64, Option<SystemTime>)>;

/// What the last stat saw of the settings document and the instance identity
/// (whose stray `[quota]` table is a finding). `None` means missing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stamp(FileStamp, FileStamp);

impl Stamp {
    fn of(registry_path: &Path) -> Self {
        Self(
            file_stamp(&crate::settings::document_path(registry_path)),
            file_stamp(registry_path),
        )
    }
}

fn file_stamp(path: &Path) -> FileStamp {
    fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.len(), metadata.modified().ok()))
}

/// The Server's stored reading and the stamp it was read at.
pub(super) struct Watched {
    stamp: Stamp,
    pub(super) report: SettingsReport,
}

impl Watched {
    /// Stat before reading, so a write racing the read is seen next time.
    pub(super) fn load(registry_path: &Path) -> Self {
        let stamp = Stamp::of(registry_path);
        Self {
            stamp,
            report: crate::settings::load(registry_path),
        }
    }

    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            stamp: Stamp::default(),
            report: crate::settings::parse(Path::new("dashboard.toml"), ""),
        }
    }
}

/// Same reading apart from when it was read.
fn same_reading(a: &SettingsReport, b: &SettingsReport) -> bool {
    a.path == b.path && a.settings == b.settings && a.rows == b.rows && a.findings == b.findings
}

impl ServerState {
    /// Stat the files; reload when a stamp changed. Returns whether the stored
    /// reading changed. Callers publish.
    pub(super) fn refresh_settings(&self) -> bool {
        let stamp = Stamp::of(&self.registry_path);
        if self.settings.lock().unwrap().stamp == stamp {
            return false;
        }
        let report = crate::settings::load(&self.registry_path);
        let mut watched = self.settings.lock().unwrap();
        watched.stamp = stamp;
        if same_reading(&watched.report, &report) {
            return false;
        }
        watched.report = report;
        true
    }

    /// The watcher tick: republish only on a real change.
    pub(super) fn poll_settings(&self) -> bool {
        let changed = self.refresh_settings();
        if changed {
            self.publish_settings();
        }
        changed
    }

    pub(super) fn title_model(&self) -> Option<title::TitleModel> {
        self.settings
            .lock()
            .unwrap()
            .report
            .settings
            .title_model
            .as_deref()
            .and_then(title::TitleModel::parse)
    }

    pub(super) fn quota_settings(&self) -> ovrcr_protocol::QuotaSettings {
        self.settings.lock().unwrap().report.settings.quota.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_reloads_only_on_stamp_change_and_republishes_only_real_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = super::super::tests::test_state(None, None);
        Arc::get_mut(&mut state).unwrap().registry_path = dir.path().join("config.toml");
        let document = dir.path().join("dashboard.toml");

        // Missing document, missing identity: same stamp as the empty reading.
        assert!(!state.poll_settings());

        std::fs::write(&document, "title_model = 'pi/a'\n").unwrap();
        assert!(state.poll_settings());
        assert_eq!(state.title_model(), title::TitleModel::parse("pi/a"));

        // Unchanged file: no reload, no republish.
        assert!(!state.poll_settings());

        // Rewritten with the same effective reading: reload, but no republish.
        std::fs::write(&document, "title_model = 'pi/a' \n").unwrap();
        assert!(!state.poll_settings());

        // A stray [quota] in the identity is a finding, so it republishes.
        std::fs::write(dir.path().join("config.toml"), "[quota]\nenabled = true\n").unwrap();
        assert!(state.poll_settings());
        assert_eq!(state.settings.lock().unwrap().report.findings.len(), 1);
        assert!(!state.quota_settings().enabled);

        std::fs::remove_file(&document).unwrap();
        assert!(state.poll_settings());
        assert_eq!(state.title_model(), None);
    }
}
