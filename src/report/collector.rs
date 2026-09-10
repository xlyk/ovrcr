//! Exact-source helper; no provider admission or runtime publication occurs here.
use super::claude_metrics::ClaudeUsageAccumulator;
use anyhow::{Context, Result, bail};
use ovrcr_protocol::UsageTotals;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const IPC_LIMIT: usize = 65_536;
const READ_LIMIT: usize = 256 * 1024;
const RECORD_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct CollectorSource {
    pub path: PathBuf,
    pub conversation: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectorSnapshot {
    pub usage: UsageTotals,
    pub source_revision: Option<String>,
    pub diagnostic: Option<String>,
    pub caught_up: bool,
    pub rebuilding: bool,
    pub retained_identities: usize,
    pub retained_bytes: usize,
}

#[derive(Serialize, Deserialize)]
enum HelperRequest {
    Start(CollectorSource),
    Read,
}

pub struct CollectorController {
    // Do not reap this PGID anchor before signaling its group during cancellation.
    child: Option<Child>,
    input: ChildStdin,
    output: ChildStdout,
    write: Vec<u8>,
    written: usize,
    response: Vec<u8>,
    in_flight: bool,
    stopped: bool,
    last_usage: Option<UsageTotals>,
    watermark: [Option<u64>; 5],
    awaiting_rebuild: bool,
    cleanup_deadline: Option<Instant>,
}

impl CollectorController {
    pub fn spawn(executable: &Path, source: CollectorSource) -> Result<Self> {
        validate_source(&source)?;
        let write = encode(&HelperRequest::Start(source))?;
        let mut child = Command::new(executable)
            .arg("__agent-collector")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|_| anyhow::anyhow!("collector spawn unavailable"))?;
        let input = child.stdin.take().context("collector input unavailable")?;
        let output = child
            .stdout
            .take()
            .context("collector output unavailable")?;
        let mut controller = Self {
            child: Some(child),
            input,
            output,
            write,
            written: 0,
            response: Vec::new(),
            in_flight: true,
            stopped: false,
            last_usage: None,
            watermark: [None; 5],
            awaiting_rebuild: false,
            cleanup_deadline: None,
        };
        nonblocking(controller.input.as_raw_fd())?;
        nonblocking(controller.output.as_raw_fd())?;
        // Keep all initialization failure cleanup under the same owned controller.
        controller.flush()?;
        Ok(controller)
    }

    /// One nonblocking request/response step. A returned EOF observation is never
    /// proof of provider completion. Calling again requests another bounded scan.
    pub fn advance(&mut self) -> Result<Option<CollectorSnapshot>> {
        if self.stopped {
            bail!("collector is stopped");
        }
        if !self.in_flight {
            self.write = encode(&HelperRequest::Read)?;
            self.written = 0;
            self.in_flight = true;
        }
        self.flush()?;
        let mut chunk = [0u8; 4096];
        loop {
            match self.output.read(&mut chunk) {
                Ok(0) => bail!("collector disconnected"),
                Ok(n) => self.response.extend_from_slice(&chunk[..n]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => bail!("collector response unavailable"),
            }
            if self.response.len() > IPC_LIMIT + 4 {
                bail!("collector response exceeds limit");
            }
            if self.response.len() < 4 {
                continue;
            }
            let length = u32::from_be_bytes(self.response[..4].try_into().unwrap()) as usize;
            if length > IPC_LIMIT {
                bail!("collector response exceeds limit");
            }
            if self.response.len() < length + 4 {
                continue;
            }
            if self.response.len() != length + 4 {
                bail!("unexpected collector response");
            }
            let mut snapshot: CollectorSnapshot = serde_json::from_slice(&self.response[4..])
                .map_err(|_| anyhow::anyhow!("invalid collector response"))?;
            self.response.clear();
            self.in_flight = false;
            self.protect_watermark(&mut snapshot);
            return Ok(Some(snapshot));
        }
    }

    fn flush(&mut self) -> Result<()> {
        while self.written < self.write.len() {
            match self.input.write(&self.write[self.written..]) {
                Ok(0) => bail!("collector request unavailable"),
                Ok(n) => self.written += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => bail!("collector request unavailable"),
            }
        }
        Ok(())
    }

    fn protect_watermark(&mut self, snapshot: &mut CollectorSnapshot) {
        self.awaiting_rebuild |= snapshot.rebuilding;
        let values = usage_values(&snapshot.usage);
        let regressed = self.watermark.iter().zip(values).any(|(old, new)| {
            old.is_some_and(|old| {
                new.is_some_and(|new| new < old) || (self.awaiting_rebuild && new.is_none())
            })
        });
        let unavailable = matches!(
            snapshot.diagnostic.as_deref(),
            Some("source_unavailable" | "read_unavailable")
        );
        if snapshot.rebuilding || regressed || unavailable {
            if let Some(previous) = &self.last_usage {
                snapshot.usage = previous.clone();
            }
            if snapshot.diagnostic.is_none() {
                if snapshot.rebuilding {
                    snapshot.diagnostic = Some("source_rebuilding".into());
                } else if regressed {
                    snapshot.diagnostic = Some("usage_regression".into());
                }
            }
            return;
        }
        self.awaiting_rebuild = false;
        for (old, new) in self.watermark.iter_mut().zip(values) {
            if new.is_some() {
                *old = new;
            }
        }
        self.last_usage = Some(snapshot.usage.clone());
    }

    /// Kill the owned group while its unreaped leader still anchors the PGID.
    /// Reaping is nonblocking and consumes only the caller's remaining deadline.
    pub fn cancel(&mut self, deadline: Instant) -> Result<()> {
        self.stopped = true;
        self.cleanup_deadline = Some(deadline);
        let Some(child) = &mut self.child else {
            return Ok(());
        };
        let result = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
        if result != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            // macOS may report EPERM for a group containing only its zombie
            // leader. Reap our child, then verify absence without signaling an
            // unanchored PGID. An exited leader alone does not prove cleanup.
            if Instant::now() >= deadline {
                bail!("collector cleanup deadline");
            }
            let pgid = child.id() as i32;
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.child = None;
                if unsafe { libc::kill(-pgid, 0) } == -1
                    && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                {
                    return Ok(());
                }
            }
            bail!("collector group cleanup unavailable");
        }
        loop {
            if Instant::now() >= deadline {
                bail!("collector cleanup deadline");
            }
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    return Ok(());
                }
                Ok(None) => {}
                Err(_) => bail!("collector reap unavailable"),
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("collector cleanup deadline");
            }
            std::thread::sleep(remaining.min(Duration::from_millis(1)));
        }
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }
}

