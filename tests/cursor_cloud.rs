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
            "CreateAgent: explicit prepare required. Send: unavailable.",
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

#[test]
fn managed_preflight_helpers_and_descendants_are_cleaned_after_adapter_death_or_timeout() {
    for mode in ["forced-adapter", "timeout"] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let helper = fixture.live.root.path().join("blocked-helper.py");
        let observed = helper.with_extension("json");
        fs::write(&helper, format!("#!/usr/bin/env python3\nimport json,os,subprocess,sys,time\nfrom pathlib import Path\nchild=subprocess.Popen([sys.executable,'-c','import time; time.sleep(300)'])\nPath({:?}).write_text(json.dumps(dict(helper=os.getpid(),adapter=os.getppid(),grandchild=child.pid,root=os.environ['OVRCR_CURSOR_CLOUD_ROOT'])))\ntime.sleep(300)\n", observed)).unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let mut config: Value =
            serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
        config["preflight_helper"] = json!(helper);
        if mode == "timeout" {
            config["preflight_timeout_ms"] = json!(2000);
        }
        fs::write(&fixture.config, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| observed.exists());
        let ids: Value = serde_json::from_slice(&fs::read(&observed).unwrap()).unwrap();
        let helper = ids["helper"].as_i64().unwrap() as i32;
        let adapter = ids["adapter"].as_i64().unwrap() as i32;
        let grandchild = ids["grandchild"].as_i64().unwrap() as i32;
        // Register the fixture-established group even in the original escaping implementation.
        let helper_group = unsafe { libc::getpgid(helper) };
        assert!(helper_group > 1);
        fixture.live.own_group(helper_group);
        if mode == "forced-adapter" {
            assert_eq!(unsafe { libc::kill(adapter, libc::SIGKILL) }, 0);
        } else {
            fixture.until(|| {
                fixture
                    .text(&session)
                    .contains("preflight command exceeded bounds")
            });
        }
        let scratch = PathBuf::from(ids["root"].as_str().unwrap());
        fixture.until(|| !scratch.exists());
        assert_eq!(
            unsafe { libc::kill(helper, 0) },
            -1,
            "preflight helper survived {mode}"
        );
        assert_eq!(
            unsafe { libc::kill(grandchild, 0) },
            -1,
            "preflight descendant survived {mode}"
        );
        assert_eq!(
            unsafe { libc::kill(adapter, 0) },
            -1,
            "adapter survived {mode}"
        );
        assert!(fixture.events().is_empty());
    }
}

