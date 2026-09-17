use super::*;
use anyhow::{Result, bail};
use std::io as std_io;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
#[cfg(target_os = "macos")]
use std::sync::mpsc::Receiver;

struct TerminationGuard(Arc<Session>);

impl TerminationGuard {
    fn force_cleanup(&self) -> Result<()> {
        let pgid = self.0.pgid;
        if !group_exists(pgid)? {
            return Ok(());
        }
        match verify_owned_group(&self.0) {
            Ok(()) => {}
            Err(_error) if !group_exists(pgid)? => return Ok(()),
            Err(error) => return Err(error),
        }
        let _ = signal_group(pgid, libc::SIGTERM)?;
        let _ = signal_group(pgid, libc::SIGCONT)?;
        if wait_for_group_exit(pgid, Instant::now() + Duration::from_secs(2))? {
            return Ok(());
        }
        if !group_exists(pgid)? {
            return Ok(());
        }
        verify_owned_group(&self.0)?;
        let _ = signal_group(pgid, libc::SIGKILL)?;
        if !wait_for_group_exit(pgid, Instant::now() + Duration::from_secs(2))? {
            bail!("test cleanup could not remove PTY process group {pgid}");
        }
        Ok(())
    }
}

impl Drop for TerminationGuard {
    fn drop(&mut self) {
        let result = if thread::panicking() {
            self.force_cleanup()
        } else {
            self.0.terminate(Duration::from_millis(200))
        };
        if let Err(error) = result
            && !error.is::<AlreadyExited>()
        {
            eprintln!(
                "session test cleanup failed for process group {}: {error}",
                self.0.pgid
            );
        }
    }
}

#[cfg(target_os = "macos")]
struct ReapGate {
    entered: mpsc::SyncSender<()>,
    release: Mutex<Receiver<()>>,
    signal_entered: mpsc::SyncSender<()>,
    signal_release: Mutex<Receiver<()>>,
    signal_result_entered: mpsc::SyncSender<()>,
}

#[cfg(target_os = "macos")]
impl ReapGate {
    #[allow(clippy::type_complexity)]
    fn new() -> (
        Arc<Self>,
        Receiver<()>,
        mpsc::SyncSender<()>,
        Receiver<()>,
        mpsc::SyncSender<()>,
        Receiver<()>,
    ) {
        let (entered_sender, entered) = mpsc::sync_channel(1);
        let (release, release_receiver) = mpsc::sync_channel(1);
        let (signal_entered_sender, signal_entered) = mpsc::sync_channel(1);
        let (signal_release, signal_release_receiver) = mpsc::sync_channel(1);
        let (signal_result_entered_sender, signal_result_entered) = mpsc::sync_channel(1);
        (
            Arc::new(Self {
                entered: entered_sender,
                release: Mutex::new(release_receiver),
                signal_entered: signal_entered_sender,
                signal_release: Mutex::new(signal_release_receiver),
                signal_result_entered: signal_result_entered_sender,
            }),
            entered,
            release,
            signal_entered,
            signal_release,
            signal_result_entered,
        )
    }

    fn wait(&self) {
        let _ = self.entered.send(());
        let _ = self.release.lock().unwrap().recv();
    }

    fn before_signal(&self) {
        let _ = self.signal_entered.send(());
        let _ = self.signal_release.lock().unwrap().recv();
    }

    fn after_signal(&self) -> Option<anyhow::Error> {
        let _ = self.signal_result_entered.send(());
        None
    }
}

#[cfg(target_os = "macos")]
struct ReapGateCleanup {
    release: mpsc::SyncSender<()>,
    signal_release: mpsc::SyncSender<()>,
}

#[cfg(target_os = "macos")]
impl Drop for ReapGateCleanup {
    fn drop(&mut self) {
        let _ = self.signal_release.try_send(());
        let _ = self.release.try_send(());
    }
}

struct CancellableBarrier {
    state: Mutex<CancellableBarrierState>,
    changed: Condvar,
}

struct CancellableBarrierState {
    waiting: usize,
    generation: u64,
    cancelled: bool,
}

impl CancellableBarrier {
    fn new() -> Self {
        Self {
            state: Mutex::new(CancellableBarrierState {
                waiting: 0,
                generation: 0,
                cancelled: false,
            }),
            changed: Condvar::new(),
        }
    }

    fn wait(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.cancelled {
            return false;
        }
        let generation = state.generation;
        state.waiting += 1;
        if state.waiting == 2 {
            state.waiting = 0;
            state.generation = state.generation.wrapping_add(1);
            self.changed.notify_all();
            return true;
        }
        while !state.cancelled && state.generation == generation {
            state = self.changed.wait(state).unwrap();
        }
        !state.cancelled
    }

    fn cancel(&self) {
        let mut state = self.state.lock().unwrap();
        state.cancelled = true;
        self.changed.notify_all();
    }
}

struct BarrierGuard(Arc<CancellableBarrier>);

