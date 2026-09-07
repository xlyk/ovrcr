//! Blocking Pi RPC supervisor. The lifetime lock covers both execution and cleanup.
use crate::tasks::{self, Run, RunStatus};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const LOG_CHUNK: usize = 64 * 1024;

pub fn read_log(run_dir: &Path, offset: u64, max_bytes: usize) -> Result<(Vec<u8>, u64)> {
    let mut file = match File::open(run_dir.join("events.jsonl")) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), offset)),
        Err(e) => return Err(e.into()),
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; max_bytes.min(LOG_CHUNK)];
    let n = file.read(&mut bytes)?;
    bytes.truncate(n);
    Ok((bytes, offset + n as u64))
}

pub fn supervise(run_dir: &Path, pi_executable: &Path) -> Result<()> {
    let mut lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(run_dir.join("run.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!("run already has a supervisor");
    }
    lock.set_len(0)?;
    writeln!(lock, "{}", std::process::id())?;
    let mut run = tasks::read_run(run_dir)?;
    if run.status.is_terminal() {
        bail!("refusing to replay a completed run");
    }
    let result = execute(run_dir, pi_executable, &mut run);
    if let Err(error) = result {
        run.status = RunStatus::Failed;
        run.error = Some(format!("{error:#}"));
    }
    run.finished_at = Some(tasks::now());
    tasks::write_run(run_dir, &run)?;
    Ok(())
}

fn execute(run_dir: &Path, pi: &Path, run: &mut Run) -> Result<()> {
    let started = Instant::now();
    let timeout = Duration::from_secs(run.spec.timeout_seconds);
    let directory = run
        .directory
        .as_ref()
        .context("run directory was not prepared")?;
    let (provider, model) = run
        .spec
        .model
        .split_once('/')
        .context("model must be provider/model")?;
    if provider.is_empty() || model.is_empty() {
        bail!("model must be provider/model");
    }
    let mut events = OpenOptions::new()
        .create(true)
        .append(true)
        .open(run_dir.join("events.jsonl"))?;
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(run_dir.join("stderr.log"))?;
    run.pi_version = version(pi, run_dir).ok();
    let session_file = run_dir.join("session.jsonl");
    if session_file.exists() {
        bail!("refusing to reuse an existing Pi session");
    }
    let extension = run_dir.join("pi-task-extension.mjs");
    fs::write(
        &extension,
        include_str!("../../../src/pi-task-extension.mjs"),
    )?;
    let mut child = Command::new(pi)
        .current_dir(directory)
        .args([
            "--mode",
            "rpc",
            "--provider",
            provider,
            "--model",
            model,
            "--thinking",
            &run.spec.thinking,
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-approve",
            "--offline",
            "--session",
        ])
        .arg(&session_file)
        .arg("--extension")
        .arg(&extension)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .process_group(0)
        .spawn()
        .with_context(|| format!("start Pi {}", pi.display()))?;
    let mut input = child.stdin.take();
    let mut output = child.stdout.take().context("Pi stdout missing")?;
    let mut groups = BTreeSet::new();
    let mut descendants = BTreeMap::new();
    let mut pending = Vec::new();
    let mut control = Vec::new();
    let mut final_reason: Option<String> = None;
    let mut final_error: Option<String> = None;
    let mut eof = false;
    let mut request = serde_json::to_vec(&json!({"type":"get_state","id":"state"}))?;
    request.push(b'\n');
    request.extend(serde_json::to_vec(
        &json!({"type":"prompt","id":"prompt","message":run.spec.prompt}),
    )?);
    request.push(b'\n');
    let mut sent = 0;
    let result = (|| -> Result<()> {
        if let Some(process) = process_info(child.id() as i32)? {
            groups.insert(process.group);
            descendants.insert(process.pid, process);
        }
        nonblocking(output.as_raw_fd())?;
        // This function runs only in the dedicated supervisor process.
        nonblocking(libc::STDIN_FILENO)?;
        run.status = RunStatus::Running;
        run.started_at = Some(tasks::now());
        run.session_file = Some(session_file.to_string_lossy().into_owned());
        tasks::write_run(run_dir, run)?;
        nonblocking(input.as_ref().context("Pi stdin missing")?.as_raw_fd())?;
        let mut last_scan = Instant::now() - Duration::from_secs(1);
        loop {
            if last_scan.elapsed() >= Duration::from_millis(100) {
                discover(&mut descendants, &mut groups)?;
                last_scan = Instant::now();
            }
            let mut bytes = [0; 16 * 1024];
            match read_fd(libc::STDIN_FILENO, &mut bytes) {
                Ok(0) => {
                    run.status = RunStatus::Interrupted;
                    run.error = Some("parent control pipe closed".into());
                    break;
                }
                Ok(n) => {
                    control.extend_from_slice(&bytes[..n]);
                    if control.split(|b| *b == b'\n').any(|line| line == b"cancel") {
                        run.status = RunStatus::Cancelled;
                        break;
                    }
                    if control.len() > 1024 {
                        bail!("invalid supervisor control input");
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e.into()),
            }
            if started.elapsed() >= timeout {
                run.status = RunStatus::TimedOut;
                run.error = Some("run timeout exceeded".into());
                break;
            }
            if sent < request.len() {
                match input
                    .as_mut()
                    .context("Pi stdin missing")?
                    .write(&request[sent..])
                {
                    Ok(0) => bail!("Pi stdin closed"),
                    Ok(n) => sent += n,
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            let mut settled = false;
            // Read a bounded batch so chatty output cannot starve cancellation.
            for _ in 0..16 {
                match output.read(&mut bytes) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        events.write_all(&bytes[..n])?;
                        pending.extend_from_slice(&bytes[..n]);
                        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                            let line: Vec<_> = pending.drain(..=end).collect();
                            if line.iter().all(u8::is_ascii_whitespace) {
                                continue;
                            }
                            let event: Value =
                                serde_json::from_slice(&line).context("invalid Pi JSONL event")?;
                            match event["type"].as_str() {
                                Some("response")
                                    if event["command"] == "prompt"
                                        && event["success"] == false =>
                                {
                                    bail!(
                                        "Pi rejected prompt: {}",
                                        event["error"].as_str().unwrap_or("unknown error")
                                    );
                                }
                                Some("response") if event["command"] == "get_state" => {
                                    if let Some(path) = event["data"]["sessionFile"].as_str() {
                                        run.session_file = Some(path.into());
                                    }
                                }
                                Some("message_end") if event["message"]["role"] == "assistant" => {
                                    final_reason =
                                        event["message"]["stopReason"].as_str().map(str::to_owned);
                                    final_error = event["message"]["errorMessage"]
                                        .as_str()
                                        .map(str::to_owned);
                                }
                                Some("agent_settled") => settled = true,
                                _ => (),
                            }
                        }
                        if pending.len() > 16 * 1024 * 1024 {
                            bail!("Pi event exceeds 16 MiB");
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e.into()),
                }
            }
            if settled {
                if final_reason.as_deref() == Some("stop") {
                    run.status = RunStatus::Succeeded;
                } else {
                    run.status = RunStatus::Failed;
                    run.error = Some(final_error.unwrap_or_else(|| {
                        format!(
                            "Pi settled with stop reason {}",
                            final_reason.as_deref().unwrap_or("missing")
                        )
                    }));
                }
                break;
            }
            if eof || child.try_wait()?.is_some() {
                bail!("Pi exited before agent_settled");
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    // Snapshot detached descendants before Pi can exit and orphan them.
    let discovery = discover(&mut descendants, &mut groups);
    // Never append commands in the middle of an incomplete prompt JSON line.
    if sent == request.len() {
        let _ = send(&mut input, json!({"type":"clear_queue","id":"clear"}));
        let _ = send(&mut input, json!({"type":"abort","id":"abort"}));
    }
    drop(input);
    let cleanup = cleanup(
        &mut child,
        &mut descendants,
        &mut groups,
        &mut output,
        &mut events,
    );
    if cleanup.is_err() {
        // Inspection or log I/O failure must not skip our last cleanup attempt.
        // signal_owned rechecks births; Child::kill knows whether it was reaped.
        let _ = signal_owned(&descendants, &groups, libc::SIGKILL);
        let _ = child.kill();
        let _ = child.try_wait();
    }
    events.sync_all()?;
    if let Err(error) = discovery.and(cleanup) {
        run.status = RunStatus::CleanupFailed;
        run.error = Some(format!(
            "{}cleanup: {error:#}",
            result
                .as_ref()
                .err()
                .map(|e| format!("{e:#}; "))
                .unwrap_or_default()
        ));
        return Ok(());
    }
    result
}

fn send(input: &mut Option<ChildStdin>, value: Value) -> Result<()> {
    if let Some(input) = input {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        input.write_all(&bytes)?;
    }
    Ok(())
}
fn nonblocking(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
fn read_fd(fd: i32, buffer: &mut [u8]) -> io::Result<usize> {
    let n = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}
// Birth identity prevents a PID retained across a long run from claiming a
// later, unrelated process (or that process's children and process group).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Process {
    pid: i32,
    parent: i32,
    group: i32,
    birth: (u64, u64),
}
impl Process {
    fn same_process(self, other: Self) -> bool {
        self.pid == other.pid && self.birth == other.birth
    }
}
#[cfg(target_os = "macos")]
fn process_info(pid: i32) -> Result<Option<Process>> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if read == size {
        return Ok(Some(Process {
            pid,
            parent: info.pbi_ppid as i32,
            group: info.pbi_pgid as i32,
            birth: (info.pbi_start_tvsec, info.pbi_start_tvusec),
        }));
    }
    let error = io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(libc::ESRCH) | Some(libc::ENOENT)) {
        Ok(None)
    } else {
        Err(error).context("inspect process birth identity")
    }
}
#[cfg(not(target_os = "macos"))]
fn process_info(pid: i32) -> Result<Option<Process>> {
    let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .context("invalid process stat")?
        .1
        .split_whitespace()
        .collect();
    Ok(Some(Process {
        pid,
        parent: fields.get(1).context("missing process parent")?.parse()?,
        group: fields.get(2).context("missing process group")?.parse()?,
        birth: (
            fields
                .get(19)
                .context("missing process start time")?
                .parse()?,
            0,
        ),
    }))
}
fn retain_owned(
    owned: &mut BTreeMap<i32, Process>,
    groups: &mut BTreeSet<i32>,
    current: &BTreeMap<i32, Process>,
) {
    owned.retain(|pid, old| current.get(pid).is_some_and(|now| old.same_process(*now)));
    // A live member with the same birth identity and unchanged group anchors
    // group ownership even when its leader has already exited.
    groups.retain(|group| {
        owned
            .values()
            .any(|old| old.group == *group && current[&old.pid].group == *group)
    });
    for old in owned.values_mut() {
        *old = current[&old.pid];
    }
    loop {
        let before = owned.len();
        for process in current.values() {
            if owned.contains_key(&process.parent) || groups.contains(&process.group) {
                owned.insert(process.pid, *process);
                if process.group == process.pid {
                    groups.insert(process.group);
                }
            }
        }
        if owned.len() == before {
            break;
        }
    }
}
fn discover(owned: &mut BTreeMap<i32, Process>, groups: &mut BTreeSet<i32>) -> Result<()> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid="])
        .output()?;
    if !output.status.success() {
        bail!("cannot inspect Pi descendants");
    }
    let rows: Vec<Vec<i32>> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .map(str::parse)
                .collect::<std::result::Result<Vec<_>, _>>()
                .ok()
        })
        .filter(|row| row.len() == 3)
        .collect();
    let mut current = BTreeMap::new();
    for pid in owned.keys() {
        if let Some(process) = process_info(*pid)? {
            current.insert(*pid, process);
        }
    }
    retain_owned(owned, groups, &current);
    loop {
        let before = owned.len();
        for row in &rows {
            if !owned.contains_key(&row[0])
                && (owned.contains_key(&row[1]) || groups.contains(&row[2]))
                && let Some(process) = process_info(row[0])?
            {
                // Recheck ancestry/group using the same native snapshot as birth.
                if owned.contains_key(&process.parent) || groups.contains(&process.group) {
                    owned.insert(process.pid, process);
                    if process.pid == process.group {
                        groups.insert(process.group);
                    }
                }
            }
        }
        if owned.len() == before {
            break;
        }
    }
    Ok(())
}
fn signal_owned(owned: &BTreeMap<i32, Process>, groups: &BTreeSet<i32>, signal: i32) -> Result<()> {
    for group in groups {
        for process in owned.values().filter(|process| process.group == *group) {
            if process_info(process.pid)?
                .is_some_and(|now| process.same_process(now) && now.group == *group)
            {
                unsafe {
                    libc::kill(-*group, signal);
                }
                break;
            }
        }
    }
    for process in owned.values().rev() {
        if process_info(process.pid)?.is_some_and(|now| process.same_process(now)) {
            unsafe {
                libc::kill(process.pid, signal);
            }
        }
    }
    Ok(())
}
fn cleanup(
    child: &mut Child,
    descendants: &mut BTreeMap<i32, Process>,
    groups: &mut BTreeSet<i32>,
    output: &mut impl Read,
    events: &mut File,
) -> Result<()> {
    let start = Instant::now();
    let mut term = false;
    let mut kill = false;
    loop {
        let mut buffer = [0; 16 * 1024];
        for _ in 0..16 {
            match output.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => events.write_all(&buffer[..n])?,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
        discover(descendants, groups)?;
        let reaped = child.try_wait()?.is_some();
        if reaped && descendants.is_empty() {
            return Ok(());
        }
        let elapsed = start.elapsed();
        let signal = if elapsed >= Duration::from_millis(1000) && !kill {
            kill = true;
            Some(libc::SIGKILL)
        } else if elapsed >= Duration::from_millis(300) && !term {
            term = true;
            Some(libc::SIGTERM)
        } else {
            None
        };
        if let Some(signal) = signal {
            signal_owned(descendants, groups, signal)?;
        }
        if elapsed >= Duration::from_secs(4) {
            bail!("Pi or descendants remain after bounded shutdown");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn version(pi: &Path, run_dir: &Path) -> Result<String> {
    let path = run_dir.join("pi-version.txt");
    let mut child = Command::new(pi)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(File::create(&path)?)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Pi version command failed");
            }
            return Ok(fs::read_to_string(&path)?.trim().to_owned());
        }
        if start.elapsed() > Duration::from_secs(2) {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            child.wait()?;
            bail!("Pi version command timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reused_pid_cannot_claim_process_group_or_children() {
        let old = Process {
            pid: 40,
            parent: 1,
            group: 40,
            birth: (100, 1),
        };
        let replacement = Process {
            birth: (100, 2),
            ..old
        };
        let unrelated_child = Process {
            pid: 41,
            parent: 40,
            group: 40,
            birth: (101, 0),
        };
        let mut owned = BTreeMap::from([(old.pid, old)]);
        let mut groups = BTreeSet::from([old.group]);
        let current = BTreeMap::from([(40, replacement), (41, unrelated_child)]);
        retain_owned(&mut owned, &mut groups, &current);
        assert!(owned.is_empty());
        assert!(groups.is_empty());
    }

    #[test]
    fn surviving_member_preserves_owned_group_after_leader_exits() {
        let member = Process {
            pid: 41,
            parent: 40,
            group: 40,
            birth: (101, 0),
        };
        let orphan = Process {
            parent: 1,
            ..member
        };
        let new_child = Process {
            pid: 42,
            parent: 1,
            group: 40,
            birth: (102, 0),
        };
        let mut owned = BTreeMap::from([(member.pid, member)]);
        let mut groups = BTreeSet::from([40]);
        let current = BTreeMap::from([(41, orphan), (42, new_child)]);
        retain_owned(&mut owned, &mut groups, &current);
        assert_eq!(owned.len(), 2);
        assert_eq!(groups, BTreeSet::from([40]));
    }
}