#[test]
fn managed_creation_retains_exact_handle_without_sending() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.until(|| {
        let text = fixture.text(&session);
        assert!(!text.contains("validation failed"), "{text}");
        text.contains("Cloud handle durably retained: fixture-agent-373")
    });
    let methods = fixture
        .events()
        .iter()
        .filter_map(|e| e["method"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(methods.iter().filter(|m| *m == "CreateAgent").count(), 1);
    assert_eq!(methods.iter().filter(|m| *m == "Send").count(), 0);
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let retained = sessions.iter().find(|s| s.id == session.id).unwrap();
    assert!(
        retained
            .recovery
            .as_ref()
            .unwrap()
            .unavailable
            .as_ref()
            .unwrap()
            .contains("handle restoration unavailable")
    );
}

#[test]
fn managed_creation_crashes_retain_original_operation_and_require_explicit_discard() {
    for (boundary, creations, description) in [
        ("before_dispatch", 0, "prepared, never dispatched"),
        ("after_create", 1, "unresolved, possibly escaped"),
        ("after_reply", 1, "unresolved, possibly escaped"),
        ("after_ack", 1, "handle acknowledged"),
    ] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let mut config: Value =
            serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
        config["creation_crash_at"] = json!(boundary);
        if boundary == "after_create" {
            let bridge_config = fixture.live.root.path().join("bridge.json");
            let mut bridge: Value =
                serde_json::from_slice(&fs::read(&bridge_config).unwrap()).unwrap();
            bridge["drop_creation_reply"] = json!(true);
            fs::write(bridge_config, bridge.to_string()).unwrap();
        }
        fs::write(&fixture.config, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        assert_eq!(
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "prepare".into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.until(|| {
            let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect)
            else {
                panic!("inventory")
            };
            !sessions
                .iter()
                .find(|s| s.id == session.id)
                .unwrap()
                .phase
                .is_live()
        });
        let offline =
            ovrcr_runtime::retained::load_session_summaries(&fixture.live.config).unwrap();
        let retained = offline.iter().find(|s| s.id == session.id).unwrap();
        let recovery = retained.recovery.as_ref().unwrap();
        assert!(
            recovery.unavailable.as_ref().unwrap().contains(description),
            "{boundary}: {retained:?}"
        );
        assert!(recovery.conversation.is_none());
        assert!(!retained.can_auto_recover());
        let events = fixture.events();
        assert_eq!(
            events
                .iter()
                .filter(|e| e["method"] == "CreateAgent")
                .count(),
            creations,
            "{boundary}"
        );
        assert_eq!(events.iter().filter(|e| e["method"] == "Send").count(), 0);
        let payloads = fs::read_dir(fixture.live.config.join("cloud-creations"))
            .unwrap()
            .count();
        assert_eq!(payloads, usize::from(boundary != "after_ack"));
        assert_eq!(
            fixture.live.request(Request::Shutdown { kill: true }),
            Response::Ok
        );
        fixture.live.join();
        fixture
            .live
            .start_binary_env(&[("OVRCR_CURSOR_CLOUD_FIXTURE", fixture.config.as_os_str())]);
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let restored = sessions.iter().find(|s| s.id == session.id).unwrap();
        assert!(
            restored
                .recovery
                .as_ref()
                .unwrap()
                .unavailable
                .as_ref()
                .unwrap()
                .contains(description)
        );
        assert!(!restored.can_auto_recover());
        assert_eq!(
            fixture.live.request(Request::CloseTerminal {
                session: session.id,
                expected_run: session.run
            }),
            Response::Ok
        );
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let archived = sessions.iter().find(|s| s.id == session.id).unwrap();
        assert!(matches!(
            fixture.live.request(Request::DiscardCloudCreation {
                session: session.id,
                expected_run: session.run
            }),
            Response::Error { .. }
        ));
        if boundary != "after_ack" {
            assert!(matches!(
                fixture.live.request(Request::DeleteArchivedSession {
                    session: session.id,
                    expected_run: archived.run
                }),
                Response::Error { .. }
            ));
        }
        assert_eq!(
            fixture.live.request(Request::DiscardCloudCreation {
                session: session.id,
                expected_run: archived.run
            }),
            Response::Ok
        );
        assert_eq!(
            fs::read_dir(fixture.live.config.join("cloud-creations"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fixture.live.request(Request::DeleteArchivedSession {
                session: session.id,
                expected_run: archived.run
            }),
            Response::Ok
        );
        // Restart reads the same store and does not recreate deleted cloud rows.
        assert_eq!(
            fixture.live.request(Request::Shutdown { kill: true }),
            Response::Ok
        );
        fixture.live.join();
        fixture
            .live
            .start_binary_env(&[("OVRCR_CURSOR_CLOUD_FIXTURE", fixture.config.as_os_str())]);
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        assert!(sessions.iter().all(|s| s.id != session.id));
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "CreateAgent")
                .count(),
            creations
        );
    }
}

#[test]
fn managed_creation_discard_serializes_against_dispatch_and_late_receipt() {
    for (boundary, creations) in [("before_dispatch", 0), ("after_claim", 1)] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let marker = fixture.live.root.path().join("creation-checkpoint");
        let mut config: Value =
            serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
        config["creation_pause_at"] = json!(boundary);
        config["creation_checkpoint"] = json!(marker);
        fs::write(&fixture.config, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        assert_eq!(
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "prepare".into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.until(|| marker.exists());
        assert_eq!(
            fixture.live.request(Request::DiscardCloudCreation {
                session: session.id,
                expected_run: session.run
            }),
            Response::Ok
        );
        fs::write(marker.with_extension("release"), "release").unwrap();
        fixture.until(|| {
            let text = fixture.text(&session);
            text.contains(if creations == 0 {
                "durable creation admission unavailable"
            } else {
                "Cloud handle durably retained: fixture-agent-373"
            })
        });
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        let row = sessions.iter().find(|s| s.id == session.id).unwrap();
        assert!(
            row.recovery
                .as_ref()
                .unwrap()
                .unavailable
                .as_ref()
                .unwrap()
                .contains("discarded locally")
        );
        assert!(!row.can_auto_recover());
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "CreateAgent")
                .count(),
            creations
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "Send")
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(fixture.live.config.join("cloud-creations"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn managed_creation_storage_failures_permit_only_durably_claimed_effects() {
    for (mode, creations) in [
        ("intent-commit", 0),
        ("claim-commit", 0),
        ("receipt-commit", 1),
        ("corrupt-payload", 0),
        ("private-root", 0),
        ("closure-commit", 1),
    ] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let marker = fixture.live.root.path().join("storage-checkpoint");
        if ["corrupt-payload", "closure-commit"].contains(&mode) {
            let mut config: Value =
                serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
            config["creation_pause_at"] = json!("before_dispatch");
            config["creation_checkpoint"] = json!(marker);
            fs::write(&fixture.config, config.to_string()).unwrap();
        }
        let database =
            rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
        let trigger = match mode {
            "intent-commit" => Some(
                "CREATE TRIGGER fail_creation BEFORE INSERT ON cloud_creations BEGIN SELECT RAISE(ABORT,'fixture full storage'); END",
            ),
            "claim-commit" => Some(
                "CREATE TRIGGER fail_creation BEFORE UPDATE ON cloud_creations WHEN json_extract(NEW.metadata,'$.claimed')=1 BEGIN SELECT RAISE(ABORT,'fixture claim storage failure'); END",
            ),
            "receipt-commit" => Some(
                "CREATE TRIGGER fail_creation BEFORE UPDATE ON cloud_creations WHEN json_type(NEW.metadata,'$.receipt')='object' BEGIN SELECT RAISE(ABORT,'fixture receipt storage failure'); END",
            ),
            "closure-commit" => Some(
                "CREATE TRIGGER fail_closure BEFORE INSERT ON cloud_creation_fences BEGIN SELECT RAISE(ABORT,'fixture closure storage failure'); END",
            ),
            _ => None,
        };
        if let Some(sql) = trigger {
            database.execute_batch(sql).unwrap();
        }
        if mode == "private-root" {
            std::os::unix::fs::symlink(
                fixture.live.root.path(),
                fixture.live.config.join("cloud-creations"),
            )
            .unwrap();
        }
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        assert_eq!(
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "prepare".into(),
                submit: true
            }),
            Response::Ok
        );
        if ["corrupt-payload", "closure-commit"].contains(&mode) {
            fixture.until(|| marker.exists());
            if mode == "corrupt-payload" {
                let payload = fs::read_dir(fixture.live.config.join("cloud-creations"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                fs::write(payload, "corrupt original request").unwrap();
            } else {
                assert!(matches!(
                    fixture.live.request(Request::DiscardCloudCreation {
                        session: session.id,
                        expected_run: session.run
                    }),
                    Response::Error { .. }
                ));
            }
            fs::write(marker.with_extension("release"), "release").unwrap();
        }
        fixture.until(|| {
            let text = fixture.text(&session);
            text.contains(if mode == "closure-commit" {
                "Cloud handle durably retained: fixture-agent-373"
            } else {
                "durable creation admission unavailable"
            })
        });
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "CreateAgent")
                .count(),
            creations,
            "{mode}"
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "Send")
                .count(),
            0
        );
        let raw: String = database
            .query_row(
                "SELECT coalesce(group_concat(metadata),'') FROM cloud_creations",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!raw.contains("ovrcr-fixture-not-a-credential"));
        assert!(!raw.contains("fixture_bridge_bearer"));
        if mode == "receipt-commit" {
            assert!(raw.contains("\"receipt\":null"));
        }
        if mode == "closure-commit" {
            assert!(raw.contains("\"closed\":false"));
        }
        if let Some(sql) = trigger {
            database
                .execute_batch(if sql.contains("fail_closure") {
                    "DROP TRIGGER fail_closure"
                } else {
                    "DROP TRIGGER fail_creation"
                })
                .unwrap();
        }
        if mode == "private-root" {
            fs::remove_file(fixture.live.config.join("cloud-creations")).unwrap();
        }
    }
}