impl Drop for BarrierGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn spawn_test_shell() -> Arc<Session> {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(1),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "local".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![OsString::from("sh")],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let _ = Box::leak(Box::new(dir));
    let _dispatcher = dispatch_test_events(session.clone(), rx);
    session
}

fn dispatch_test_events(
    session: Arc<Session>,
    rx: crate::server::ReportingReceiver<SessionEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let exited = matches!(event, SessionEvent::Exited { .. });
            session.apply_event(event);
            if exited {
                break;
            }
        }
    })
}

#[test]
fn retired_run_output_and_exit_cannot_mutate_a_live_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let (events, receiver) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(44),
        SessionSpec {
            run: SessionRunId(2), kind: SessionKind::Terminal,
            project: "p".into(), workspace: "w".into(), name: "replacement".into(),
            label: "sh".into(), cwd: directory.path().to_path_buf(),
            argv: vec!["/bin/sh".into(), "-c".into(),
                "stty -echo; printf 'CURRENT_READY\\n'; IFS= read -r line; printf 'ACK:%s\\n' \"$line\"; IFS= read -r done".into()],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 }, events, NO_REGISTER,
    ).unwrap();
    let _cleanup = TerminationGuard(session.clone());
    let dispatcher = dispatch_test_events(session.clone(), receiver);
    assert!(wait_for_screen(
        &session,
        "CURRENT_READY",
        Duration::from_secs(3)
    ));
    session.apply_event(SessionEvent::Output {
        id: session.id(),
        run: SessionRunId(1),
        bytes: b"RETIRED_OUTPUT".to_vec(),
    });
    session.apply_event(SessionEvent::Exited {
        id: session.id(),
        run: SessionRunId(1),
        phase: SessionPhase::Exited {
            code: Some(99),
            signal: None,
        },
    });
    assert!(
        session.is_live(),
        "retired exit changed the replacement's lifecycle"
    );
    assert!(!String::from_utf8_lossy(&session.current_screen()).contains("RETIRED_OUTPUT"));
    session.send_text("still-live", true).unwrap();
    assert!(wait_for_screen(
        &session,
        "ACK:still-live",
        Duration::from_secs(3)
    ));
    session.terminate(Duration::from_millis(200)).unwrap();
    dispatcher.join().unwrap();
}

fn wait_for_screen(session: &Session, marker: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if String::from_utf8_lossy(&session.current_screen()).contains(marker) {
            return true;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    false
}

#[cfg(target_os = "macos")]
fn wait_for_process_state(pid: u32, expected: char, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let pid = pid.to_string();
    while Instant::now() < deadline {
        if let Ok(output) = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid])
            .output()
        {
            let state = String::from_utf8_lossy(&output.stdout);
            if state.trim_start().starts_with(expected) {
                return true;
            }
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    false
}

#[cfg(target_os = "macos")]
fn spawn_term_race_session(reap_gate: Arc<ReapGate>) -> (Arc<Session>, JoinHandle<()>) {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let signal_gate = Arc::clone(&reap_gate);
    let signal_result_gate = Arc::clone(&reap_gate);
    let session = Session::spawn_with_test_hooks(
        SessionId(50),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "term-race".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap 'exit 0' HUP TERM; printf READY; while :; do read line; done".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        Some(Arc::new(move || reap_gate.wait())),
        Some(Arc::new(move || signal_gate.before_signal())),
        Some(Arc::new(move || signal_result_gate.after_signal())),
    )
    .unwrap();
    let _ = Box::leak(Box::new(dir));
    let dispatcher = dispatch_test_events(session.clone(), rx);
    (session, dispatcher)
}

fn spawn_live_refusal_session(
    signal_result_hook: Arc<dyn Fn() -> Option<anyhow::Error> + Send + Sync>,
) -> (Arc<Session>, JoinHandle<()>) {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_with_test_hooks(
        SessionId(51),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "live-refusal".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap '' HUP TERM; printf READY; while :; do read line; done".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        None,
        None,
        Some(signal_result_hook),
    )
    .unwrap();
    let _ = Box::leak(Box::new(dir));
    let dispatcher = dispatch_test_events(session.clone(), rx);
    (session, dispatcher)
}

#[test]
fn pause_resume_phase_and_input_admission() {
    let session = spawn_test_shell();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    session.write(b"printf 'REA%s\\n' DY\r").unwrap();
    assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));

    assert!(session.set_paused(true).unwrap());
    assert_eq!(session.summary().phase, SessionPhase::Paused);
    assert!(!session.set_paused(true).unwrap());
    assert!(session.write(b"SHOULD_NOT_BE_ACCEPTED\r").is_err());
    assert!(session.send_text("SHOULD_NOT_BE_ACCEPTED", true).is_err());
    assert!(session.set_paused(false).unwrap());
    assert!(!session.set_paused(false).unwrap());
    session.write(b"printf 'RESUME_%s\\n' ACK\r").unwrap();
    assert!(wait_for_screen(
        &session,
        "RESUME_ACK",
        Duration::from_secs(2)
    ));
    session.terminate(Duration::from_millis(200)).unwrap();
    assert!(session.set_paused(true).is_err());
    assert!(session.set_paused(false).is_err());
}

