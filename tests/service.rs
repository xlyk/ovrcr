use clap::{Parser, Subcommand};
use ovrcr::protocol::{ClientMessage, Request, Response, ServerMessage, read_frame, write_frame};
use ovrcr::server::ServerPaths;
use ovrcr::service::{
    ServiceArgs, ServiceCommand, ServiceConfig, ServicePlatform, read_environment_file, run_with,
    start_if_installed_with,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::thread;
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    config: ServiceConfig,
    log: PathBuf,
}

impl Fixture {
    fn new(platform: ServicePlatform) -> Self {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("bin/ovrcr");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "fixture").unwrap();
        let log = root.path().join("manager.log");
        let active = root.path().join("manager.active");
        let manager = root.path().join("fake-manager");
        fs::write(
            &manager,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n\
                 if [ \"$1\" = print ]; then [ -f '{active}' ] || exit 1; printf '\\tpid = '; cat '{active}'; exit 0; fi\n\
                 if [ \"$2\" = show ]; then cat '{active}'; exit $?; fi\n\
                 if [ \"$2\" = is-active ]; then [ -f '{}' ]; exit $?; fi\n\
                 case \"$1 $2\" in\n\
                 'bootstrap '*|'kickstart '*|'--user start'|'--user restart') : > '{}' ;;\n\
                 'bootout '*|'--user stop'|'--user disable') /bin/rm -f '{}' ;;\n\
                 esac\n",
                log.display(),
                active.display(),
                active.display(),
                active.display(),
                active = active.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&manager).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&manager, permissions).unwrap();
        let definition_path = match platform {
            ServicePlatform::Launchd => root.path().join("LaunchAgents/com.ovrcr.server.plist"),
            ServicePlatform::Systemd => root.path().join("systemd/user/ovrcr.service"),
        };
        let config = ServiceConfig {
            platform,
            service_name: match platform {
                ServicePlatform::Launchd => "com.ovrcr.server".into(),
                ServicePlatform::Systemd => "ovrcr.service".into(),
            },
            definition_path,
            manager_executable: manager,
            executable,
            registry_path: root.path().join("config/config.toml"),
            server_paths: ServerPaths {
                socket: root.path().join("run/server.sock"),
            },
            environment: vec![
                ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
                ("SHELL".into(), "/bin/zsh".into()),
                ("HOME".into(), root.path().display().to_string()),
                (
                    "PI_CODING_AGENT_DIR".into(),
                    root.path().join("pi").display().to_string(),
                ),
            ],
        };
        Self { root, config, log }
    }

    fn manager_log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn install_stopped(&self) {
        run_with(
            &self.config,
            ServiceCommand::Install {
                environment_file: None,
                kill_sessions: false,
            },
            false,
        )
        .unwrap();
        fs::remove_file(self.root.path().join("manager.active")).unwrap();
        fs::write(&self.log, "").unwrap();
    }

    fn mark_managed(&self) {
        fs::write(
            self.root.path().join("manager.active"),
            std::process::id().to_string(),
        )
        .unwrap();
    }
}

#[derive(Parser)]
struct TestCli {
    #[command(subcommand)]
    command: TestCommand,
}

#[derive(Subcommand)]
enum TestCommand {
    Service(ServiceArgs),
}

#[test]
fn service_args_parse_install_environment_file() {
    let cli = TestCli::try_parse_from([
        "ovrcr",
        "service",
        "install",
        "--environment-file",
        "/tmp/ovrcr.env",
    ])
    .unwrap();
    let TestCommand::Service(args) = cli.command;
    assert!(
        matches!(args.command, ServiceCommand::Install { environment_file: Some(path), kill_sessions: false } if path == Path::new("/tmp/ovrcr.env"))
    );
}

