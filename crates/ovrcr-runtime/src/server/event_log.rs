//! Bounded ring and `events.jsonl` inside the instance directory.
//!
//! The ring is what the Dashboard and `ovrcr events` read. The file is for
//! people and other tools: one JSON event per line, mode 0600, capped at 5 MB
//! with a single rotation to `events.jsonl.1`. Lines already written are never
//! rewritten, and a rotation never splits an event across the two files.
use super::*;
use ovrcr_protocol::{Event, EventComponent};
use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

pub(super) const RING_LIMIT: usize = 2000;
pub(super) const FILE_CAP_BYTES: u64 = 5 * 1024 * 1024;

/// `events.jsonl` inside the instance directory, next to `dashboard.toml`
/// when that file is not redirected.
pub(super) fn events_path(home: &Path) -> PathBuf {
    crate::config::events_log_path(home)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("events.jsonl"))
        .to_os_string();
    name.push(".1");
    path.with_file_name(name)
}

fn one_line(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

pub(super) struct Log {
    ring: VecDeque<Event>,
    path: Option<PathBuf>,
    followers: Vec<mpsc::Sender<Event>>,
}

impl Log {
    #[cfg(test)]
    pub(super) fn memory() -> Self {
        Self {
            ring: VecDeque::new(),
            path: None,
            followers: Vec::new(),
        }
    }

    /// Remember where to append. The file is created on the first event.
    pub(super) fn open(path: PathBuf) -> Self {
        Self {
            ring: VecDeque::new(),
            path: Some(path),
            followers: Vec::new(),
        }
    }

    pub(super) fn push(
        &mut self,
        component: EventComponent,
        subject: Option<String>,
        message: String,
    ) -> Event {
        let event = Event {
            time_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            component,
            subject: subject.map(|subject| one_line(&subject)),
            message: one_line(&message),
        };
        if self.ring.len() == RING_LIMIT {
            self.ring.pop_front();
        }
        self.ring.push_back(event.clone());
        if let Some(path) = &self.path
            && let Err(error) = append_line(path, &event)
        {
            eprintln!("event log: {error}");
        }
        self.followers
            .retain(|follower| follower.send(event.clone()).is_ok());
        event
    }

    /// Oldest first, newest last.
    pub(super) fn snapshot(&self) -> Vec<Event> {
        self.ring.iter().cloned().collect()
    }

    /// Snapshot and a receiver for events recorded after it. Holding this
    /// lock across both steps means a record cannot fall in the gap.
    pub(super) fn subscribe(&mut self) -> (Vec<Event>, mpsc::Receiver<Event>) {
        let (sender, receiver) = mpsc::channel();
        let snapshot = self.snapshot();
        self.followers.push(sender);
        (snapshot, receiver)
    }
}

fn append_line(path: &Path, event: &Event) -> io::Result<()> {
    let line = serde_json::to_string(event)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let adding = line.len() as u64 + 1;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let len = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    // An empty file keeps a single oversized event whole. Otherwise rotate
    // before the write so the new line is not split across the cap.
    if len > 0 && len.saturating_add(adding) > FILE_CAP_BYTES {
        rotate(path)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn rotate(path: &Path) -> io::Result<()> {
    let backup = backup_path(path);
    let _ = fs::remove_file(&backup);
    fs::rename(path, backup)
}

impl ServerState {
    pub(super) fn record_event(
        &self,
        component: EventComponent,
        subject: Option<String>,
        message: impl Into<String>,
    ) {
        let event = self
            .event_log
            .lock()
            .unwrap()
            .push(component, subject, message.into());
        self.dashboard
            .try_send(ServerMessage::Event(ServerEvent::Recorded(event)));
    }

    pub(super) fn event_snapshot(&self) -> Vec<Event> {
        self.event_log.lock().unwrap().snapshot()
    }

    pub(super) fn subscribe_events(&self) -> (Vec<Event>, mpsc::Receiver<Event>) {
        self.event_log.lock().unwrap().subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn messages(log: &Log) -> Vec<String> {
        log.snapshot()
            .into_iter()
            .map(|event| event.message)
            .collect()
    }

    #[test]
    fn ring_drops_the_oldest_at_2000() {
        let mut log = Log::memory();
        for index in 0..=RING_LIMIT {
            log.push(EventComponent::Settings, None, format!("n{index}"));
        }
        let kept = messages(&log);
        assert_eq!(kept.len(), RING_LIMIT);
        assert_eq!(kept.first().map(String::as_str), Some("n1"));
        assert_eq!(
            kept.last().map(String::as_str),
            Some(format!("n{RING_LIMIT}").as_str())
        );
        assert!(!kept.iter().any(|message| message == "n0"));
    }

    #[test]
    fn file_rotates_once_at_the_cap_without_splitting_an_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let mut log = Log::open(path.clone());
        let secret = "EXCERPT_SECRET_9f3a secret-body account@example.invalid";
        // Just under the cap, so the next whole event has to rotate.
        let filler = "y".repeat((FILE_CAP_BYTES as usize) - 120);
        log.push(EventComponent::Titles, Some("4".into()), filler.clone());
        let first_len = fs::metadata(&path).unwrap().len();
        assert!(first_len <= FILE_CAP_BYTES, "{first_len}");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        log.push(
            EventComponent::Quota,
            Some("Codex".into()),
            "Checking -> Unavailable: HTTP 503".into(),
        );
        let backup = dir.path().join("events.jsonl.1");
        assert!(backup.is_file(), "rotation did not keep the previous lines");
        assert!(
            !dir.path().join("events.jsonl.2").exists(),
            "rotation keeps a single backup"
        );
        let current = fs::read_to_string(&path).unwrap();
        let previous = fs::read_to_string(&backup).unwrap();
        for (name, text) in [("current", &current), ("backup", &previous)] {
            for line in text.lines() {
                let parsed: Value = serde_json::from_str(line).unwrap_or_else(|error| {
                    panic!("{name} split or lost an event: {error}: {line}")
                });
                assert!(parsed.get("message").is_some(), "{name}");
            }
        }
        assert!(current.contains("HTTP 503"));
        assert!(!current.contains(&filler[..32]));
        assert!(previous.contains(&filler[..32]));
        assert!(!current.contains(secret) && !previous.contains(secret));
        // A second overflow replaces the one backup and still does not create .2.
        let filler = "z".repeat((FILE_CAP_BYTES as usize) - 80);
        log.push(EventComponent::Settings, None, filler);
        log.push(
            EventComponent::Settings,
            None,
            "after-second-rotation".into(),
        );
        assert!(!dir.path().join("events.jsonl.2").exists());
        let current = fs::read_to_string(&path).unwrap();
        let previous = fs::read_to_string(&backup).unwrap();
        assert!(current.contains("after-second-rotation"));
        assert!(
            !current.contains("HTTP 503"),
            "the single backup replaced the older file"
        );
        assert!(previous.contains(&"z".repeat(32)));
        for line in current.lines().chain(previous.lines()) {
            serde_json::from_str::<Event>(line).expect("whole event");
        }
    }

    #[test]
    fn json_lines_round_trip_the_ring_they_were_written_from() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let path = events_path(home);
        assert_eq!(path, home.join("events.jsonl"));
        let mut log = Log::open(path.clone());
        log.push(
            EventComponent::Titles,
            Some("7".into()),
            "title applied".into(),
        );
        let from_file: Vec<Event> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(from_file, log.snapshot());
        let again: Vec<Event> =
            serde_json::from_str(&serde_json::to_string(&from_file).unwrap()).unwrap();
        assert_eq!(again, from_file);
    }
}