#[test]
fn agent_report_order_rejects_zero_sequence_without_mutating_unset() {
    let mut order = ReportOrder::default();
    assert!(order.accept(Some(0)).is_err());
    assert_eq!(order, ReportOrder::Unset);
}

#[test]
fn agent_report_requires_capability_and_preserves_order_on_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let (events, receiver) = crate::server::event_channel(None);
    let capability = [0x37; 32];
    let session = Session::spawn_registered(
        SessionId(700),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "agent".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "while IFS= read -r line; do :; done".into(),
            ],
            hook_env: Some(HookEnvironment {
                socket: PathBuf::from("/private/test/ovrcr.sock"),
                session: SessionId(700),
                capability,
            }),
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        NO_REGISTER,
    )
    .unwrap();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let _dispatcher = dispatch_test_events(session.clone(), receiver);

    let report = |capability, sequence, activity| ovrcr_protocol::AgentReport {
        session: SessionId(700),
        capability,
        sequence,
        update: ovrcr_protocol::AgentUpdate::Activity(activity),
    };
    assert!(
        session
            .apply_agent_report(&report([0x38; 32], Some(1), AgentActivity::Busy))
            .is_err()
    );
    assert_eq!(session.summary().activity, AgentActivity::Unknown);
    assert!(
        session
            .apply_agent_report(&report(capability, Some(0), AgentActivity::Busy))
            .is_err()
    );
    assert_eq!(session.summary().activity, AgentActivity::Unknown);
    assert!(
        session
            .apply_agent_report(&report(capability, Some(1), AgentActivity::Busy))
            .unwrap()
    );
    assert_eq!(session.summary().activity, AgentActivity::Busy);
    assert!(
        session
            .apply_agent_report(&report(capability, Some(1), AgentActivity::Idle))
            .is_err()
    );
    assert_eq!(session.summary().activity, AgentActivity::Busy);
    assert!(
        !session
            .apply_agent_report(&report(capability, Some(2), AgentActivity::Busy))
            .unwrap()
    );
    assert!(
        session
            .apply_agent_report(&report(capability, Some(2), AgentActivity::Idle))
            .is_err()
    );
    assert_eq!(session.summary().activity, AgentActivity::Busy);
    session.set_paused(true).unwrap();
    assert!(
        session
            .apply_agent_report(&report(capability, Some(3), AgentActivity::Idle))
            .unwrap()
    );
    assert_eq!(session.summary().activity, AgentActivity::Idle);
    session.terminate(Duration::from_millis(200)).unwrap();
    assert!(
        session
            .apply_agent_report(&report(capability, Some(4), AgentActivity::Busy))
            .is_err()
    );
    assert_eq!(session.summary().activity, AgentActivity::Unknown);
}

#[test]
fn pause_resume_terminate_runs_term_handler() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
            SessionId(5),
            SessionSpec { run: SessionRunId(1), kind: SessionKind::Terminal, project: "p".into(),
            workspace: "w".into(),
            name: "term-handler".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap 'printf TERM_HANDLED; exit 0' HUP TERM; printf READY; while :; do read line; done"
                    .into(),
            ],
            hook_env: None, },
            TerminalSize { rows: 24, cols: 80 },
            tx, NO_REGISTER)
        .unwrap();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let dispatcher = dispatch_test_events(session.clone(), rx);

    assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));
    assert!(session.set_paused(true).unwrap());
    session.terminate(Duration::from_millis(200)).unwrap();
    dispatcher.join().unwrap();

    assert!(String::from_utf8_lossy(&session.current_screen()).contains("TERM_HANDLED"));
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
    assert!(!group_exists(session.pgid).unwrap());
}

#[test]
fn pause_resume_terminate_serializes_competing_controls() {
    let session = spawn_test_shell();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let gate = Arc::new(Barrier::new(3));
    let (results, received) = mpsc::sync_channel(3);

    let pause_gate = Arc::clone(&gate);
    let pause_session = Arc::clone(&session);
    let pause_results = results.clone();
    let pause = thread::spawn(move || {
        pause_gate.wait();
        let result = pause_session
            .set_paused(true)
            .map(|_| ())
            .map_err(|error| error.to_string());
        pause_results.send(("pause", result)).unwrap();
    });

    let resume_gate = Arc::clone(&gate);
    let resume_session = Arc::clone(&session);
    let resume_results = results.clone();
    let resume = thread::spawn(move || {
        resume_gate.wait();
        let result = resume_session
            .set_paused(false)
            .map(|_| ())
            .map_err(|error| error.to_string());
        resume_results.send(("resume", result)).unwrap();
    });

    let terminate_gate = Arc::clone(&gate);
    let terminate_session = Arc::clone(&session);
    let terminate = thread::spawn(move || {
        terminate_gate.wait();
        let result = terminate_session
            .terminate(Duration::from_millis(200))
            .map_err(|error| error.to_string());
        results.send(("terminate", result)).unwrap();
    });

    let mut outcomes = Vec::new();
    for _ in 0..3 {
        outcomes.push(
            received
                .recv_timeout(Duration::from_secs(2))
                .expect("competing controls must finish promptly"),
        );
    }
    pause.join().unwrap();
    resume.join().unwrap();
    terminate.join().unwrap();

    assert!(
        outcomes
            .iter()
            .any(|(name, result)| { *name == "terminate" && result.is_ok() })
    );
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
    assert!(!group_exists(session.pgid).unwrap());
}

