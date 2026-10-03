//! The Server's settings watcher: a 2-second stat and change-only republish.
//!
//! [`run`] is its own thread. It reloads only when the document's (or the
//! instance identity's) length or mtime changed, and replaces and republishes
//! the stored reading only when the effective reading or the findings changed.
//! The title worker does not tick it.
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
        self.reload_settings(false)
    }

    /// `force` reloads even when the stamp looks unchanged, as after a write
    /// that kept the length within the file system's mtime granularity.
    fn reload_settings(&self, force: bool) -> bool {
        let stamp = Stamp::of(&self.registry_path);
        if !force && self.settings.lock().unwrap().stamp == stamp {
            return false;
        }
        let report = crate::settings::load(&self.registry_path);
        let mut watched = self.settings.lock().unwrap();
        watched.stamp = stamp;
        if same_reading(&watched.report, &report) {
            return false;
        }
        let findings = report.findings.len();
        watched.report = report;
        drop(watched);
        quota::sync_consent(self);
        self.record_event(ovrcr_protocol::EventComponent::Settings, None, "reloaded");
        self.record_event(
            ovrcr_protocol::EventComponent::Settings,
            None,
            format!("findings: {findings}"),
        );
        true
    }

    /// The watcher tick: republish only on a real change.
    pub(super) fn poll_settings(&self) -> bool {
        self.reload_and_publish(false)
    }

    /// Republish the settings reading, and the quota rows that follow
    /// `quota.enabled`, when a reload changed them.
    fn reload_and_publish(&self, force: bool) -> bool {
        let quotas = self.quotas.lock().unwrap().clone();
        let changed = self.reload_settings(force);
        if changed {
            self.publish_settings();
            let now = self.quotas.lock().unwrap().clone();
            if now != quotas {
                // Even the default: turning quota off returns to it.
                self.send_quotas(now);
            }
        }
        changed
    }

    /// Apply one `SetSetting` edit to the latest document, holding the
    /// settings lock so Server writes never interleave, then reload at once
    /// and republish on a real change. External editors stay unlocked.
    pub(super) fn set_setting(&self, path: &str, value: Option<&str>) -> anyhow::Result<()> {
        {
            let _writing = self.settings.lock().unwrap();
            if let Err(error) = crate::settings::set(
                &crate::settings::document_path(&self.registry_path),
                path,
                value,
            ) {
                drop(_writing);
                self.record_event(
                    ovrcr_protocol::EventComponent::Settings,
                    Some(path.to_owned()),
                    "refused",
                );
                return Err(error);
            }
        }
        self.record_event(
            ovrcr_protocol::EventComponent::Settings,
            Some(path.to_owned()),
            "wrote",
        );
        self.reload_and_publish(true);
        Ok(())
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

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Stat and reload on a dedicated thread, every two seconds. A title call
/// must not delay this; the title worker does not call it.
pub(super) fn run(state: Arc<ServerState>) {
    let mut last = Instant::now() - POLL_INTERVAL;
    while !state.shutdown.load(Ordering::Acquire) {
        thread::park_timeout(Duration::from_millis(200));
        if state.shutdown.load(Ordering::Acquire) {
            break;
        }
        if last.elapsed() >= POLL_INTERVAL {
            last = Instant::now();
            state.poll_settings();
        }
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

    #[test]
    fn set_setting_request_writes_the_document_and_reloads_at_once() {
        use ovrcr_protocol::{ErrorCode, Request, Response};
        let dir = tempfile::tempdir().unwrap();
        let mut state = super::super::tests::test_state(None, None);
        Arc::get_mut(&mut state).unwrap().registry_path = dir.path().join("config.toml");
        let document = dir.path().join("dashboard.toml");
        std::fs::write(&document, "# mine\nbranch_prefix = \"a/\" # prefix\n").unwrap();
        assert!(state.poll_settings());
        let set = |path: &str, value: Option<&str>| {
            super::super::connections::handle_request_with_id(
                &state,
                &mut ClientRole::Control,
                Request::SetSetting {
                    path: path.into(),
                    value: value.map(str::to_owned),
                },
                1,
                None,
            )
        };

        // Same length as before: the reading follows without waiting for a
        // stamp change or the watcher's next tick.
        assert_eq!(set("branch_prefix", Some("\"b/\"")), Response::Ok);
        assert_eq!(
            std::fs::read_to_string(&document).unwrap(),
            "# mine\nbranch_prefix = \"b/\" # prefix\n"
        );
        let reading = |state: &ServerState| state.settings.lock().unwrap().report.clone();
        assert_eq!(reading(&state).settings.branch_prefix, "b/");

        assert_eq!(set("quota.enabled", Some("true")), Response::Ok);
        assert!(state.quota_settings().enabled);

        for (path, value, message) in [
            ("ready_sound", Some("\"yes\""), "expected a boolean"),
            ("ready_sund", Some("true"), "no setting named ready_sund"),
            ("picker_roots[3]", None, "no such element"),
        ] {
            let before = std::fs::read_to_string(&document).unwrap();
            let Response::Error {
                code,
                message: error,
            } = set(path, value)
            else {
                panic!("{path} accepted");
            };
            assert_eq!(code, ErrorCode::InvalidRequest);
            assert!(error.contains(message), "{path}: {error}");
            assert_eq!(std::fs::read_to_string(&document).unwrap(), before);
        }

        assert_eq!(set("quota.enabled", None), Response::Ok);
        assert!(!state.quota_settings().enabled);
        assert_eq!(set("branch_prefix", None), Response::Ok);
        assert_eq!(std::fs::read_to_string(&document).unwrap(), "# mine\n");
        assert_eq!(reading(&state).settings.branch_prefix, "feature/");
    }

    #[test]
    fn settings_reload_on_its_own_thread_without_the_title_worker() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = super::super::tests::test_state(None, None);
        Arc::get_mut(&mut state).unwrap().registry_path = dir.path().join("config.toml");
        let document = dir.path().join("dashboard.toml");
        std::fs::write(&document, "title_model = 'pi/own-thread'\n").unwrap();
        assert_eq!(state.title_model(), None);

        // No title worker is started. A reload that only happens inside that
        // worker's poll never observes this document.
        let worker = Arc::clone(&state);
        let thread = thread::Builder::new()
            .name("ovrcr-settings-watch".into())
            .spawn(move || run(worker))
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while state.title_model() != title::TitleModel::parse("pi/own-thread") {
            assert!(
                Instant::now() < deadline,
                "settings did not reload without the title worker"
            );
            thread::sleep(Duration::from_millis(50));
        }

        state.shutdown.store(true, Ordering::Release);
        thread.thread().unpark();
        thread.join().unwrap();
    }

    #[test]
    fn settings_events_record_a_reload_with_findings_and_a_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = super::super::tests::test_state(None, None);
        Arc::get_mut(&mut state).unwrap().registry_path = dir.path().join("config.toml");
        let document = dir.path().join("dashboard.toml");
        let secret = "SECRET_VALUE_9f3a";
        std::fs::write(
            &document,
            "not_a_setting = true
",
        )
        .unwrap();
        assert!(state.poll_settings());
        let set = super::super::connections::handle_request_with_id(
            &state,
            &mut ClientRole::Control,
            Request::SetSetting {
                path: "ready_sound".into(),
                value: Some(format!("\"{secret}\"")),
            },
            1,
            None,
        );
        assert!(matches!(set, Response::Error { .. }));
        assert_eq!(
            super::super::connections::handle_request_with_id(
                &state,
                &mut ClientRole::Control,
                Request::SetSetting {
                    path: "ready_sound".into(),
                    value: Some("true".into()),
                },
                2,
                None,
            ),
            Response::Ok
        );
        let events = state.event_snapshot();
        let messages: Vec<_> = events.iter().map(|event| event.message.clone()).collect();
        assert!(
            messages.iter().any(|message| message == "reloaded"),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message == "findings: 1"),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message == "refused"),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message == "wrote"),
            "{messages:?}"
        );
        let rendered = format!("{events:?}");
        assert!(!rendered.contains(secret));
    }
}