impl Drop for CollectorController {
    fn drop(&mut self) {
        let deadline = self
            .cleanup_deadline
            .unwrap_or_else(|| Instant::now() + Duration::from_millis(100));
        let _ = self.cancel(deadline);
        if let Some(mut child) = self.child.take() {
            // Group signaling was attempted with an unreaped ownership anchor.
            // Transfer the remaining wait instead of dropping that ownership or
            // extending the caller's deadline. Dropping JoinHandle detaches it.
            let _ = std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

fn nonblocking(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        bail!("collector channel unavailable");
    }
    Ok(())
}

fn usage_values(usage: &UsageTotals) -> [Option<u64>; 5] {
    [
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_tokens,
        usage.cache_write_tokens,
        usage.reasoning_output_tokens,
    ]
}

fn validate_source(source: &CollectorSource) -> Result<()> {
    if !source.path.is_absolute() {
        bail!("collector source must be absolute");
    }
    ovrcr_protocol::validate_agent_id(&source.conversation)?;
    Ok(())
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let body =
        serde_json::to_vec(value).map_err(|_| anyhow::anyhow!("invalid collector message"))?;
    if body.len() > IPC_LIMIT {
        bail!("collector message exceeds limit");
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

fn receive<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header[..1]) {
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(_) => bail!("collector request unavailable"),
        Ok(()) => {}
    }
    reader
        .read_exact(&mut header[1..])
        .map_err(|_| anyhow::anyhow!("incomplete collector request"))?;
    let length = u32::from_be_bytes(header) as usize;
    if length > IPC_LIMIT {
        bail!("collector request exceeds limit");
    }
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| anyhow::anyhow!("incomplete collector request"))?;
    Ok(Some(serde_json::from_slice(&bytes).map_err(|_| {
        anyhow::anyhow!("invalid collector request")
    })?))
}

/// Hidden CLI entry. It has no socket, lease or provider credentials. Only this
/// disposable process performs potentially blocking filesystem operations.
pub fn run_helper() -> Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let Some(HelperRequest::Start(source)) = receive(&mut input)? else {
        bail!("collector requires source configuration");
    };
    validate_source(&source)?;
    let mut reader = TranscriptReader::new(source);
    loop {
        output
            .write_all(&encode(&reader.step())?)
            .map_err(|_| anyhow::anyhow!("collector response unavailable"))?;
        output
            .flush()
            .map_err(|_| anyhow::anyhow!("collector response unavailable"))?;
        match receive(&mut input)? {
            Some(HelperRequest::Read) => {}
            None => return Ok(()),
            _ => bail!("collector source cannot change in place"),
        }
    }
}