#[test]
fn launchd_install_writes_login_job_that_restarts_only_after_failure() {
    let fixture = Fixture::new(ServicePlatform::Launchd);
    let environment_file = fixture.root.path().join("service.env");
    fs::write(&environment_file, "ANTHROPIC_API_KEY=from-file\n").unwrap();
    run_with(
        &fixture.config,
        ServiceCommand::Install {
            environment_file: Some(environment_file.clone()),
            kill_sessions: false,
        },
        false,
    )
    .unwrap();

    let plist = fs::read_to_string(&fixture.config.definition_path).unwrap();
    assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
    assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
    for path in [
        &fixture.config.executable,
        &fixture.config.registry_path,
        &fixture.config.server_paths.socket,
        &environment_file,
    ] {
        assert!(plist.contains(&format!("<string>{}</string>", path.display())));
    }
    assert!(plist.contains("<key>OVRCR_ENV_FILE</key>"));
    assert!(!plist.contains("ANTHROPIC_API_KEY"));
    #[cfg(target_os = "macos")]
    assert!(
        std::process::Command::new("/usr/bin/plutil")
            .args(["-lint", fixture.config.definition_path.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    assert!(fixture.manager_log().contains(&format!(
        "bootstrap gui/{} {}",
        unsafe { libc::getuid() },
        fixture.config.definition_path.display()
    )));
}

#[test]
fn systemd_install_writes_user_unit_and_enables_login_startup() {
    let fixture = Fixture::new(ServicePlatform::Systemd);
    let environment_file = fixture.root.path().join("service.env");
    fs::write(&environment_file, "ANTHROPIC_API_KEY=from-file\n").unwrap();
    run_with(
        &fixture.config,
        ServiceCommand::Install {
            environment_file: Some(environment_file.clone()),
            kill_sessions: false,
        },
        true,
    )
    .unwrap();

    let unit = fs::read_to_string(&fixture.config.definition_path).unwrap();
    assert!(unit.contains("Restart=on-failure"));
    assert!(unit.contains("WantedBy=default.target"));
    assert!(unit.contains(&format!(
        "Environment=\"OVRCR_ENV_FILE={}\"",
        environment_file.display()
    )));
    assert!(unit.contains(&format!(
        "ExecStart=\"{}\" server",
        fixture.config.executable.display()
    )));
    assert!(unit.contains(&format!(
        "Environment=\"OVRCR_CONFIG={}\"",
        fixture.config.registry_path.display()
    )));
    let log = fixture.manager_log();
    assert!(log.contains("--user daemon-reload"));
    assert!(log.contains("--user enable ovrcr.service"));
    assert!(log.contains("--user restart ovrcr.service"));
}

#[test]
fn environment_file_is_parsed_literally_in_the_application() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("service.env");
    fs::write(
        &path,
        "# comment\nTOKEN=$HOME/not-expanded\nNAME=two words\nEMPTY=\n",
    )
    .unwrap();
    assert_eq!(
        read_environment_file(&path).unwrap(),
        vec![
            ("TOKEN".into(), "$HOME/not-expanded".into()),
            ("NAME".into(), "two words".into()),
            ("EMPTY".into(), "".into()),
        ]
    );
}

#[test]
fn install_refuses_to_take_over_an_unmanaged_server() {
    let fixture = Fixture::new(ServicePlatform::Systemd);
    fs::create_dir_all(fixture.config.server_paths.socket.parent().unwrap()).unwrap();
    let _listener = UnixListener::bind(&fixture.config.server_paths.socket).unwrap();
    let error = run_with(
        &fixture.config,
        ServiceCommand::Install {
            environment_file: None,
            kill_sessions: false,
        },
        false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("unmanaged OVRCR server"));
    assert!(!fixture.config.definition_path.exists());
    assert!(!fixture.manager_log().contains("daemon-reload"));
}

#[test]
fn start_if_installed_only_uses_an_existing_service_definition() {
    let fixture = Fixture::new(ServicePlatform::Launchd);
    assert!(!start_if_installed_with(&fixture.config).unwrap());
    assert!(fixture.manager_log().is_empty());
    fixture.install_stopped();
    assert!(start_if_installed_with(&fixture.config).unwrap());
    assert!(fixture.manager_log().contains("bootstrap gui/"));
}

#[test]
fn status_queries_but_never_starts_the_service() {
    let fixture = Fixture::new(ServicePlatform::Systemd);
    fixture.install_stopped();
    run_with(&fixture.config, ServiceCommand::Status, false).unwrap();
    assert_eq!(
        fixture.manager_log(),
        "--user is-active --quiet ovrcr.service\n"
    );
}

#[test]
fn stop_requests_graceful_server_shutdown_before_stopping_the_manager() {
    for platform in [ServicePlatform::Launchd, ServicePlatform::Systemd] {
        let fixture = Fixture::new(platform);
        fixture.install_stopped();
        fixture.mark_managed();
        fs::create_dir_all(fixture.config.server_paths.socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&fixture.config.server_paths.socket).unwrap();
        let server = thread::spawn(move || {
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                let Ok(message) = read_frame::<ClientMessage>(&mut stream) else {
                    continue;
                };
                assert_eq!(message.request, Request::Shutdown { kill: true });
                write_frame(
                    &mut stream,
                    &ServerMessage::Response {
                        request_id: message.request_id,
                        response: Response::Ok,
                    },
                )
                .unwrap();
                break;
            }
        });

        run_with(&fixture.config, ServiceCommand::Stop, false).unwrap();
        server.join().unwrap();
        assert!(fixture.manager_log().contains(match platform {
            ServicePlatform::Launchd => "bootout gui/",
            ServicePlatform::Systemd => "--user stop ovrcr.service",
        }));
    }
}

#[test]
fn service_install_refuses_when_sessions_exist() {
    use ovrcr::protocol::{
        AgentActivity, ErrorCode, Registry, SessionId, SessionPhase, SessionSummary,
    };
    let fixture = Fixture::new(ServicePlatform::Systemd);
    fixture.install_stopped();
    fixture.mark_managed();
    let definition = fs::read(&fixture.config.definition_path).unwrap();
    fs::create_dir_all(fixture.config.server_paths.socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&fixture.config.server_paths.socket).unwrap();
    let server = thread::spawn(move || {
        let mut received = Vec::new();
        loop {
            let (mut stream, _) = listener.accept().unwrap();
            let Ok(message) = read_frame::<ClientMessage>(&mut stream) else {
                break;
            };
            let response = match &message.request {
                Request::Shutdown { kill: false } => Response::Error {
                    code: ErrorCode::SessionsRemain,
                    message: "sessions remain".into(),
                },
                Request::List => Response::Inventory {
                    registry: Registry::default(),
                    sessions: vec![SessionSummary {
                        id: SessionId(7),
                        project: "demo".into(),
                        workspace: "main".into(),
                        name: "shell".into(),
                        label: "shell".into(),
                        pid: Some(1),
                        started_unix_ms: 0,
                        phase: SessionPhase::Running,
                        activity: AgentActivity::Idle,
                        context_usage: None,
                    }],
                },
                other => panic!("install sent {other:?}"),
            };
            received.push(message.request);
            write_frame(
                &mut stream,
                &ServerMessage::Response {
                    request_id: message.request_id,
                    response,
                },
            )
            .unwrap();
        }
        received
    });

    let error = run_with(
        &fixture.config,
        ServiceCommand::Install {
            environment_file: None,
            kill_sessions: false,
        },
        false,
    )
    .unwrap_err();
    // The server must still be listening: an empty connection ends the fake server.
    drop(std::os::unix::net::UnixStream::connect(&fixture.config.server_paths.socket).unwrap());
    let received = server.join().unwrap();
    let text = error.to_string();
    assert!(text.contains("1 session"), "{text}");
    assert!(text.contains("--kill-sessions"), "{text}");
    assert!(received.contains(&Request::Shutdown { kill: false }));
    assert!(!received.contains(&Request::Shutdown { kill: true }));
    assert!(!fixture.manager_log().contains("restart"));
    assert_eq!(
        fs::read(&fixture.config.definition_path).unwrap(),
        definition
    );
}

#[test]
fn failed_graceful_shutdown_does_not_stop_the_manager() {
    let fixture = Fixture::new(ServicePlatform::Systemd);
    fixture.install_stopped();
    fixture.mark_managed();
    fs::create_dir_all(fixture.config.server_paths.socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&fixture.config.server_paths.socket).unwrap();
    let server = thread::spawn(move || {
        loop {
            let (mut stream, _) = listener.accept().unwrap();
            let Ok(message) = read_frame::<ClientMessage>(&mut stream) else {
                continue;
            };
            write_frame(
                &mut stream,
                &ServerMessage::Response {
                    request_id: message.request_id,
                    response: Response::Error {
                        code: ovrcr::protocol::ErrorCode::PartialFailure,
                        message: "cleanup failed".into(),
                    },
                },
            )
            .unwrap();
            break;
        }
    });

    let error = run_with(&fixture.config, ServiceCommand::Stop, false).unwrap_err();
    server.join().unwrap();
    assert!(error.to_string().contains("cleanup failed"));
    assert!(!fixture.manager_log().contains("--user stop"));
}

#[test]
fn uninstall_removes_only_service_files_and_preserves_task_data() {
    let fixture = Fixture::new(ServicePlatform::Systemd);
    let tasks = fixture.config.registry_path.with_extension("tasks");
    fs::create_dir_all(&tasks).unwrap();
    fs::write(&fixture.config.registry_path, "projects = []\n").unwrap();
    fs::write(tasks.join("state.toml"), "max_concurrent = 3\n").unwrap();
    fixture.install_stopped();
    run_with(&fixture.config, ServiceCommand::Uninstall, false).unwrap();
    assert!(!fixture.config.definition_path.exists());
    assert!(fixture.config.registry_path.exists());
    assert!(tasks.join("state.toml").exists());
    let log = fixture.manager_log();
    assert!(log.contains("--user disable --now ovrcr.service"));
    assert!(log.contains("--user daemon-reload"));
}

#[test]
fn service_definitions_reject_relative_runtime_paths() {
    let mut fixture = Fixture::new(ServicePlatform::Systemd);
    fixture.config.executable = PathBuf::from("ovrcr");
    let error = run_with(
        &fixture.config,
        ServiceCommand::Install {
            environment_file: None,
            kill_sessions: false,
        },
        false,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("executable must be an absolute path")
    );
    assert!(!fixture.config.definition_path.exists());
}

#[test]
fn loaded_service_does_not_send_shutdown_to_a_foreign_listener() {
    use std::os::unix::net::UnixStream;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for platform in [ServicePlatform::Launchd, ServicePlatform::Systemd] {
        for (pid, command) in [
            (
                "1",
                ServiceCommand::Install {
                    environment_file: None,
                    kill_sessions: false,
                },
            ),
            ("1", ServiceCommand::Stop),
            ("1", ServiceCommand::Uninstall),
            ("0", ServiceCommand::Stop),
        ] {
            let fixture = Fixture::new(platform);
            run_with(
                &fixture.config,
                ServiceCommand::Install {
                    environment_file: None,
                    kill_sessions: false,
                },
                false,
            )
            .unwrap();
            // A different native process owns the loaded job; the listener belongs to this test process.
            fs::write(fixture.root.path().join("manager.active"), pid).unwrap();
            fs::create_dir_all(fixture.config.server_paths.socket.parent().unwrap()).unwrap();
            let listener = UnixListener::bind(&fixture.config.server_paths.socket).unwrap();
            let finished = Arc::new(AtomicBool::new(false));
            let server_finished = finished.clone();
            let server = thread::spawn(move || {
                let mut received = Vec::new();
                loop {
                    let (mut stream, _) = listener.accept().unwrap();
                    if server_finished.load(Ordering::Acquire) {
                        break;
                    }
                    if let Ok(message) = read_frame::<ClientMessage>(&mut stream) {
                        received.push(message.request);
                        write_frame(
                            &mut stream,
                            &ServerMessage::Response {
                                request_id: message.request_id,
                                response: Response::Ok,
                            },
                        )
                        .unwrap();
                    }
                }
                received
            });
            let definition = fs::read(&fixture.config.definition_path).unwrap();
            let result = run_with(&fixture.config, command, false);
            finished.store(true, Ordering::Release);
            let _wake = UnixStream::connect(&fixture.config.server_paths.socket).unwrap();
            let received = server.join().unwrap();
            assert!(
                received.is_empty(),
                "foreign listener received: {received:?}"
            );
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("unmanaged OVRCR server")
            );
            assert_eq!(
                fs::read(&fixture.config.definition_path).unwrap(),
                definition
            );
        }
    }
}

#[test]
fn changed_socket_cannot_control_the_installed_job_even_without_a_listener() {
    for platform in [ServicePlatform::Launchd, ServicePlatform::Systemd] {
        let fixture = Fixture::new(platform);
        fixture.install_stopped();
        fixture.mark_managed();
        let mut config = fixture.config.clone();
        config.server_paths.socket = fixture.root.path().join("different.sock");
        let definition = fs::read(&config.definition_path).unwrap();
        for command in [
            ServiceCommand::Install {
                environment_file: None,
                kill_sessions: false,
            },
            ServiceCommand::Start,
            ServiceCommand::Stop,
            ServiceCommand::Uninstall,
        ] {
            let error = run_with(&config, command, false).unwrap_err();
            assert!(error.to_string().contains("socket does not match"));
            assert!(
                fixture.manager_log().is_empty(),
                "must refuse before controlling the installed job"
            );
            assert_eq!(fs::read(&config.definition_path).unwrap(), definition);
        }
        assert!(
            start_if_installed_with(&config)
                .unwrap_err()
                .to_string()
                .contains("socket does not match")
        );
        assert!(fixture.manager_log().is_empty());
    }
}