#[cfg(target_os = "macos")]
#[test]
fn terminate_reaps_group_after_transient_sigcont_permission() {
    let (reap_gate, entered, release, signal_entered, signal_release, signal_result_entered) =
        ReapGate::new();
    let (session, dispatcher) = spawn_term_race_session(reap_gate);
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let _gate_cleanup = ReapGateCleanup {
        release: release.clone(),
        signal_release: signal_release.clone(),
    };
    assert!(entered.recv_timeout(Duration::from_secs(2)).is_ok());
    assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));

    let leader = session.summary().pid.unwrap();
    let (termination_sender, termination_receiver) = mpsc::sync_channel(1);
    let termination_session = Arc::clone(&session);
    let termination = thread::spawn(move || {
        let _ = termination_sender.send(
            termination_session
                .terminate(Duration::from_millis(200))
                .map_err(|error| format!("{error:#}")),
        );
    });
    assert!(signal_entered.recv_timeout(Duration::from_secs(2)).is_ok());
    assert!(wait_for_process_state(leader, 'Z', Duration::from_secs(2)));
    assert!(group_exists(session.pgid).unwrap());
    signal_release.send(()).unwrap();
    assert!(
        signal_result_entered
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
    );
    release.send(()).unwrap();

    assert!(
        termination_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_ok()
    );
    termination.join().unwrap();
    dispatcher.join().unwrap();
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
    assert!(!group_exists(session.pgid).unwrap());
}

#[cfg(target_os = "macos")]
#[test]
fn terminate_retains_sigcont_permission_error_when_group_remains() {
    let (reap_gate, entered, release, signal_entered, signal_release, signal_result_entered) =
        ReapGate::new();
    let (session, dispatcher) = spawn_term_race_session(reap_gate);
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let _gate_cleanup = ReapGateCleanup {
        release: release.clone(),
        signal_release: signal_release.clone(),
    };
    assert!(entered.recv_timeout(Duration::from_secs(2)).is_ok());
    assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));

    let leader = session.summary().pid.unwrap();
    let (termination_sender, termination_receiver) = mpsc::sync_channel(1);
    let termination_session = Arc::clone(&session);
    let termination = thread::spawn(move || {
        let result = termination_session
            .terminate(Duration::from_millis(50))
            .map_err(|error| format!("{error:#}"));
        let _ = termination_sender.send(result);
    });
    assert!(signal_entered.recv_timeout(Duration::from_secs(2)).is_ok());
    assert!(wait_for_process_state(leader, 'Z', Duration::from_secs(2)));
    assert!(group_exists(session.pgid).unwrap());
    signal_release.send(()).unwrap();
    assert!(
        signal_result_entered
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
    );

    let error = termination_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .expect_err("a still-present refused group must retain SIGCONT error");
    assert!(error.contains("signal PTY process group"));
    assert!(error.contains("Operation not permitted"));
    release.send(()).unwrap();
    termination.join().unwrap();
    session.wait_until_exited(Duration::from_secs(2)).unwrap();
    dispatcher.join().unwrap();
    assert!(!group_exists(session.pgid).unwrap());
}

#[test]
fn terminate_retains_sigcont_permission_error_for_live_group() {
    let refuse_sigcont = Arc::new(AtomicBool::new(true));
    let refusal = Arc::clone(&refuse_sigcont);
    let signal_result_hook = Arc::new(move || {
        if refusal.load(Ordering::Acquire) {
            Some(anyhow::anyhow!(
                "signal PTY process group: Operation not permitted (os error 1)"
            ))
        } else {
            None
        }
    });
    let (session, dispatcher) = spawn_live_refusal_session(signal_result_hook);
    let _cleanup = TerminationGuard(Arc::clone(&session));
    assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));
    let leader = session.summary().pid.unwrap();

    let error = session
        .terminate(Duration::from_millis(50))
        .expect_err("a still-present refused group must retain SIGCONT error");
    assert!(error.to_string().contains("signal PTY process group"));
    assert!(error.to_string().contains("Operation not permitted"));
    assert!(pid_exists(leader), "SIGKILL must not reach the live leader");
    assert!(group_exists(session.pgid).unwrap());

    refuse_sigcont.store(false, Ordering::Release);
    session.terminate(Duration::from_millis(200)).unwrap();
    dispatcher.join().unwrap();
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
    assert!(!group_exists(session.pgid).unwrap());
}