struct TranscriptReader {
    source: CollectorSource,
    file: Option<File>,
    metadata: Option<Metadata>,
    offset: u64,
    pending: Vec<u8>,
    accumulator: ClaudeUsageAccumulator,
    frozen: Option<&'static str>,
    rebuilding: bool,
}

impl TranscriptReader {
    fn new(source: CollectorSource) -> Self {
        Self {
            source,
            file: None,
            metadata: None,
            offset: 0,
            pending: Vec::new(),
            accumulator: ClaudeUsageAccumulator::default(),
            frozen: None,
            rebuilding: false,
        }
    }

    fn refresh(&mut self) -> io::Result<bool> {
        let metadata = std::fs::metadata(&self.source.path)?;
        if !metadata.is_file() {
            return Err(io::Error::other("nonregular source"));
        }
        let replace = self.metadata.as_ref().is_none_or(|old| {
            old.dev() != metadata.dev()
                || old.ino() != metadata.ino()
                || metadata.len() < old.len()
                || metadata.len() < self.offset
                || (metadata.len() == old.len()
                    && (metadata.mtime(), metadata.mtime_nsec()) != (old.mtime(), old.mtime_nsec()))
        });
        let rebuilding_started = replace && self.metadata.is_some();
        if replace {
            // O_NONBLOCK avoids waiting on a FIFO substituted between stat/open.
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&self.source.path)?;
            let opened = file.metadata()?;
            if !opened.is_file() || opened.dev() != metadata.dev() || opened.ino() != metadata.ino()
            {
                return Err(io::Error::other("source changed during open"));
            }
            self.rebuilding = self.metadata.is_some();
            self.file = Some(file);
            self.offset = 0;
            self.pending = Vec::new();
            self.accumulator = ClaudeUsageAccumulator::default();
            self.frozen = None;
        }
        self.metadata = Some(metadata);
        Ok(rebuilding_started)
    }

    fn step(&mut self) -> CollectorSnapshot {
        let rebuilding_started = match self.refresh() {
            Ok(started) => started,
            Err(_) => return self.snapshot(false, Some("source_unavailable")),
        };
        if self.frozen.is_some() {
            return self.snapshot(false, self.frozen);
        }
        let mut bytes = vec![0u8; READ_LIMIT];
        let read = match self.file.as_mut().unwrap().read(&mut bytes) {
            Ok(read) => read,
            Err(_) => return self.snapshot(false, Some("read_unavailable")),
        };
        self.offset += read as u64;
        for part in bytes[..read].split_inclusive(|&byte| byte == b'\n') {
            let complete = part.last() == Some(&b'\n');
            let body = if complete {
                &part[..part.len() - 1]
            } else {
                part
            };
            if self.pending.len() + body.len() > RECORD_LIMIT {
                self.frozen = Some("record_limit");
                break;
            }
            self.pending.extend_from_slice(body);
            if complete {
                if !self.pending.iter().all(u8::is_ascii_whitespace) {
                    let result = serde_json::from_slice::<serde_json::Value>(&self.pending);
                    match result {
                        Ok(record) => self.apply(&record),
                        Err(_) => self.frozen = Some("invalid_usage_record"),
                    }
                }
                self.pending.clear();
                if self.frozen.is_some() {
                    break;
                }
            }
        }
        if read == 0 && !rebuilding_started {
            self.rebuilding = false;
        }
        self.snapshot(read == 0, self.frozen)
    }

    fn apply(&mut self, record: &serde_json::Value) {
        if !record.is_object() || !record.get("type").is_some_and(serde_json::Value::is_string) {
            self.frozen = Some("unrecognized_usage_record");
            return;
        }
        if record.get("type").and_then(serde_json::Value::as_str) != Some("assistant") {
            return;
        }
        if record.get("sessionId").and_then(serde_json::Value::as_str)
            != Some(&self.source.conversation)
        {
            self.frozen = Some("foreign_usage_record");
            return;
        }
        match record
            .get("isSidechain")
            .and_then(serde_json::Value::as_bool)
        {
            Some(false) => {}
            Some(true) => return, // Explicitly excluded from this partial subset.
            None => {
                self.frozen = Some("unrecognized_usage_record");
                return;
            }
        }
        if self.accumulator.apply_unique_record(record).is_err() {
            self.frozen = Some("invalid_usage_record");
        } else {
            self.frozen = self.accumulator.diagnostic();
        }
    }

    fn snapshot(&self, caught_up: bool, diagnostic: Option<&str>) -> CollectorSnapshot {
        CollectorSnapshot {
            usage: self.accumulator.snapshot(),
            source_revision: None,
            diagnostic: diagnostic.map(str::to_owned),
            caught_up,
            rebuilding: self.rebuilding,
            retained_identities: self.accumulator.retained_identities(),
            retained_bytes: self.accumulator.retained_bytes(),
        }
    }
}
