//! Opt-in real socket/PTY/helper capacity acceptance; run alone on a recorded host.
use ovrcr::config::{Registry, save_registry_atomic};
use ovrcr::protocol::*;
use ovrcr::report::collector::{CollectorController, CollectorSource};
use ovrcr::server::ServerPaths;
#[cfg(not(feature = "acceptance-diagnostics"))]
use ovrcr::server::run_server;
#[cfg(feature = "acceptance-diagnostics")]
use ovrcr::server::{ServerQueueDiagnostics, run_server_with_diagnostics};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

fn connect(path: &Path) -> UnixStream {
    let s = connect_server(path).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    s
}
fn send(s: &mut UnixStream, request: Request) -> Response {
    let owner = matches!(
        request,
        Request::ReserveAgent(_) | Request::SupervisorHello(_) | Request::Shutdown { .. }
    );
    let path = s.peer_addr().unwrap().as_pathname().unwrap().to_owned();
    write_frame(
        s,
        &ClientMessage {
            request_id: 1,
            request,
        },
    )
    .unwrap();
    let response = match read_frame::<ServerMessage>(s).unwrap() {
        ServerMessage::Response {
            request_id: 1,
            response,
        } => response,
        other => panic!("unexpected {other:?}"),
    };
    if !owner {
        *s = connect(&path);
    }
    response
}
struct Fixture {
    root: tempfile::TempDir,
    socket: PathBuf,
    server: Option<thread::JoinHandle<()>>,
    #[cfg(feature = "acceptance-diagnostics")]
    diagnostics: ServerQueueDiagnostics,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Load Fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&repo)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        fs::write(repo.join("README"), "fixture").unwrap();
        for args in [["add", "README"], ["commit", "-mfixture"]] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&repo)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        let socket = root.path().join("server.sock");
        let registry = root.path().join("config.toml");
        save_registry_atomic(&Registry::default(), &registry).unwrap();
        let paths = ServerPaths {
            socket: socket.clone(),
        };
        #[cfg(feature = "acceptance-diagnostics")]
        let diagnostics = ServerQueueDiagnostics::default();
        #[cfg(feature = "acceptance-diagnostics")]
        let server_diagnostics = diagnostics.clone();
        #[cfg(feature = "acceptance-diagnostics")]
        let server = Some(thread::spawn(move || {
            run_server_with_diagnostics(paths, registry, server_diagnostics).unwrap()
        }));
        #[cfg(not(feature = "acceptance-diagnostics"))]
        let server = Some(thread::spawn(move || run_server(paths, registry).unwrap()));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let fixture = Self {
            root,
            socket,
            server,
            #[cfg(feature = "acceptance-diagnostics")]
            diagnostics,
        };
        let mut control = connect(&fixture.socket);
        assert_eq!(
            send(
                &mut control,
                Request::AddProject {
                    name: "load".into(),
                    repo,
                    workspace_root: fixture.root.path().join("workspaces")
                }
            ),
            Response::Ok
        );
        assert_eq!(
            send(
                &mut control,
                Request::CreateWorkspace {
                    project: "load".into(),
                    name: "work".into(),
                    branch: BranchRequest::New {
                        branch: "load".into(),
                        base: "main".into()
                    }
                }
            ),
            Response::Ok
        );
        let Response::Inventory { sessions, .. } = send(&mut control, Request::Inspect) else {
            panic!("initial inventory");
        };
        for session in sessions {
            assert_eq!(
                send(
                    &mut control,
                    Request::KillSession {
                        session: session.id
                    }
                ),
                Response::Ok
            );
        }
        fixture
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let mut s = connect(&self.socket);
        assert_eq!(send(&mut s, Request::Shutdown { kill: true }), Response::Ok);
        self.server.take().unwrap().join().unwrap();
    }
}
fn measure<T>(value: T) -> Measurement<T> {
    Measurement {
        value,
        source: "synthetic-load".into(),
        source_revision: None,
        source_sequence: None,
        freshness: MeasurementFreshness::Uncertain,
    }
}
fn sample(index: usize, tick: u64, usage: UsageTotals) -> MetricsSample {
    MetricsSample {
        model: Some(format!("session-{index}")),
        context: measure(ContextSample {
            used_tokens: Some(tick),
            capacity_tokens: Some(10000),
            quality: SampleQuality::Observed,
        }),
        usage: measure(usage),
        cost: measure(None),
    }
}
// ps is outside the measured request path. Include the test/server process and
// every descendant (PTY shells, helpers and ps), not just the daemon thread.
fn resources() -> (u64, f64, usize) {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss=,%cpu="])
        .output()
        .unwrap();
    assert!(output.status.success());
    let rows: Vec<_> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let x: Vec<_> = line.split_whitespace().collect();
            Some((
                x.first()?.parse::<u32>().ok()?,
                x.get(1)?.parse::<u32>().ok()?,
                x.get(2)?.parse::<u64>().ok()?,
                x.get(3)?.parse::<f64>().ok()?,
            ))
        })
        .collect();
    let mut ids = std::collections::HashSet::from([std::process::id()]);
    loop {
        let before = ids.len();
        for (pid, ppid, _, _) in &rows {
            if ids.contains(ppid) {
                ids.insert(*pid);
            }
        }
        if before == ids.len() {
            break;
        }
    }
    let owned: Vec<_> = rows.iter().filter(|r| ids.contains(&r.0)).collect();
    (
        owned.iter().map(|r| r.2).sum(),
        owned.iter().map(|r| r.3).sum(),
        owned.len(),
    )
}
#[test]
#[ignore = "50 real PTYs/helpers, production indices, 60 seconds; run explicitly alone"]
fn fifty_session_reporting_capacity() {
    let evidence = PathBuf::from(
        std::env::var_os("OVRCR_LOAD_EVIDENCE").expect("set isolated evidence directory"),
    );
    fs::create_dir_all(&evidence).unwrap();
    let mut resource_log = File::create(evidence.join("resources.csv")).unwrap();
    writeln!(resource_log, "phase,second,rss_kib,cpu_percent,processes").unwrap();
    let fixture = Fixture::new();
    let mut control = connect(&fixture.socket);
    let mut identities = Vec::new();
    let mut owned_pids = File::create(evidence.join("owned-pids.csv")).unwrap();
    writeln!(owned_pids, "kind,index,pid").unwrap();
    let mut pty_pids = Vec::new();
    for index in 0..50 {
        let path = fixture.root.path().join(format!("identity-{index}"));
        let Response::CreatedSession(session) = send(&mut control, Request::CreateSession(CreateSessionRequest {
            project: "load".into(), workspace: "work".into(), name: format!("load-{index}"), label: None,
            argv: vec!["sh".into(), "-c".into(), "printf '%s' \"$OVRCR_HOOK_TOKEN\" > \"$1\"; while IFS= read -r line; do :; done".into(), "load".into(), path.clone().into_os_string()],
        })) else { panic!("create session"); };
        let deadline = Instant::now() + Duration::from_secs(5);
        let token = loop {
            if let Ok(token) = fs::read_to_string(&path)
                && token.len() == 64
            {
                break token;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        };
        let mut capability = [0; 32];
        for (i, v) in capability.iter_mut().enumerate() {
            *v = u8::from_str_radix(&token[i * 2..i * 2 + 2], 16).unwrap();
        }
        let pid = session.pid.expect("live PTY PID");
        pty_pids.push(pid);
        writeln!(owned_pids, "pty,{index},{pid}").unwrap();
        identities.push((session.id, capability));
    }
    #[cfg(feature = "acceptance-diagnostics")]
    let dashboard_session = identities[0].0;
    #[cfg(feature = "acceptance-diagnostics")]
    let (dashboard_shutdown, dashboard_reader) = {
        let mut dashboard = connect(&fixture.socket);
        write_frame(
            &mut dashboard,
            &ClientMessage {
                request_id: 1,
                request: Request::DashboardHello,
            },
        )
        .unwrap();
        assert!(matches!(
            read_frame::<ServerMessage>(&mut dashboard).unwrap(),
            ServerMessage::Response {
                request_id: 1,
                response: Response::Hierarchy(_)
            }
        ));
        write_frame(
            &mut dashboard,
            &ClientMessage {
                request_id: 2,
                request: Request::SetView {
                    view: DashboardView {
                        revision: 1,
                        panes: vec![PaneTarget {
                            session: dashboard_session,
                            size: TerminalSize { rows: 24, cols: 80 },
                        }],
                        focused: Some(dashboard_session),
                    },
                },
            },
        )
        .unwrap();
        loop {
            if matches!(
                read_frame::<ServerMessage>(&mut dashboard).unwrap(),
                ServerMessage::Response {
                    request_id: 2,
                    response: Response::Ok
                }
            ) {
                break;
            }
        }
        dashboard.set_read_timeout(None).unwrap();
        let shutdown = dashboard.try_clone().unwrap();
        let reader = thread::spawn(move || {
            let mut frames = 0usize;
            let mut output_frames = 0usize;
            while let Ok(message) = read_frame::<ServerMessage>(&mut dashboard) {
                frames += 1;
                if matches!(
                    message,
                    ServerMessage::Event(
                        ServerEvent::Output { .. } | ServerEvent::ScreenDirty { .. }
                    )
                ) {
                    output_frames += 1;
                }
            }
            (frames, output_frames)
        });
        (shutdown, reader)
    };
    let baseline = resources();
    writeln!(
        resource_log,
        "empty50,0,{},{},{}",
        baseline.0, baseline.1, baseline.2
    )
    .unwrap();
    let source = fixture.root.path().join("production.jsonl");
    let mut file = BufWriter::new(File::create(&source).unwrap());
    for id in 0..65_537 {
        writeln!(file, "{}", serde_json::json!({"type":"assistant","sessionId":"root","isSidechain":false,"requestId":id.to_string(),"message":{"id":id.to_string(),"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}})).unwrap();
    }
    file.flush().unwrap();
    drop(file);
    let mut helpers: Vec<_> = (0..50)
        .map(|_| {
            CollectorController::spawn(
                Path::new(env!("CARGO_BIN_EXE_ovrcr")),
                CollectorSource {
                    path: source.clone(),
                    conversation: "root".into(),
                },
            )
            .unwrap()
        })
        .collect();
    for (index, helper) in helpers.iter().enumerate() {
        writeln!(
            owned_pids,
            "collector,{index},{}",
            helper.process_id().unwrap()
        )
        .unwrap();
    }
    owned_pids.flush().unwrap();
    let mut ready = vec![None; 50];
    let fill_start = Instant::now();
    let mut fill_second = u64::MAX;
    let mut fill_peak = baseline.0;
    let deadline = fill_start + Duration::from_secs(120);
    while ready.iter().any(Option::is_none) {
        for (index, helper) in helpers.iter_mut().enumerate() {
            if ready[index].is_none()
                && let Some(s) = helper.advance().unwrap()
            {
                assert!(s.retained_identities <= 65_536 && s.retained_bytes <= 16 * 1024 * 1024);
                if s.diagnostic.is_some() {
                    assert_eq!(s.diagnostic.as_deref(), Some("accounting_limit"));
                    ready[index] = Some(s);
                }
            }
        }
        let second = fill_start.elapsed().as_secs();
        if second != fill_second {
            let r = resources();
            fill_peak = fill_peak.max(r.0);
            writeln!(resource_log, "fill,{second},{},{},{}", r.0, r.1, r.2).unwrap();
            resource_log.flush().unwrap();
            assert!(
                fill_peak.saturating_sub(baseline.0) < 3 * 1024 * 1024,
                "RSS budget during fill"
            );
            fill_second = second;
        }
        assert!(Instant::now() < deadline, "production fill deadline");
        thread::sleep(Duration::from_millis(1));
    }
    let mut bounds = File::create(evidence.join("accounting.csv")).unwrap();
    writeln!(bounds, "session,identities,charged_bytes,diagnostic").unwrap();
    for (index, snapshot) in ready.iter().enumerate() {
        let s = snapshot.as_ref().unwrap();
        assert_eq!(s.retained_identities, 65_536);
        writeln!(
            bounds,
            "{index},{},{},{}",
            s.retained_identities,
            s.retained_bytes,
            s.diagnostic.as_ref().unwrap()
        )
        .unwrap();
    }
    #[cfg(feature = "acceptance-diagnostics")]
    {
        let mut ipc = File::create(evidence.join("collector-queues.csv")).unwrap();
        writeln!(
            ipc,
            "collector,pending_items,pending_bytes,peak_items,peak_bytes,rejected"
        )
        .unwrap();
        for (index, helper) in helpers.iter().enumerate() {
            let snapshot = helper.reporting_snapshot();
            assert!(snapshot.pending_items <= 1);
            assert!(snapshot.peak_items <= 1);
            assert!(snapshot.peak_bytes <= 65_536 + 4);
            writeln!(
                ipc,
                "{index},{},{},{},{},{}",
                snapshot.pending_items,
                snapshot.pending_bytes,
                snapshot.peak_items,
                snapshot.peak_bytes,
                snapshot.rejected
            )
            .unwrap();
        }
    }
    let full = resources();
    writeln!(resource_log, "full50,0,{},{},{}", full.0, full.1, full.2).unwrap();
    assert!(
        full.0.saturating_sub(baseline.0) < 3 * 1024 * 1024,
        "RSS budget before load"
    );
    let start = Instant::now() + Duration::from_secs(2);
    let mut workers = Vec::new();
    for (index, ((session, capability), mut helper)) in
        identities.into_iter().zip(helpers).enumerate()
    {
        let socket = fixture.socket.clone();
        let usage = ready[index].take().unwrap().usage;
        workers.push(thread::spawn(move || {
            let mut s = connect(&socket);
            let Response::AgentOperation(AgentOperationResult::Reserved(reservation)) = send(
                &mut s,
                Request::ReserveAgent(ReserveAgent {
                    session,
                    capability: AgentSecret(capability),
                    operation: "reserve".into(),
                    expected_epoch: 0,
                    invocation: format!("inv-{index}"),
                    provider: AgentProvider::Claude,
                }),
            ) else {
                panic!("reserve");
            };
            let auth = SupervisorAuth {
                session,
                lease: reservation.lease,
            };
            let mut owner = connect(&socket);
            assert_eq!(
                send(&mut owner, Request::SupervisorHello(auth.clone())),
                Response::Ok
            );
            s = connect(&socket);
            let Response::AgentOperation(AgentOperationResult::Bound(binding)) = send(
                &mut s,
                Request::Supervisor(SupervisorRequest {
                    auth: auth.clone(),
                    operation: "bind".into(),
                    command: AgentCommand::Bind {
                        expected_binding: None,
                        conversation: format!("root-{index}"),
                    },
                }),
            ) else {
                panic!("bind");
            };
            let mut max_late = 0;
            for tick in 1..=600 {
                let scheduled = start + Duration::from_millis((tick - 1) * 100);
                thread::sleep(scheduled.saturating_duration_since(Instant::now()));
                max_late = max_late.max(
                    Instant::now()
                        .saturating_duration_since(scheduled)
                        .as_micros(),
                );
                let report = |binding| {
                    Request::AgentReport(AgentReport {
                        session,
                        capability,
                        sequence: None,
                        update: AgentUpdate::Provider(ProviderReport {
                            binding,
                            revision: tick,
                            observation: AgentObservation::Metrics(Box::new(sample(
                                index,
                                tick,
                                usage.clone(),
                            ))),
                        }),
                    })
                };
                assert_eq!(send(&mut s, report(binding.clone())), Response::Ok);
                if tick % 10 == 0 {
                    let mut stale = binding.clone();
                    stale.invocation = "stale".into();
                    assert!(matches!(
                        send(&mut s, report(stale)),
                        Response::Error { .. }
                    ));
                }
                if tick == 300 && index < 5 {
                    unsafe {
                        assert_eq!(
                            libc::kill(-(helper.process_id().unwrap() as i32), libc::SIGKILL),
                            0
                        );
                    }
                    let deadline = Instant::now() + Duration::from_secs(2);
                    loop {
                        if helper.advance().is_err() {
                            break;
                        }
                        assert!(Instant::now() < deadline);
                        thread::yield_now();
                    }
                }
                if tick == 1 || tick == 300 && index < 5 {
                    assert_eq!(
                        send(
                            &mut s,
                            Request::Supervisor(SupervisorRequest {
                                auth: auth.clone(),
                                operation: format!("health-{tick}"),
                                command: AgentCommand::Health(ProviderReport {
                                    binding: binding.clone(),
                                    revision: tick,
                                    observation: AgentObservation::Health(HealthSample {
                                        state: ReporterHealth::Unavailable,
                                        reason: Some(
                                            if tick == 300 {
                                                "collector_lost"
                                            } else {
                                                "accounting_limit"
                                            }
                                            .into()
                                        )
                                    })
                                })
                            })
                        ),
                        Response::AgentOperation(AgentOperationResult::HealthUpdated)
                    );
                }
            }
            assert!(
                Instant::now() < start + Duration::from_secs(60),
                "600 updates missed the 60-second window"
            );
            thread::sleep(
                (start + Duration::from_secs(60)).saturating_duration_since(Instant::now()),
            );
            let pid = helper.process_id().unwrap() as i32;
            #[cfg(feature = "acceptance-diagnostics")]
            let collector_snapshot = helper.reporting_snapshot();
            let result = helper.cancel(Instant::now() + Duration::from_secs(2));
            #[cfg(feature = "acceptance-diagnostics")]
            let drained = helper.reporting_snapshot();
            drop(helper);
            let deadline = Instant::now() + Duration::from_secs(3);
            while unsafe { libc::kill(pid, 0) } == 0 {
                assert!(Instant::now() < deadline, "owned helper {pid} not reaped");
                thread::yield_now();
            }
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
            println!("collector={index} pid={pid} reaped cancel={result:?}");
            result.unwrap();
            #[cfg(feature = "acceptance-diagnostics")]
            return (index, max_late, owner, collector_snapshot, drained);
            #[cfg(not(feature = "acceptance-diagnostics"))]
            (index, max_late, owner)
        }));
    }
    let mut latency = File::create(evidence.join("latency.csv")).unwrap();
    writeln!(latency, "sample,elapsed_us").unwrap();
    #[cfg(feature = "acceptance-diagnostics")]
    let mut queue_log = File::create(evidence.join("queues.csv")).unwrap();
    #[cfg(feature = "acceptance-diagnostics")]
    writeln!(
        queue_log,
        "second,queue,pending_items,pending_bytes,peak_items,peak_bytes,rejected"
    )
    .unwrap();
    thread::sleep(start.saturating_duration_since(Instant::now()));
    let mut times = Vec::new();
    let mut peak = full.0.max(fill_peak);
    let mut last_second = u64::MAX;
    while start.elapsed() < Duration::from_secs(60) {
        let now = Instant::now();
        let Response::Inventory { sessions, .. } = send(&mut control, Request::Inspect) else {
            panic!("inspect");
        };
        let micros = now.elapsed().as_micros();
        writeln!(latency, "{},{}", times.len(), micros).unwrap();
        times.push(micros);
        let sessions: Vec<_> = sessions
            .into_iter()
            .filter(|s| s.name.starts_with("load-"))
            .collect();
        assert_eq!(sessions.len(), 50);
        for session in sessions {
            if let Some(agent) = session.agent
                && let Some(metrics) = agent.metrics
            {
                assert_eq!(
                    metrics.sample.model,
                    Some(session.name.replacen("load-", "session-", 1))
                );
                assert_eq!(
                    agent.binding.conversation,
                    session.name.replacen("load-", "root-", 1)
                );
                assert_eq!(metrics.sample.usage.value.input_tokens, Some(65_536));
                if start.elapsed() > Duration::from_secs(1) {
                    assert_eq!(agent.health.state, ReporterHealth::Unavailable);
                }
            }
        }
        let second = start.elapsed().as_secs();
        if second != last_second {
            #[cfg(feature = "acceptance-diagnostics")]
            assert_eq!(
                send(
                    &mut control,
                    Request::SendTerminal {
                        session: dashboard_session,
                        text: "dashboard-load".into(),
                        submit: true,
                    }
                ),
                Response::Ok
            );
            let r = resources();
            peak = peak.max(r.0);
            writeln!(resource_log, "load,{second},{},{},{}", r.0, r.1, r.2).unwrap();
            resource_log.flush().unwrap();
            #[cfg(feature = "acceptance-diagnostics")]
            for (name, snapshot) in [
                ("raw-events", fixture.diagnostics.raw_events.snapshot()),
                ("dispatcher", fixture.diagnostics.dispatcher.snapshot()),
            ] {
                writeln!(
                    queue_log,
                    "{second},{name},{},{},{},{},{}",
                    snapshot.pending_items,
                    snapshot.pending_bytes,
                    snapshot.peak_items,
                    snapshot.peak_bytes,
                    snapshot.rejected
                )
                .unwrap();
                assert!(snapshot.pending_items <= 64);
            }
            #[cfg(feature = "acceptance-diagnostics")]
            {
                let dashboard = fixture
                    .diagnostics
                    .dashboard
                    .snapshot()
                    .expect("active dashboard diagnostics");
                writeln!(
                    queue_log,
                    "{second},dashboard,{},{},{},{},{}",
                    dashboard.pending_items,
                    dashboard.pending_bytes,
                    dashboard.peak_items,
                    dashboard.peak_bytes,
                    dashboard.rejected
                )
                .unwrap();
                assert!(dashboard.message_items <= 64);
                assert!(dashboard.terminal_items <= 1);
                assert!(dashboard.dirty_items <= 2);
            }
            #[cfg(feature = "acceptance-diagnostics")]
            queue_log.flush().unwrap();
            assert!(
                peak.saturating_sub(baseline.0) < 3 * 1024 * 1024,
                "RSS budget"
            );
            last_second = second;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let mut owners = Vec::new();
    #[cfg(feature = "acceptance-diagnostics")]
    let mut collector_log = File::create(evidence.join("collector-final.csv")).unwrap();
    #[cfg(feature = "acceptance-diagnostics")]
    writeln!(
        collector_log,
        "collector,phase,pending_items,pending_bytes,peak_items,peak_bytes,rejected"
    )
    .unwrap();
    for worker in workers {
        #[cfg(feature = "acceptance-diagnostics")]
        let (index, late, owner, before_cancel, drained) = worker.join().unwrap();
        #[cfg(not(feature = "acceptance-diagnostics"))]
        let (index, late, owner) = worker.join().unwrap();
        println!("session={index} reports=600 stale_rejected=60 max_schedule_late_us={late}");
        #[cfg(feature = "acceptance-diagnostics")]
        for (phase, snapshot) in [("before-cancel", before_cancel), ("drained", drained)] {
            writeln!(
                collector_log,
                "{index},{phase},{},{},{},{},{}",
                snapshot.pending_items,
                snapshot.pending_bytes,
                snapshot.peak_items,
                snapshot.peak_bytes,
                snapshot.rejected
            )
            .unwrap();
            assert!(snapshot.peak_items <= 1);
            assert!(snapshot.peak_bytes <= 65_536 + 4);
            if phase == "drained" {
                assert_eq!(snapshot.pending_items, 0);
                assert_eq!(snapshot.pending_bytes, 0);
            }
        }
        owners.push(owner);
    }
    let Response::Inventory { sessions, .. } = send(&mut control, Request::Inspect) else {
        panic!("final inventory");
    };
    let sessions: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.name.starts_with("load-"))
        .collect();
    assert_eq!(sessions.len(), 50);
    for session in sessions {
        let index: usize = session.name.strip_prefix("load-").unwrap().parse().unwrap();
        let agent = session.agent.expect("all 50 bindings must remain owned");
        assert_eq!(agent.metrics_revision, 600);
        assert_eq!(agent.binding.conversation, format!("root-{index}"));
        let metrics = agent.metrics.unwrap();
        assert_eq!(metrics.sample.model, Some(format!("session-{index}")));
        assert_eq!(metrics.sample.context.value.used_tokens, Some(600));
        assert_eq!(agent.health.state, ReporterHealth::Unavailable);
        assert_eq!(
            agent.health.reason.as_deref(),
            Some(if index < 5 {
                "collector_lost"
            } else {
                "accounting_limit"
            })
        );
    }
    times.sort_unstable();
    let p99 = times[(times.len() * 99).div_ceil(100) - 1];
    let max = *times.last().unwrap();
    println!(
        "reports=30000 seconds=60 collectors=50 failed_collectors=5 controls={} p99_us={p99} max_us={max} baseline_kib={} peak_kib={peak} growth_kib={}",
        times.len(),
        baseline.0,
        peak.saturating_sub(baseline.0)
    );
    assert!(p99 < 250_000 && max < 1_000_000, "control latency budget");
    #[cfg(feature = "acceptance-diagnostics")]
    {
        let events = fixture.diagnostics.raw_events.snapshot();
        let dispatcher = fixture.diagnostics.dispatcher.snapshot();
        assert!(events.peak_items <= 64 && dispatcher.peak_items <= 64);
        assert!(events.peak_bytes <= 64 * 8192);
        let dashboard = fixture
            .diagnostics
            .dashboard
            .snapshot()
            .expect("dashboard remains active through final sample");
        assert!(dashboard.peak_items > 0, "dashboard was not exercised");
        assert!(dashboard.message_items <= 64);
    }
    #[cfg(feature = "acceptance-diagnostics")]
    {
        dashboard_shutdown
            .shutdown(std::net::Shutdown::Both)
            .unwrap();
        let (dashboard_frames, dashboard_output_frames) = dashboard_reader.join().unwrap();
        assert!(
            dashboard_frames > 0,
            "dashboard received no frames under load"
        );
        assert!(
            dashboard_output_frames > 0,
            "dashboard received no output delivery under load"
        );
        println!(
            "dashboard_frames={dashboard_frames} dashboard_output_frames={dashboard_output_frames}"
        );
    }
    drop(owners);
    drop(fixture);
    for pid in pty_pids {
        assert_eq!(
            unsafe { libc::kill(pid as i32, 0) },
            -1,
            "owned PTY remains"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    println!("all 50 owned PTY PIDs absent after server shutdown");
}

// Linux-only execution, but keep this compiled on macOS so ordinary checks also
// type-check the opt-in fixture. No high-water test runs without --ignored.
#[test]
#[ignore = "isolated Linux runner only: 50 collectors at raw/index limits"]
fn fifty_collector_raw_index_and_sequential_json_high_water() {
    assert_eq!(
        std::env::consts::OS,
        "linux",
        "Linux evidence requires /proc"
    );
    let evidence = PathBuf::from(
        std::env::var_os("OVRCR_MEMORY_EVIDENCE").expect("set isolated memory evidence directory"),
    );
    fs::create_dir_all(&evidence).unwrap();
    let root = tempfile::tempdir().unwrap();
    memory_preflight(&evidence, root.path());
    let root_path = root.path().to_owned();
    let source = root.path().join("shared.jsonl");
    File::create(&source).unwrap();
    let mut helpers: Vec<_> = (0..50)
        .map(|_| {
            CollectorController::spawn(
                Path::new(env!("CARGO_BIN_EXE_ovrcr")),
                CollectorSource {
                    path: source.clone(),
                    conversation: "root".into(),
                },
            )
            .unwrap()
        })
        .collect();
    let pids: Vec<_> = helpers.iter().map(|h| h.process_id().unwrap()).collect();
    for pid in &pids {
        assert_eq!(unsafe { libc::getpgid(*pid as i32) }, *pid as i32);
    }
    let mut log = File::create(evidence.join("memory.csv")).unwrap();
    writeln!(log, "phase,kind,pid,rss_kib,vmhwm_kib").unwrap();
    memory_eof_barrier(&mut helpers, 0, 0, &mut log, &pids, "empty");
    memory_sample(&mut log, &pids, "empty50");

    let mut file = BufWriter::new(fs::OpenOptions::new().append(true).open(&source).unwrap());
    for id in 0..65_536 {
        // 112 fixed bytes + two 72-byte IDs = 256 charged bytes per entry.
        writeln!(file, "{}", memory_usage_record(id)).unwrap();
    }
    file.flush().unwrap();
    memory_eof_barrier(
        &mut helpers,
        65_536,
        16 * 1024 * 1024,
        &mut log,
        &pids,
        "index",
    );
    memory_sample(&mut log, &pids, "index50");

    // A new identity must be rejected only AFTER the complete valid JSON is
    // parsed. That diagnostic proves the held partial record was not discarded.
    let mut prefix = memory_usage_record(65_536).to_string();
    assert_eq!(prefix.pop(), Some('}'));
    prefix.push_str(",\"nested\":");
    prefix.push_str(&"[".repeat(96));
    prefix.push('0');
    prefix.push_str(&"]".repeat(96));
    prefix.push_str(",\"fields\":{");
    for field in 0..65_536 {
        if field != 0 {
            prefix.push(',');
        }
        use std::fmt::Write as _;
        write!(prefix, "\"f{field:05}\":0").unwrap();
    }
    prefix.push_str("},\"padding\":\"");
    const RAW_BYTES: usize = 32 * 1024 * 1024;
    const SUFFIX: &[u8] = b"\"}";
    assert!(prefix.len() + SUFFIX.len() < RAW_BYTES);
    file.write_all(prefix.as_bytes()).unwrap();
    let mut remaining = RAW_BYTES - prefix.len() - SUFFIX.len();
    let padding = [b'x'; 65_536];
    while remaining != 0 {
        let count = remaining.min(padding.len());
        file.write_all(&padding[..count]).unwrap();
        remaining -= count;
    }
    file.write_all(SUFFIX).unwrap();
    file.flush().unwrap();
    drop(prefix);
    memory_eof_barrier(
        &mut helpers,
        65_536,
        16 * 1024 * 1024,
        &mut log,
        &pids,
        "raw",
    );
    memory_sample(&mut log, &pids, "raw32m_index16m_all50");

    // Stop requesting reads at each EOF acknowledgement. All 50 now retain the
    // unterminated record. A newline releases parsing, one helper at a time.
    file.write_all(b"\n").unwrap();
    file.flush().unwrap();
    drop(file);
    for (index, helper) in helpers.iter_mut().enumerate() {
        memory_safety_check();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut next_sample = Instant::now();
        loop {
            if let Some(snapshot) = helper.advance().unwrap() {
                assert_eq!(snapshot.diagnostic.as_deref(), Some("accounting_limit"));
                assert_eq!(snapshot.retained_identities, 65_536);
                assert_eq!(snapshot.retained_bytes, 16 * 1024 * 1024);
                assert_eq!(snapshot.usage.input_tokens, Some(65_536));
                assert_eq!(snapshot.usage.output_tokens, Some(65_536));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "JSON parse deadline, collector {index}"
            );
            if Instant::now() >= next_sample {
                memory_sample(&mut log, &pids, &format!("parsing_{index}"));
                next_sample = Instant::now() + Duration::from_millis(250);
            }
            thread::sleep(Duration::from_millis(2));
        }
        memory_sample(&mut log, &pids, &format!("parsed_{index}"));
    }
    for helper in &mut helpers {
        helper
            .cancel(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }
    for pid in pids {
        assert_eq!(
            unsafe { libc::kill(-(pid as i32), 0) },
            -1,
            "owned group remains"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    drop(helpers);
    drop(root);
    assert!(!root_path.exists());
    writeln!(log, "cleanup,all50_groups_and_source_absent,0,0,0").unwrap();
    log.flush().unwrap();
}

fn memory_usage_record(id: usize) -> serde_json::Value {
    let id = format!("{id:072}");
    serde_json::json!({"type":"assistant","sessionId":"root","isSidechain":false,
        "requestId":id,"message":{"id":id,"usage":{"input_tokens":1,"output_tokens":1,
        "cache_creation_input_tokens":0,"cache_read_input_tokens":0}}})
}

fn memory_available_kib() -> u64 {
    let info =
        fs::read_to_string("/proc/meminfo").expect("Linux memory preflight requires /proc/meminfo");
    memory_field(&info, "MemAvailable:")
}

fn memory_field(info: &str, key: &str) -> u64 {
    let mut fields = info
        .lines()
        .find(|line| line.starts_with(key))
        .unwrap_or_else(|| panic!("missing {key}"))
        .split_whitespace();
    assert_eq!(fields.next(), Some(key));
    let value = fields.next().unwrap().parse().unwrap();
    assert_eq!(fields.next(), Some("kB"));
    value
}

fn memory_preflight(evidence: &Path, source_directory: &Path) {
    use std::os::unix::ffi::OsStrExt as _;
    let path = std::ffi::CString::new(source_directory.as_os_str().as_bytes()).unwrap();
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::statvfs(path.as_ptr(), &mut stat) }, 0);
    let available_disk = u128::from(stat.f_bavail) * u128::from(stat.f_frsize);
    let available_ram = memory_available_kib();
    fs::write(evidence.join("preflight.txt"), format!(
        "MemAvailable_kib={available_ram}\navailable_disk_bytes={available_disk}\nrequired_ram_kib={}\nrequired_disk_bytes={}\n",
        6 * 1024 * 1024, 512 * 1024 * 1024
    )).unwrap();
    assert!(
        available_ram >= 6 * 1024 * 1024,
        "memory preflight requires 6 GiB currently available RAM"
    );
    assert!(
        available_disk >= 512 * 1024 * 1024,
        "memory preflight requires 512 MiB free disk"
    );
}

fn memory_safety_check() {
    assert!(
        memory_available_kib() >= 1024 * 1024,
        "memory safety stop: less than 1 GiB available; owned helper guards will clean up"
    );
}

fn memory_sample(log: &mut File, pids: &[u32], phase: &str) {
    memory_safety_check();
    let sampled = resources().0;
    writeln!(log, "{phase},sampled_process_tree,0,{sampled},").unwrap();
    for pid in std::iter::once(std::process::id()).chain(pids.iter().copied()) {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        writeln!(
            log,
            "{phase},process,{pid},{},{}",
            memory_field(&status, "VmRSS:"),
            memory_field(&status, "VmHWM:")
        )
        .unwrap();
    }
    log.flush().unwrap();
}

fn memory_eof_barrier(
    helpers: &mut [CollectorController],
    identities: usize,
    charged: usize,
    log: &mut File,
    pids: &[u32],
    phase: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut ready = vec![false; helpers.len()];
    let mut next_sample = Instant::now();
    while ready.iter().any(|ready| !ready) {
        for (index, helper) in helpers.iter_mut().enumerate() {
            if !ready[index]
                && let Some(snapshot) = helper.advance().unwrap()
            {
                assert!(
                    snapshot.diagnostic.is_none(),
                    "{phase}: {:?}",
                    snapshot.diagnostic
                );
                if snapshot.caught_up {
                    assert_eq!(snapshot.retained_identities, identities);
                    assert_eq!(snapshot.retained_bytes, charged);
                    assert_eq!(
                        snapshot.usage.input_tokens,
                        (identities > 0).then_some(identities as u64)
                    );
                    ready[index] = true;
                }
            }
        }
        if Instant::now() >= next_sample {
            memory_sample(log, pids, phase);
            next_sample = Instant::now() + Duration::from_millis(250);
        }
        assert!(
            Instant::now() < deadline,
            "{phase}: collector EOF barrier deadline"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