#[test]
fn pause_resume_exit_event_cannot_be_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(4),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "exit-race".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "printf 'EXIT_%s' READY; IFS= read -r _".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let gate = Arc::new(CancellableBarrier::new());
    let _gate_cleanup = BarrierGuard(Arc::clone(&gate));
    let (exit_ready, exit_ready_receiver) = mpsc::sync_channel(1);
    let (dispatch_done, dispatch_done_receiver) = mpsc::sync_channel(1);
    let (control_result, control_result_receiver) = mpsc::sync_channel(1);
    let dispatcher = dispatch_test_events_with_exit_gate(
        Arc::clone(&session),
        rx,
        Arc::clone(&gate),
        exit_ready,
        dispatch_done,
    );
    let control_gate = Arc::clone(&gate);
    let control_session = Arc::clone(&session);
    let control = thread::spawn(move || {
        if !control_gate.wait() {
            let _ = control_result.send(None);
            return;
        }
        let _ = control_result.send(Some(control_session.set_paused(true)));
    });

    assert!(wait_for_screen(
        &session,
        "EXIT_READY",
        Duration::from_secs(2)
    ));
    session.write(b"\n").unwrap();
    assert!(
        exit_ready_receiver
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
    );
    assert!(matches!(
        control_result_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        Some(Err(_))
    ));
    assert!(
        dispatch_done_receiver
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
    );
    control.join().unwrap();
    dispatcher.join().unwrap();
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
}

fn dispatch_test_events_with_exit_gate(
    session: Arc<Session>,
    rx: crate::server::ReportingReceiver<SessionEvent>,
    gate: Arc<CancellableBarrier>,
    exit_ready: mpsc::SyncSender<()>,
    dispatch_done: mpsc::SyncSender<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let exited = matches!(event, SessionEvent::Exited { .. });
            if exited {
                let _ = exit_ready.send(());
                gate.wait();
            }
            session.apply_event(event);
            if exited {
                let _ = dispatch_done.send(());
                break;
            }
        }
    })
}

#[test]
fn pause_resume_rejects_unsafe_group() {
    assert!(verify_group_identity(0, false).is_err());
    assert!(verify_group_identity(1, false).is_err());
    assert!(verify_group_identity(unsafe { libc::getpgrp() }, false).is_err());
}

#[test]
fn shell_round_trip_updates_the_current_screen() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(1),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "local".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![OsString::from("sh")],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let dispatcher = dispatch_test_events(session.clone(), rx);
    session.write(b"printf 'OVRCR_%s' READY\r").unwrap();
    assert!(wait_for_screen(
        &session,
        "OVRCR_READY",
        Duration::from_secs(2)
    ));
    session.terminate(Duration::from_millis(200)).unwrap();
    dispatcher.join().unwrap();
}

#[test]
fn final_output_is_parsed_before_session_becomes_removable() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(2),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "final".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec!["sh".into(), "-c".into(), "printf FINAL_MARKER".into()],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let first_event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(session.summary().phase, SessionPhase::Running));
    let mut next_event = Some(first_event);
    let mut saw_output = false;
    loop {
        let event = match next_event.take() {
            Some(event) => event,
            None => rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        };
        let exited = matches!(event, SessionEvent::Exited { .. });
        if !exited {
            saw_output = saw_output || matches!(event, SessionEvent::Output { .. });
        }
        session.apply_event(event);
        if exited {
            break;
        }
    }
    assert!(saw_output);
    assert!(String::from_utf8_lossy(&session.current_screen()).contains("FINAL_MARKER"));
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
}

#[test]
fn resize_reaches_the_child_tty() {
    let session = spawn_test_shell();
    session
        .resize(TerminalSize {
            rows: 37,
            cols: 111,
        })
        .unwrap();
    session.write(b"stty size\r").unwrap();
    assert!(wait_for_screen(&session, "37 111", Duration::from_secs(2)));
    session.terminate(Duration::from_millis(200)).unwrap();
}

