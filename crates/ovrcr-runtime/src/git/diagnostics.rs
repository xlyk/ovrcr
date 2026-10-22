//! Opt-in, content-free receipts from the original Rust Git owner.
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_CALLS: u64 = 8192;
const MAX_ROW: usize = 1024;
const SEAL_RESERVE: usize = 512;
type Shared = Arc<Mutex<Journal>>;
static JOURNAL: OnceLock<Option<Shared>> = OnceLock::new();

struct Journal {
    file: Option<File>,
    bytes: usize,
    sequence: u64,
    calls: u64,
    pending: u64,
    failed: bool,
    write_failed: bool,
    sealed: bool,
    #[cfg(test)]
    fail_terminal_sync: bool,
}

impl Journal {
    fn unavailable() -> Self {
        Self {
            file: None,
            bytes: 0,
            sequence: 0,
            calls: 0,
            pending: 0,
            failed: true,
            write_failed: true,
            sealed: false,
            #[cfg(test)]
            fail_terminal_sync: false,
        }
    }

    fn open(directory: &Path, run: &str) -> io::Result<Self> {
        if !directory.is_absolute()
            || run.is_empty()
            || run.len() > 64
            || !run
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(io::Error::other("invalid Git diagnostic scope"));
        }
        let before = fs::symlink_metadata(directory)?;
        let uid = unsafe { libc::getuid() };
        if !before.is_dir() || before.uid() != uid || before.permissions().mode() & 0o777 != 0o700 {
            return Err(io::Error::other("Git diagnostic directory is not private"));
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join("git-owner.jsonl"))?;
        let actual = file.metadata()?;
        let after = fs::symlink_metadata(directory)?;
        if !actual.is_file()
            || actual.uid() != uid
            || actual.nlink() != 1
            || actual.permissions().mode() & 0o777 != 0o600
            || (before.dev(), before.ino()) != (after.dev(), after.ino())
        {
            return Err(io::Error::other("Git diagnostic file boundary changed"));
        }
        let mut journal = Self {
            file: Some(file),
            bytes: 0,
            sequence: 0,
            calls: 0,
            pending: 0,
            failed: false,
            write_failed: false,
            sealed: false,
            #[cfg(test)]
            fail_terminal_sync: false,
        };
        let owner = crate::task_runner::diagnostic_process_info(std::process::id() as i32)
            .ok()
            .flatten();
        if owner.is_none() {
            journal.failed = true;
        }
        journal.record(json!({"kind":"ready", "run":run, "owner":owner}), false);
        Ok(journal)
    }

    fn record(&mut self, mut row: Value, seal: bool) {
        if self.write_failed && !seal {
            return;
        }
        self.sequence += 1;
        row["schema"] = json!(1);
        row["seq"] = json!(self.sequence);
        let Ok(mut data) = serde_json::to_vec(&row) else {
            self.failed = true;
            self.write_failed = true;
            return;
        };
        data.push(b'\n');
        let limit = MAX_BYTES - if seal { 0 } else { SEAL_RESERVE };
        if data.len() > MAX_ROW || self.bytes + data.len() > limit {
            self.failed = true;
            self.write_failed = true;
            return;
        }
        self.bytes += data.len();
        #[cfg(test)]
        let fail_terminal_sync = seal && self.fail_terminal_sync;
        if self.file.as_mut().is_none_or(|file| {
            file.write_all(&data)
                .and_then(|()| {
                    #[cfg(test)]
                    if fail_terminal_sync {
                        return Err(io::Error::from_raw_os_error(libc::EIO));
                    }
                    file.sync_all()
                })
                .is_err()
        }) {
            self.failed = true;
            self.write_failed = true;
        }
    }

    fn begin(&mut self) -> Option<u64> {
        if self.sealed || self.calls == MAX_CALLS {
            self.failed = true;
            self.record(json!({"kind":"gap"}), false);
            return None;
        }
        self.calls += 1;
        self.pending += 1;
        self.record(json!({"kind":"begin", "construction":self.calls}), false);
        Some(self.calls)
    }

    fn spawned(&mut self, id: u64, pid: u32, identity: Option<Value>) {
        let valid = identity.as_ref().is_some_and(|row| {
            row["pid"].as_u64() == Some(u64::from(pid))
                && row["parent"].as_u64() == Some(u64::from(std::process::id()))
                && row["group"].as_u64().is_some_and(|g| g > 1)
                && row["birth"].as_array().is_some_and(|b| {
                    b.len() == 2
                        && b[0].as_u64().is_some_and(|n| n > 0)
                        && b[1].as_u64().is_some_and(|n| n < 1_000_000)
                })
        });
        if !valid {
            self.failed = true;
        }
        self.record(
            json!({"kind":"spawn", "construction":id, "child_pid":pid,
                          "identity":if valid { identity } else { None }}),
            false,
        );
    }

    fn spawn_error(&mut self, id: u64, error: &io::Error) {
        self.pending -= 1;
        self.failed = true;
        self.record(
            json!({"kind":"spawn_error", "construction":id, "errno":error.raw_os_error()}),
            false,
        );
    }

    fn completed(&mut self, id: u64, result: Result<&ExitStatus, &io::Error>, pipes: bool) {
        self.pending -= 1;
        let row = match result {
            Ok(status) => {
                if status.code().is_some() == status.signal().is_some() {
                    self.failed = true;
                }
                json!({"kind":"wait", "construction":id, "original_wait":true,
                       "code":status.code(), "signal":status.signal(), "pipes_drained":pipes})
            }
            Err(error) => {
                self.failed = true;
                json!({"kind":"wait", "construction":id, "original_wait":false,
                       "errno":error.raw_os_error(), "pipes_drained":false})
            }
        };
        self.record(row, false);
    }

    fn finish(&mut self) -> bool {
        if self.sealed {
            self.failed = true;
            return false;
        }
        self.sealed = true;
        let row = json!({"kind":"seal", "failed":self.failed || self.pending != 0,
                         "calls":self.calls, "pending":self.pending});
        self.record(row, true);
        !self.failed && !self.write_failed && self.pending == 0
    }
}