#[test]
fn restart_cleans_owned_publication_aliases_and_durably_retired_creation_payloads() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    for boundary in ["before_dispatch", "after_ack"] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let mut config: Value =
            serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
        config["creation_crash_at"] = json!(boundary);
        fs::write(&fixture.config, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        assert_eq!(
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "prepare".into(),
                submit: true
            }),
            Response::Ok
        );
        fixture.until(|| {
            let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect)
            else {
                panic!("inventory")
            };
            !sessions
                .iter()
                .find(|s| s.id == session.id)
                .unwrap()
                .phase
                .is_live()
        });
        assert_eq!(
            fixture.live.request(Request::Shutdown { kill: true }),
            Response::Ok
        );
        fixture.live.join();
        let database =
            rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
        let raw: String = database
            .query_row(
                "SELECT metadata FROM cloud_creations WHERE session=?1",
                [i64::try_from(session.id.0).unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        let record: Value = serde_json::from_str(&raw).unwrap();
        let intent: ovrcr::protocol::CloudCreationIntent =
            serde_json::from_value(record["intent"].clone()).unwrap();
        let root = fixture.live.config.join("cloud-creations");
        let payload = root.join(&intent.key);
        if boundary == "after_ack" {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&payload)
                .unwrap();
            file.write_all(&serde_json::to_vec(&intent.request()).unwrap())
                .unwrap();
        }
        fs::hard_link(&payload, root.join(format!(".{}", "d".repeat(64)))).unwrap();
        drop(database);
        fixture
            .live
            .start_binary_env(&[("OVRCR_CURSOR_CLOUD_FIXTURE", fixture.config.as_os_str())]);
        assert!(matches!(
            fixture.live.request(Request::Inspect),
            Response::Inventory { .. }
        ));
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            usize::from(boundary == "before_dispatch")
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "CreateAgent")
                .count(),
            usize::from(boundary == "after_ack")
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "Send")
                .count(),
            0
        );
    }
}