#[test]
fn terminate_removes_the_whole_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(3),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "group".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap '' HUP; sleep 30 & printf 'OVRCR_DESC:%s\\n' \"$!\"; exit".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let dispatcher = dispatch_test_events(session.clone(), rx);
    let leader = session.summary().pid.unwrap();
    let screen_deadline = Instant::now() + Duration::from_secs(2);
    let descendant = loop {
        if let Some(pid) = extract_tagged_pid(&session.current_screen(), b"OVRCR_DESC:")
            && pid_exists(pid)
        {
            break pid;
        }
        assert!(
            Instant::now() < screen_deadline,
            "descendant PID was not observed: {}",
            String::from_utf8_lossy(&session.current_screen())
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    assert!(pid_exists(descendant));
    let pgid = session.pgid;
    struct GroupTeardown {
        pgid: libc::pid_t,
    }
    impl Drop for GroupTeardown {
        fn drop(&mut self) {
            if group_exists(self.pgid).unwrap_or(true) {
                let _ = signal_group(self.pgid, libc::SIGKILL);
                let _ = wait_for_group_exit(self.pgid, Instant::now() + Duration::from_secs(2));
            }
        }
    }
    let _teardown = GroupTeardown { pgid };
    let _termination = session.terminate(Duration::from_millis(200));
    dispatcher.join().unwrap();
    assert_pid_is_gone(leader);
    assert_pid_is_gone(descendant);
    assert!(!group_exists(session.pgid).unwrap());
}

#[test]
fn terminate_listing_failure_does_not_certify_stop() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(23),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "listing-fail".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                // Keep both processes alive through SIGCONT. A naturally exiting
                // group can return macOS EPERM and mask the injected listing error.
                // Inherited ignores force owned SIGKILL cleanup after the grace period.
                "trap '' HUP TERM; sleep 30 & printf 'OVRCR_DESC:%s\\n' \"$!\"; wait".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let dispatcher = dispatch_test_events(session.clone(), rx);
    let leader = session.summary().pid.unwrap();
    let screen_deadline = Instant::now() + Duration::from_secs(2);
    let descendant = loop {
        if let Some(pid) = extract_tagged_pid(&session.current_screen(), b"OVRCR_DESC:")
            && pid_exists(pid)
        {
            break pid;
        }
        assert!(
            Instant::now() < screen_deadline,
            "descendant PID was not observed: {}",
            String::from_utf8_lossy(&session.current_screen())
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    assert!(pid_exists(descendant));
    session.set_listing_error_hook(Some(Arc::new(|| {
        Some(anyhow::anyhow!("ovrcr-test-injected-listing-failure"))
    })));
    let pgid = session.pgid;
    struct ListingFailTeardown {
        pgid: libc::pid_t,
    }
    impl Drop for ListingFailTeardown {
        fn drop(&mut self) {
            if group_exists(self.pgid).unwrap_or(true) {
                let _ = signal_group(self.pgid, libc::SIGKILL);
                let _ = wait_for_group_exit(self.pgid, Instant::now() + Duration::from_secs(2));
            }
        }
    }
    let _teardown = ListingFailTeardown { pgid };
    let termination = session.terminate(Duration::from_millis(200));
    let error = match termination {
        Err(error) => error,
        Ok(()) => panic!("listing failure must not certify stop"),
    };
    assert!(
        error
            .to_string()
            .contains("ovrcr-test-injected-listing-failure"),
        "must surface injected discovery error, not another path: {error:#}"
    );
    dispatcher.join().unwrap();
    assert_pid_is_gone(leader);
    assert_pid_is_gone(descendant);
    assert!(!group_exists(session.pgid).unwrap());
}

#[test]
fn pause_listing_failure_does_not_claim_paused() {
    let session = spawn_test_shell();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let before = session.summary().phase;
    assert!(before.is_live());
    session.set_listing_error_hook(Some(Arc::new(|| {
        Some(anyhow::anyhow!("ovrcr-test-injected-listing-failure"))
    })));
    let error = session
        .set_paused(true)
        .expect_err("listing failure must not pause");
    assert!(
        error
            .to_string()
            .contains("ovrcr-test-injected-listing-failure"),
        "must surface injected discovery error: {error:#}"
    );
    assert_eq!(
        session.summary().phase,
        before,
        "listing refusal must not claim pause"
    );
    session.set_listing_error_hook(None);
}

#[test]
fn terminate_noop_sigterm_does_not_ok() {
    let session = spawn_test_shell();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    session.set_term_result_hook(Some(Arc::new(|| Some(false))));
    let result = session.terminate(Duration::from_millis(200));
    assert!(
        result.is_err(),
        "no-op SIGTERM must not certify stop: {result:?}"
    );
    session.set_term_result_hook(None);
}

#[test]
fn terminate_removes_job_control_subgroups() {
    // An interactive shell puts each background job in its own process
    // group, so the job is invisible to leader-group signalling. Ownership
    // is the controlling terminal, and terminate must hang up every group
    // attached to it without waiting out the grace period.
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(4),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "jobs".into(),
            label: "bash".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec!["bash".into(), "--norc".into(), "+H".into(), "-i".into()],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let _cleanup = TerminationGuard(Arc::clone(&session));
    let dispatcher = dispatch_test_events(session.clone(), rx);
    session
        // Split the tag so the echoed command line does not match it.
        .write(b"sleep 300 & printf 'OVRCR_JO''B:%s\\n' \"$!\"\r")
        .unwrap();
    let screen_deadline = Instant::now() + Duration::from_secs(3);
    let job = loop {
        if let Some(pid) = extract_tagged_pid(&session.current_screen(), b"OVRCR_JOB:")
            && pid_exists(pid)
        {
            break pid;
        }
        assert!(
            Instant::now() < screen_deadline,
            "job PID was not observed: {}",
            String::from_utf8_lossy(&session.current_screen())
        );
        thread::park_timeout(Duration::from_millis(5));
    };
    let job_group = unsafe { libc::getpgid(job as libc::pid_t) };
    assert_ne!(
        job_group, session.pgid,
        "bash job control should isolate the job"
    );
    assert_eq!(job_group, job as libc::pid_t);

    let started = Instant::now();
    session.terminate(Duration::from_secs(5)).unwrap();
    let elapsed = started.elapsed();
    dispatcher.join().unwrap();

    assert!(
        elapsed < Duration::from_secs(2),
        "interactive shell should hang up promptly, took {elapsed:?}"
    );
    assert_pid_is_gone(job);
    assert!(!group_exists(session.pgid).unwrap());
    assert!(!group_exists(job_group).unwrap());
}

#[test]
fn agent_report_order_rejects_replay_and_mode_switch() {
    let mut order = ReportOrder::default();
    assert!(order.accept(Some(2)).is_ok());
    assert!(order.accept(Some(1)).is_err());
    assert!(order.accept(Some(2)).is_err());
    assert!(order.accept(None).is_err());
    assert_eq!(order, ReportOrder::Sequenced(2));
    assert!(order.accept(Some(3)).is_ok());
}

#[test]
fn agent_report_order_receipt_accepts_repeated_values() {
    let mut order = ReportOrder::default();
    assert!(order.accept(None).is_ok());
    assert!(order.accept(None).is_ok());
    assert_eq!(order, ReportOrder::Receipt);
    assert!(order.accept(Some(1)).is_err());
}

#[test]
fn group_leader_timeout_override_keeps_the_default_bound() {
    assert_eq!(group_leader_timeout(None), GROUP_LEADER_TIMEOUT);
    assert_eq!(group_leader_timeout(Some("")), GROUP_LEADER_TIMEOUT);
    assert_eq!(group_leader_timeout(Some("   ")), GROUP_LEADER_TIMEOUT);
    assert_eq!(group_leader_timeout(Some("soon")), GROUP_LEADER_TIMEOUT);
    assert_eq!(group_leader_timeout(Some("-5")), GROUP_LEADER_TIMEOUT);
    assert_eq!(
        group_leader_timeout(Some(" 250 ")),
        Duration::from_millis(250)
    );
}

#[test]
fn group_leader_timeout_kills_the_child() {
    // `portable_pty` calls `setsid` in the child's `pre_exec`, before our
    // command runs, so no real command can withhold its process group long
    // enough to reach the deadline. The probe stands in for that child and
    // nothing else is simulated: a live PTY child, the real poll loop against
    // the override bound, the real kill, and the kernel's own answer to
    // `kill(pid, 0)`.
    let dir = tempfile::tempdir().unwrap();
    let (events, _events_receiver) = crate::server::event_channel(None);
    let (observed, observed_pid) = mpsc::sync_channel(1);
    let probe = move |pid: libc::pid_t| {
        let _ = observed.try_send(pid);
        false
    };
    let started = Instant::now();
    let spawned = Session::spawn_with_leader_wait(
        SessionId(70),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "wedged".into(),
            label: "sh".into(),
            cwd: dir.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "while IFS= read -r line; do :; done".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        LeaderWaitOverride {
            timeout: Duration::from_millis(200),
            probe: &probe,
        },
    );
    let Err(error) = spawned else {
        panic!("spawn must fail when the child never leads a process group");
    };
    assert!(
        !error.is::<NoProcessStarted>(),
        "a started program may have spawned descendants"
    );
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(200),
        "spawn must poll for the whole override bound, waited {waited:?}"
    );
    let message = format!("{error:#}");
    assert!(
        message.contains("did not provide a process-group leader"),
        "{message}"
    );
    assert!(message.contains("200ms"), "{message}");
    let pid = observed_pid
        .recv_timeout(Duration::from_secs(1))
        .expect("the group-leader poll must have observed the child PID") as u32;
    let deadline = Instant::now() + Duration::from_secs(1);
    while pid_exists(pid) && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    assert_pid_is_gone(pid);
}

fn extract_tagged_pid(screen: &[u8], tag: &[u8]) -> Option<u32> {
    let start = screen.windows(tag.len()).position(|window| window == tag)? + tag.len();
    let digits = screen[start..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .copied()
        .collect::<Vec<_>>();
    if digits.is_empty() {
        return None;
    }
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

fn pid_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std_io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn assert_pid_is_gone(pid: u32) {
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, 0) }, -1);
    assert_eq!(
        std_io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn compact_zsh_prompt_preserves_config_and_reports_failure() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cwd = dir.path().join("project");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&cwd).unwrap();
    std::fs::write(home.join(".zshenv"), "export PROMPT_FIXTURE_ENV=loaded\n").unwrap();
    std::fs::write(home.join(".zshrc"), "alias fixture_alias='printf CONFIG_%s_OK $PROMPT_FIXTURE_ENV'\nPROMPT='UGLY_THEME> '\nRPROMPT='RIGHT_THEME'\n").unwrap();
    let locale = if cfg!(target_os = "macos") {
        "en_US.UTF-8"
    } else {
        "C.UTF-8"
    };
    let shell = dir.path().join("zsh");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nunset OVRCR_ORIGINAL_ZDOTDIR\nexport HOME='{}' LC_ALL={locale} TERM=xterm-256color\nexec /bin/zsh -d \"$@\"\n",
            home.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (tx, rx) = crate::server::event_channel(None);
    let session = Session::spawn_registered(
        SessionId(1),
        SessionSpec {
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "local".into(),
            label: "zsh".into(),
            cwd,
            argv: vec![shell.into_os_string()],
            hook_env: None,
        },
        TerminalSize {
            rows: 24,
            cols: 100,
        },
        tx,
        NO_REGISTER,
    )
    .unwrap();
    let dispatcher = dispatch_test_events(session.clone(), rx);
    // Collect results before asserting so a regression still cleans up its PTY.
    let prompt_arrived = wait_for_screen(&session, "›", Duration::from_secs(3));
    let prompt = prompt_arrived
        && session
            .terminal
            .lock()
            .unwrap()
            .parser
            .screen()
            .contents()
            .contains("project ›");
    session.write(b"fixture_alias\r").unwrap();
    let config = wait_for_screen(&session, "CONFIG_loaded_OK", Duration::from_secs(3));
    session.write(b"false\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut red = false;
    while Instant::now() < deadline {
        let terminal = session.terminal.lock().unwrap();
        let screen = terminal.parser.screen();
        let (row, col) = screen.cursor_position();
        if col >= 2 {
            red = screen.cell(row, col - 2).is_some_and(|cell| {
                cell.contents() == "›" && cell.fgcolor() == vt100::Color::Idx(1)
            });
        }
        drop(terminal);
        if red {
            break;
        }
        thread::yield_now();
    }
    session.terminate(Duration::from_millis(200)).unwrap();
    dispatcher.join().unwrap();
    assert!(
        prompt,
        "compact directory prompt missing: {}",
        session.terminal.lock().unwrap().parser.screen().contents()
    );
    assert!(config, "normal zsh environment and aliases did not load");
    assert!(red, "failed command did not render a red arrow");
}

#[test]
fn automatic_title_callbacks_handle_fragments_unicode_controls_and_bounds() {
    let mut parser = vt100::Parser::new_with_callbacks(3, 30, 0, SessionTitles::default());
    parser.process(b"\x1b]2;frag");
    assert_eq!(parser.callbacks().application, None);
    assert_eq!(parser.callbacks().revision, 0);
    parser.process("mented 🦀\x07".as_bytes());
    assert_eq!(
        parser.callbacks().application.as_deref(),
        Some("fragmented 🦀")
    );
    assert_eq!(parser.callbacks().revision, 1);
    parser.process("\x1b]0;next title\x1b\\".as_bytes());
    assert_eq!(
        parser.callbacks().application.as_deref(),
        Some("next title")
    );
    assert_eq!(parser.callbacks().revision, 2);
    parser.process(b"\x1b]1;icon only\x07");
    assert_eq!(
        parser.callbacks().application.as_deref(),
        Some("next title")
    );
    assert_eq!(parser.callbacks().revision, 2);
    parser.process("\x1b]0;next title\x07".as_bytes());
    assert_eq!(parser.callbacks().revision, 2);
    parser.process(format!("\x1b]2;{}\x07", "🦀".repeat(300)).as_bytes());
    assert_eq!(
        parser
            .callbacks()
            .application
            .as_ref()
            .unwrap()
            .chars()
            .count(),
        MAX_TITLE_CHARS
    );
    assert_eq!(parser.callbacks().revision, 3);
    assert_eq!(
        sanitize_title("  hi\n\t\u{202e}there  ").as_deref(),
        Some("hithere")
    );
    parser.process(b"\x1b]2;   \x07");
    assert_eq!(parser.callbacks().application, None);
    assert_eq!(parser.callbacks().revision, 4);
    parser.process(b"\x1b]2;semi;colon\x07");
    assert_eq!(
        parser.callbacks().application.as_deref(),
        Some("semi;colon")
    );
    parser.process(b"\x1b]2;bad\xfftitle\x07");
    assert_eq!(parser.callbacks().application.as_deref(), Some("bad�title"));
    assert_eq!(parser.callbacks().revision, 6);
}