fn locked(shared: &Shared) -> std::sync::MutexGuard<'_, Journal> {
    match shared.lock() {
        Ok(journal) => journal,
        Err(poisoned) => {
            let mut journal = poisoned.into_inner();
            journal.failed = true;
            journal
        }
    }
}

fn configured() -> Option<Shared> {
    #[cfg(test)]
    if let Some(journal) = TEST_JOURNAL.with(|slot| slot.borrow().clone()) {
        return Some(journal);
    }
    JOURNAL
        .get_or_init(|| {
            let directory = std::env::var_os("OVRCR_GIT_DIAGNOSTICS_DIR")?;
            let journal = std::env::var("OVRCR_GIT_DIAGNOSTICS_RUN")
                .ok()
                .and_then(|run| Journal::open(Path::new(&directory), &run).ok())
                .unwrap_or_else(Journal::unavailable);
            Some(Arc::new(Mutex::new(journal)))
        })
        .clone()
}

fn observed_spawn(
    command: &mut Command,
    journal: &Shared,
    inspect: &dyn Fn(i32) -> anyhow::Result<Option<Value>>,
) -> io::Result<(Child, Option<u64>)> {
    let id = locked(journal).begin();
    match command.spawn() {
        Ok(child) => {
            if let Some(id) = id {
                let identity = inspect(child.id() as i32).ok().flatten();
                locked(journal).spawned(id, child.id(), identity);
            }
            Ok((child, id))
        }
        Err(error) => {
            if let Some(id) = id {
                locked(journal).spawn_error(id, &error);
            }
            Err(error)
        }
    }
}

fn output_observed(
    command: &mut Command,
    journal: Option<Shared>,
    inspect: &dyn Fn(i32) -> anyhow::Result<Option<Value>>,
) -> io::Result<Output> {
    let Some(journal) = journal else {
        return command.output();
    };
    // All four private Git output callers use Command.output's default stdio.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, id) = observed_spawn(command, &journal, inspect)?;
    let result = child.wait_with_output();
    if let Some(id) = id {
        locked(&journal).completed(id, result.as_ref().map(|r| &r.status), result.is_ok());
    }
    result
}

pub(super) fn output(command: &mut Command) -> io::Result<Output> {
    output_observed(
        command,
        configured(),
        &crate::task_runner::diagnostic_process_info,
    )
}

pub(super) fn status(command: &mut Command) -> io::Result<ExitStatus> {
    let Some(journal) = configured() else {
        return command.status();
    };
    let (mut child, id) = observed_spawn(
        command,
        &journal,
        &crate::task_runner::diagnostic_process_info,
    )?;
    let result = child.wait();
    if let Some(id) = id {
        locked(&journal).completed(id, result.as_ref(), false);
    }
    result
}

pub(super) fn finish() -> io::Result<()> {
    if configured().is_some_and(|journal| !locked(&journal).finish()) {
        return Err(io::Error::other("Git acceptance diagnostics are unhealthy"));
    }
    Ok(())
}

#[cfg(test)]
thread_local! { static TEST_JOURNAL: std::cell::RefCell<Option<Shared>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
mod tests;
