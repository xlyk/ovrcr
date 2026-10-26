#![cfg(all(feature = "acceptance-diagnostics", target_os = "macos"))]
#[path = "support/live.rs"]
mod live;
use live::{Live, PROJECT, WORKSPACE};
use ovrcr::protocol::{CreateSessionRequest, Request, Response, SessionKind, SessionSummary};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture {
    live: Live,
    log: PathBuf,
    config: PathBuf,
    workspace: PathBuf,
}
impl Fixture {
    fn new(model: &str, published: bool, allowed: bool, reject_credential: bool) -> Self {
        let live = Live::idle().bounded();
        let root = live.root.path();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        let bridge = root.join("bridge.py");
        fs::write(&bridge, include_str!("fixtures/cursor-cloud/bridge.py")).unwrap();
        fs::set_permissions(&bridge, fs::Permissions::from_mode(0o700)).unwrap();
        let log = root.join("bridge-events.jsonl");
        fs::write(
            bridge.with_extension("json"),
            json!({"log":log,"model":model,"reject_credential":reject_credential}).to_string(),
        )
        .unwrap();
        let sha = live::git(&live.repo, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        live::git(
            &live.repo,
            &["remote", "add", "origin", "https://github.com/acme/app"],
        );
        let config = root.join("cloud-fixture.json");
        fs::write(&config, json!({"schema":"ovrcr-cursor-cloud-fixture-v1", "bridge":bridge, "published_sha":if published {sha} else {"0".repeat(40)}}).to_string()).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
        let account = ovrcr::protocol::CursorCloudAccount {
            name: "work".into(),
            keychain_service: "fixture-only".into(),
            keychain_account: "work".into(),
            model: "composer-2.5".into(),
            repositories: vec![
                if allowed {
                    "https://github.com/acme/app"
                } else {
                    "https://github.com/acme/other"
                }
                .into(),
            ],
            owners: vec![],
            bridge_archive: "/unused/pinned-sdk.tar.gz".into(),
        };
        let settings = ovrcr::protocol::Settings {
            cursor_cloud_accounts: vec![account],
            ..Default::default()
        };
        fs::write(
            live.config.join("dashboard.toml"),
            toml::to_string(&settings).unwrap(),
        )
        .unwrap();
        live.start_binary_env(&[("OVRCR_CURSOR_CLOUD_FIXTURE", config.as_os_str())]);
        live.ready("cloud-preview");
        let Response::Inventory { registry, .. } = live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let workspace = registry.workspace(PROJECT, WORKSPACE).unwrap().path.clone();
        fs::write(
            workspace.join("uncommitted-cloud-local.txt"),
            "excluded local edit",
        )
        .unwrap();
        Self {
            live,
            log,
            config,
            workspace,
        }
    }
    fn launch(&self) -> SessionSummary {
        self.launch_command(vec![
            self.live.executable.clone().into_os_string(),
            "agent".into(),
            "run".into(),
            "cursor-cloud".into(),
            "--".into(),
            "work".into(),
        ])
    }
    fn launch_command(&self, argv: Vec<std::ffi::OsString>) -> SessionSummary {
        let Response::CreatedSession(summary) =
            self.live
                .request(Request::CreateSession(CreateSessionRequest {
                    project: PROJECT.into(),
                    workspace: WORKSPACE.into(),
                    name: "cloud".into(),
                    label: None,
                    argv,
                    kind: SessionKind::Agent {
                        name: "cursor-cloud:work".into(),
                    },
                }))
        else {
            panic!("ordinary Agent creation refused")
        };
        self.live.own_group(summary.pid.unwrap() as i32);
        *summary
    }
    fn text(&self, session: &SessionSummary) -> String {
        let Response::TerminalText { text, .. } = self.live.request(Request::ReadTerminal {
            session: session.id,
            max_lines: None,
        }) else {
            panic!("read terminal")
        };
        text
    }
    fn until(&self, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "fixture timed out; log={:?}",
                fs::read_to_string(&self.log)
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
    fn events(&self) -> Vec<Value> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn no_mutations(&self) {
        assert!(
            self.events()
                .iter()
                .all(|event| event["method"] != "CreateAgent" && event["method"] != "Send")
        );
    }
}

#[test]
fn ordinary_cloud_agent_validates_preview_and_cleans_owned_tree_on_exit_and_signals() {
    for mode in ["normal", "sigterm", "forced-adapter", "server-close"] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let session = fixture.launch();
        fixture.until(|| {
            let text = fixture.text(&session);
            assert!(
                !text.contains("validation failed")
                    && !text.contains("invalid cursor_cloud_accounts"),
                "{text}"
            );
            text.contains("Press Enter to close preview.")
        });
        let text = fixture.text(&session);
        let sha = live::git(&fixture.workspace, &["rev-parse", "HEAD"]);
        assert!(
            text.contains(sha.trim()),
            "exact published HEAD missing: {text}"
        );
        for expected in [
            "Account: work",
            "https://github.com/acme/app",
            "Published commit:",
            "Model: composer-2.5",
            "Local edits are excluded",
            "CreateAgent/Send: unavailable",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        let events = fixture.events();
        assert_eq!(
            events
                .iter()
                .filter_map(|e| e["method"].as_str())
                .collect::<Vec<_>>(),
            ["GetVersion", "Me", "ListModels", "ListRepositories"]
        );
        let started = &events[0];
        let bridge = started["pid"].as_i64().unwrap() as i32;
        let adapter = started["adapter_pid"].as_i64().unwrap() as i32;
        let scratch = PathBuf::from(started["root"].as_str().unwrap());
        assert_eq!(started["environment_isolated"], true);
        match mode {
            "normal" => assert_eq!(
                fixture.live.request(Request::SendTerminal {
                    session: session.id,
                    text: "".into(),
                    submit: true
                }),
                Response::Ok
            ),
            "sigterm" => assert_eq!(
                unsafe { libc::kill(session.pid.unwrap() as i32, libc::SIGTERM) },
                0
            ),
            "forced-adapter" => assert_eq!(unsafe { libc::kill(adapter, libc::SIGKILL) }, 0),
            "server-close" => assert_eq!(
                fixture.live.request(Request::CloseTerminal {
                    session: session.id,
                    expected_run: session.run
                }),
                Response::Ok
            ),
            _ => unreachable!(),
        }
        fixture.until(|| !scratch.exists());
        assert_eq!(
            unsafe { libc::kill(bridge, 0) },
            -1,
            "owned bridge survived {mode}"
        );
        assert_eq!(
            unsafe { libc::kill(adapter, 0) },
            -1,
            "owned adapter survived {mode}"
        );
        fixture.no_mutations();
        assert!(fixture.config.exists());
        assert_eq!(
            fs::read_to_string(fixture.workspace.join("uncommitted-cloud-local.txt")).unwrap(),
            "excluded local edit"
        );
        assert!(
            live::git(&fixture.workspace, &["status", "--porcelain"])
                .contains("uncommitted-cloud-local.txt")
        );
        assert_eq!(live::git(&fixture.workspace, &["rev-parse", "HEAD"]), sha);
    }
}

#[test]
fn ordinary_cloud_agent_refuses_policy_unpublished_source_bad_key_and_missing_model() {
    for (model, published, allowed, reject_credential) in [
        ("composer-2.5", true, false, false),
        ("composer-2.5", false, true, false),
        ("other-model", true, true, false),
        ("composer-2.5", true, true, true),
    ] {
        let fixture = Fixture::new(model, published, allowed, reject_credential);
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cursor cloud validation failed")
        });
        assert!(!fixture.text(&session).contains("validated preview"));
        let events = fixture.events();
        let methods = events
            .iter()
            .filter_map(|event| event["method"].as_str())
            .collect::<Vec<_>>();
        if !published || !allowed {
            assert!(events.is_empty());
        } else if reject_credential {
            assert_eq!(methods, ["GetVersion", "Me"]);
        } else {
            assert_eq!(methods, ["GetVersion", "Me", "ListModels"]);
        }
        fixture.no_mutations();
    }
}

#[test]
fn ordinary_cloud_agent_missing_selected_credential_starts_no_bridge() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let mut config: Value = serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
    config["credential_missing"] = json!(true);
    fs::write(&fixture.config, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        let text = fixture.text(&session);
        assert!(
            !text.contains("validated preview"),
            "missing credential reached preview"
        );
        text.contains("selected Keychain credential missing")
    });
    assert!(fixture.events().is_empty());
}

#[test]
fn ordinary_cloud_agent_missing_invocation_channel_starts_no_adapter_or_bridge() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let unavailable = fixture.live.root.path().join("not-a-temp-directory");
    fs::write(&unavailable, "owned fixture").unwrap();
    let session = fixture.launch_command(vec![
        "/usr/bin/env".into(),
        format!("TMPDIR={}", unavailable.display()).into(),
        fixture.live.executable.clone().into_os_string(),
        "agent".into(),
        "run".into(),
        "cursor-cloud".into(),
        "--".into(),
        "work".into(),
    ]);
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("invocation channel admission unavailable")
    });
    assert!(fixture.events().is_empty());
}
