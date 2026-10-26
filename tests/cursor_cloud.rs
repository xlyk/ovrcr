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
            "CreateAgent: explicit prepare required. Send: available after prepare.",
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
fn managed_cloud_refuses_incompatible_bridge_before_account_requests() {
    let required = [
        "agent.create",
        "agent.send",
        "run.cancel",
        "agent.management",
        "cursor.catalog",
    ];
    let base = json!({"bridgeVersion":"1.0.0", "protocolVersion":"sdk.v1"});
    let mut cases = vec![base.clone()];
    for malformed in [json!(null), json!("agent.create"), json!([true])] {
        let mut version = base.clone();
        version["capabilities"] = malformed;
        cases.push(version);
    }
    let mut malformed = base.clone();
    let mut mixed = json!(required);
    mixed.as_array_mut().unwrap().push(json!(5));
    malformed["capabilities"] = mixed;
    cases.push(malformed);
    for missing in required {
        let mut version = base.clone();
        version["capabilities"] = json!(
            required
                .iter()
                .filter(|flag| **flag != missing)
                .collect::<Vec<_>>()
        );
        cases.push(version);
    }
    for field in ["bridgeVersion", "protocolVersion"] {
        let mut version = base.clone();
        version["capabilities"] = json!(required);
        version[field] = json!("unsupported");
        cases.push(version);
    }
    for version in cases {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let path = fixture.live.root.path().join("bridge.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["version_response"] = version.clone();
        fs::write(path, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            let text = fixture.text(&session);
            text.contains("Cursor cloud validation failed") || text.contains("validated preview")
        });
        assert!(
            fixture
                .text(&session)
                .contains("Cursor cloud validation failed"),
            "incompatible bridge accepted: {version}"
        );
        assert!(!fixture.text(&session).contains("validated preview"));
        let events = fixture.events();
        let methods = events
            .iter()
            .filter_map(|event| event["method"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            ["GetVersion"],
            "must refuse before credentialed/catalog or agent operations: {version}"
        );
        fixture.no_mutations();
    }
}

#[test]
fn managed_cloud_accepts_unknown_additional_bridge_capabilities() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let path = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["version_response"] = json!({"bridgeVersion":"1.0.0", "protocolVersion":"sdk.v1",
        "capabilities":["future.unrecognized", "cursor.catalog", "agent.management", "run.cancel", "agent.send", "agent.create"]});
    fs::write(path, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    assert!(
        fixture
            .text(&session)
            .contains("Cursor cloud validated preview")
    );
    assert!(
        fixture
            .text(&session)
            .contains("Terminal responses, Retry and active reattachment remain blocked")
    );
    fixture.no_mutations();
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
        fs::write(&helper, format!("#!/usr/bin/env python3\nimport json,os,subprocess,sys,time\nfrom pathlib import Path\nchild=subprocess.Popen([sys.executable,'-c','import time; time.sleep(300)'])\nobserved=Path({:?})\nobserved.with_suffix('.pending').write_text(json.dumps(dict(helper=os.getpid(),adapter=os.getppid(),grandchild=child.pid,root=os.environ['OVRCR_CURSOR_CLOUD_ROOT'])))\nobserved.with_suffix('.pending').replace(observed)\ntime.sleep(300)\n", observed)).unwrap();
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
        assert!(matches!(
            fixture.live.request(Request::DeleteArchivedSession {
                session: session.id,
                expected_run: archived.run
            }),
            Response::Error { .. }
        ));
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

#[test]
fn managed_cloud_send_streams_exact_run_and_idle_followup_same_conversation() {
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
            submit: true
        }),
        Response::Ok
    );
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained:")
    });
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "Inspect the selected source".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Ready: exact run fixture-run-1")
    });
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "Summarize that inspection".into(),
            submit: true
        }),
        Response::Ok
    );
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Ready: exact run fixture-run-2")
    });
    let methods = fixture
        .events()
        .iter()
        .filter_map(|e| e["method"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(methods.iter().filter(|m| *m == "CreateAgent").count(), 1);
    assert_eq!(methods.iter().filter(|m| *m == "Send").count(), 2);
    assert_eq!(methods.iter().filter(|m| *m == "GetAgent").count(), 2);
    assert_eq!(methods.iter().filter(|m| *m == "GetRun").count(), 2);
    assert_eq!(
        methods
            .iter()
            .filter(|m| ["ResumeAgent", "ListRuns", "CancelRun"].contains(&m.as_str()))
            .count(),
        0
    );
}

#[test]
fn managed_result_links_require_independent_exact_head_and_never_invent_a_pr() {
    for mode in ["branch", "pr", "wrong-head"] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let bridge_config = fixture.live.root.path().join("bridge.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&bridge_config).unwrap()).unwrap();
        config["result_branch"] = json!(true);
        config["result_pr"] = json!(mode != "branch");
        fs::write(&bridge_config, config.to_string()).unwrap();
        live::git(&fixture.live.repo, &["branch", "fixture-result"]);
        let gh = fixture.live.claude.command.parent().unwrap().join("gh");
        fs::write(&gh,format!(r#"#!/usr/bin/env python3
import json,subprocess,sys
repo={repo:?}
sha=subprocess.check_output(['git','-C',repo,'rev-parse','fixture-result'],text=True).strip()
base=subprocess.check_output(['git','-C',repo,'rev-parse','HEAD'],text=True).strip()
assert sys.argv[1]=='api'
path=sys.argv[2]
if path=='repos/acme/app/commits/fixture-result': value={{'sha':sha}}
elif path=='repos/acme/app/compare/'+base+'...'+sha:
    ancestor=subprocess.check_output(['git','-C',repo,'merge-base',base,sha],text=True).strip()
    value={{'status':'identical','merge_base_commit':{{'sha':ancestor}}}}
elif path=='repos/acme/app/pulls/17': value={{'html_url':'https://github.com/acme/app/pull/17','head':{{'sha':('0'*40 if {wrong} else sha),'ref':'fixture-result','repo':{{'html_url':'https://github.com/acme/app'}}}}}}
else: raise SystemExit(2)
print(json.dumps(value))
"#,repo=fixture.live.repo,wrong=if mode=="wrong-head" {"True"} else {"False"})).unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o700)).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cloud handle durably retained:")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "Inspect source".into(),
            submit: true,
        });
        let expected = match mode {
            "branch" => "Verified published branch:",
            "pr" => "Verified existing PR:",
            _ => "Result link unverified:",
        };
        fixture.until(|| fixture.text(&session).contains(expected));
        let text = fixture.text(&session);
        if mode == "branch" {
            assert!(!text.contains("Verified existing PR:"));
            assert!(text.contains("no PR"));
        }
        if mode == "wrong-head" {
            assert!(!text.contains("Verified existing PR:"));
        }
    }
}

#[test]
fn managed_busy_prompt_is_rejected_retained_and_requires_manual_resubmission() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let pause = fixture.live.root.path().join("send-paused");
    let bridge_config = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&bridge_config).unwrap()).unwrap();
    config["send_pause_file"] = json!(pause);
    fs::write(&bridge_config, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained:")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "Inspect source".into(),
        submit: true,
    });
    fixture.until(|| pause.exists());
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "Retain this manual follow-up".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Busy: prompt rejected and retained locally")
    });
    fs::write(pause.with_extension("release"), "release").unwrap();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Ready: exact run fixture-run-1")
    });
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send")
            .count(),
        1
    );
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/resubmit".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Ready: exact run fixture-run-2")
    });
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send")
            .count(),
        2
    );
}

#[test]
fn managed_send_uncertainty_and_malformed_evidence_never_retry_or_become_ready() {
    for scenario in [
        "lost_reply",
        "cross_run",
        "unknown_shape",
        "bad_source",
        "bad_call",
        "bad_success",
        "missing_done",
        "wrong_repository",
        "wrong_snapshot",
    ] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let bridge_config = fixture.live.root.path().join("bridge.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&bridge_config).unwrap()).unwrap();
        config["send_scenario"] = json!(scenario);
        config["drop_send_reply"] = json!(scenario == "lost_reply");
        fs::write(&bridge_config, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cloud handle durably retained:")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "Inspect source".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Observation unknown/unverified:")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "/resubmit".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Busy: prompt rejected and retained locally")
        });
        let text = fixture.text(&session);
        assert!(!text.contains("Ready: exact run"), "{scenario}: {text}");
        let methods = fixture.events();
        assert_eq!(
            methods.iter().filter(|e| e["method"] == "Send").count(),
            1,
            "{scenario}"
        );
        assert_eq!(
            methods
                .iter()
                .filter(|e| e["method"] == "CreateAgent")
                .count(),
            1
        );
        assert!(
            methods
                .iter()
                .all(|e| e["method"] != "ListRuns" && e["method"] != "ResumeAgent")
        );
        assert_eq!(
            fixture.live.request(Request::DiscardCloudCreation {
                session: session.id,
                expected_run: session.run
            }),
            Response::Ok
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
fn managed_send_storage_faults_never_dispatch_without_claim_or_retire_unreceipted_payload() {
    for (mode, sends) in [
        ("intent", 0),
        ("claim", 0),
        ("receipt", 1),
        ("terminal", 1),
        ("corrupt", 0),
        ("closure", 1),
    ] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let marker = fixture.live.root.path().join("send-storage");
        if mode == "corrupt" || mode == "closure" {
            let mut controls: Value =
                serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
            controls["send_pause_at"] = json!("before_dispatch");
            controls["send_checkpoint"] = json!(marker);
            fs::write(&fixture.config, controls.to_string()).unwrap();
        }
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cloud handle durably retained:")
        });
        let database =
            rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
        let condition = match mode {
            "intent" => "json_array_length(NEW.metadata,'$.sends') > 0",
            "claim" => "json_extract(NEW.metadata,'$.sends[0].claimed') = 1",
            "receipt" => "json_type(NEW.metadata,'$.sends[0].run_id') = 'text'",
            "terminal" => "json_type(NEW.metadata,'$.sends[0].terminal') = 'text'",
            _ => "0",
        };
        database.execute_batch(&format!("CREATE TRIGGER fail_send BEFORE UPDATE ON cloud_creations WHEN {condition} BEGIN SELECT RAISE(ABORT,'fixture Send storage fault'); END;")).unwrap();
        if mode == "closure" {
            database.execute_batch("CREATE TRIGGER fail_send_close BEFORE INSERT ON cloud_creation_fences BEGIN SELECT RAISE(ABORT,'fixture closure fault'); END;").unwrap();
        }
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "PRIVATE_SEND_PAYLOAD_374".into(),
            submit: true,
        });
        if mode == "corrupt" || mode == "closure" {
            fixture.until(|| marker.exists());
            if mode == "corrupt" {
                let payload = fs::read_dir(fixture.live.config.join("cloud-creations"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                fs::write(payload, "corrupt").unwrap();
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
            fixture.text(&session).contains(if mode == "closure" {
                "Ready: exact run"
            } else {
                "Observation unknown/unverified:"
            })
        });
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "Send")
                .count(),
            sends,
            "{mode}"
        );
        let raw: String = database
            .query_row(
                "SELECT metadata FROM cloud_creations WHERE session=?1",
                [session.id.0 as i64],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            !raw.contains("PRIVATE_SEND_PAYLOAD_374")
                && !raw.contains("ovrcr-fixture-not-a-credential")
        );
        if mode == "receipt" {
            assert!(raw.contains("\"run_id\":null"));
            assert_eq!(
                fs::read_dir(fixture.live.config.join("cloud-creations"))
                    .unwrap()
                    .count(),
                1
            );
        }
        if mode == "terminal" {
            let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect)
            else {
                panic!("inspect")
            };
            let current = sessions.iter().find(|s| s.id == session.id).unwrap();
            assert_eq!(
                current.agent.as_ref().unwrap().effective_activity(),
                ovrcr::protocol::AgentActivity::Unknown
            );
            assert!(current.unread.is_none());
        }
        database.execute_batch("DROP TRIGGER fail_send;").unwrap();
        if mode == "closure" {
            database
                .execute_batch("DROP TRIGGER fail_send_close;")
                .unwrap();
        }
        assert_eq!(
            fixture.live.request(Request::DiscardCloudCreation {
                session: session.id,
                expected_run: session.run
            }),
            Response::Ok
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
fn managed_send_crash_and_discard_boundaries_preserve_uncertainty_and_exact_late_identity() {
    for (boundary, sends) in [
        ("before_dispatch", 0),
        ("after_claim", 1),
        ("after_reply", 1),
        ("after_ack", 1),
    ] {
        for discard in [false, true] {
            let fixture = Fixture::new("composer-2.5", true, true, false);
            let marker = fixture.live.root.path().join("send-checkpoint");
            let mut controls: Value =
                serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
            controls[if discard {
                "send_pause_at"
            } else {
                "send_crash_at"
            }] = json!(boundary);
            controls["send_checkpoint"] = json!(marker);
            fs::write(&fixture.config, controls.to_string()).unwrap();
            let session = fixture.launch();
            fixture.until(|| {
                fixture
                    .text(&session)
                    .contains("Press Enter to close preview.")
            });
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "prepare".into(),
                submit: true,
            });
            fixture.until(|| {
                fixture
                    .text(&session)
                    .contains("Cloud handle durably retained:")
            });
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "Inspect source".into(),
                submit: true,
            });
            if discard {
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
                    fixture
                        .text(&session)
                        .contains("Observation unknown/unverified:")
                });
            } else {
                fixture.until(|| {
                    let Response::Inventory { sessions, .. } =
                        fixture.live.request(Request::Inspect)
                    else {
                        panic!("inspect")
                    };
                    sessions
                        .iter()
                        .find(|s| s.id == session.id)
                        .is_some_and(|s| !s.phase.is_live())
                });
            }
            // Claim wins before the adapter's next boundary; after_claim crash has zero actual RPCs.
            let actual = if !discard && boundary == "after_claim" {
                0
            } else {
                sends
            };
            assert_eq!(
                fixture
                    .events()
                    .iter()
                    .filter(|e| e["method"] == "Send")
                    .count(),
                actual,
                "{boundary} discard={discard}"
            );
            let database =
                rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
            let raw: String = database
                .query_row(
                    "SELECT metadata FROM cloud_creations WHERE session=?1",
                    [session.id.0 as i64],
                    |r| r.get(0),
                )
                .unwrap();
            let retained: Value = serde_json::from_str(&raw).unwrap();
            assert!(retained["sends"][0]["terminal"].is_null());
            assert_eq!(
                retained["sends"][0]["run_id"].is_string(),
                boundary == "after_ack"
                    || (discard && ["after_claim", "after_reply"].contains(&boundary))
            );
            if discard {
                assert_eq!(
                    fs::read_dir(fixture.live.config.join("cloud-creations"))
                        .unwrap()
                        .count(),
                    0
                );
            }
            assert!(!fixture.text(&session).contains("Ready: exact run"));
        }
    }
}

#[test]
fn review_374_followup_claim_clears_prior_ready_before_lost_run_identity() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let config_path = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["drop_send_reply_on"] = json!(2);
    fs::write(config_path, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained:")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "Inspect source".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Ready: exact run fixture-run-1")
    });
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inspect")
    };
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .agent
            .as_ref()
            .unwrap()
            .effective_activity(),
        ovrcr::protocol::AgentActivity::ResponseReady
    );
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "Follow-up whose reply is lost".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Observation unknown/unverified:")
    });
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inspect")
    };
    let current = sessions.iter().find(|s| s.id == session.id).unwrap();
    assert_eq!(
        current.agent.as_ref().unwrap().effective_activity(),
        ovrcr::protocol::AgentActivity::Unknown,
        "a claimed follow-up must retire prior Ready even without a new run ID"
    );
    assert!(
        current.unread.is_none(),
        "prior completion cannot remain an Unread/alert target"
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send")
            .count(),
        2
    );
}

#[test]
fn review_374_contradictory_shell_failure_cannot_verify_source_or_publish_ready() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let config_path = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["send_scenario"] = json!("shell_failure_discriminator");
    fs::write(config_path, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained:")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "Inspect source".into(),
        submit: true,
    });
    fixture.until(|| {
        let text = fixture.text(&session);
        text.contains("Ready: exact run") || text.contains("Observation unknown/unverified:")
    });
    assert!(
        fixture
            .text(&session)
            .contains("Observation unknown/unverified:"),
        "shell failure discriminator must defeat superficially successful flat output"
    );
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inspect")
    };
    let current = sessions.iter().find(|s| s.id == session.id).unwrap();
    assert_ne!(
        current.agent.as_ref().unwrap().effective_activity(),
        ovrcr::protocol::AgentActivity::ResponseReady
    );
    assert!(current.unread.is_none());
}

#[test]
fn managed_recorded_shell_success_shapes_and_duplicate_frames_remain_verified() {
    for scenario in ["shell_success", "shell_flat_success", "duplicate"] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let config_path = fixture.live.root.path().join("bridge.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        config["send_scenario"] = json!(scenario);
        fs::write(config_path, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cloud handle durably retained:")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "Inspect source".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Ready: exact run fixture-run-1")
        });
        assert_eq!(
            fixture
                .text(&session)
                .matches("Inspecting selected source.")
                .count(),
            1,
            "{scenario}"
        );
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inspect")
        };
        let current = sessions.iter().find(|s| s.id == session.id).unwrap();
        assert_eq!(
            current.agent.as_ref().unwrap().effective_activity(),
            ovrcr::protocol::AgentActivity::ResponseReady
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "Send")
                .count(),
            1
        );
    }
}

#[test]
fn managed_cancel_before_send_closes_delivery_and_preserves_private_content() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/cancel".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Local delivery stopped before dispatch")
    });
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send" || e["method"] == "CancelRun")
            .count(),
        0
    );
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    assert!(
        sessions
            .iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .recovery
            .as_ref()
            .unwrap()
            .unavailable
            .as_ref()
            .unwrap()
            .contains("cancellation intent retained")
    );
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/detach".into(),
        submit: true,
    });
}

#[test]
fn managed_live_cancel_known_run_requires_durable_intent_and_authoritative_outcome() {
    for (outcome, snapshot) in [
        (
            "RUN_LIFECYCLE_STATUS_CANCELLED",
            "RUN_LIFECYCLE_STATUS_CANCELLED",
        ),
        (
            "RUN_LIFECYCLE_STATUS_FINISHED",
            "RUN_LIFECYCLE_STATUS_FINISHED",
        ),
        ("RUN_LIFECYCLE_STATUS_ERROR", "RUN_LIFECYCLE_STATUS_ERROR"),
        (
            "RUN_LIFECYCLE_STATUS_CANCELLED",
            "RUN_LIFECYCLE_STATUS_FINISHED",
        ),
    ] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let marker = fixture.live.root.path().join("cancel-identity");
        let bridge = fixture.live.root.path().join("bridge.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&bridge).unwrap()).unwrap();
        config["pause_after_identity"] = json!(marker);
        config["cancel_outcome"] = json!(outcome);
        config["cancel_snapshot_outcome"] = json!(snapshot);
        fs::write(bridge, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        for text in ["prepare", "inspect"] {
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: text.into(),
                submit: true,
            });
            fixture.until(|| {
                fixture.text(&session).contains(if text == "prepare" {
                    "Cloud handle durably retained"
                } else {
                    "Observing exact run fixture-run-1"
                })
            });
        }
        assert_eq!(
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "partial λ".into(),
                submit: false
            }),
            Response::Ok
        );
        assert_eq!(
            fixture.live.request(Request::CancelCloudCreation {
                session: session.id,
                expected_run: session.run
            }),
            Response::Ok
        );
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cancellation requested for exact run fixture-run-1")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: " τελικό".into(),
            submit: true,
        });
        fixture.until(|| fixture.text(&session).contains("τελικό"));
        let echoed = fixture.text(&session);
        assert!(
            echoed.contains("partial λ") && echoed.contains("τελικό"),
            "partial Unicode input was lost: {echoed}"
        );
        assert!(echoed.contains("Busy: prompt rejected and retained locally"));
        let cancels: Vec<_> = fixture
            .events()
            .into_iter()
            .filter(|e| e["method"] == "CancelRun")
            .collect();
        assert_eq!(cancels.len(), 1);
        assert_eq!(cancels[0]["run"], "fixture-run-1");
        assert!(
            !fixture.text(&session).contains("ended Cancelled"),
            "RPC acknowledgement is not terminal evidence"
        );
        fs::write(marker.with_extension("release"), "release").unwrap();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains(if snapshot.ends_with("FINISHED") {
                    "Ready: exact run"
                } else if snapshot.ends_with("ERROR") {
                    "ended Error"
                } else {
                    "ended Cancelled"
                })
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "must not dispatch".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("durable creation admission unavailable")
        });
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "Send")
                .count(),
            1
        );
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "/detach".into(),
            submit: true,
        });
    }
}

#[test]
fn managed_cancel_dispatch_races_retain_intent_without_replay_and_late_id_cancels_exact_run() {
    for (phase, boundary, expected_create, expected_send, expected_cancel) in [
        ("creation", "before_dispatch", 0, 0, 0),
        ("creation", "after_claim", 1, 0, 0),
        ("send", "before_dispatch", 1, 0, 0),
        ("send", "after_claim", 1, 1, 1),
        ("send", "after_reply", 1, 1, 1),
    ] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let marker = fixture.live.root.path().join("cancel-boundary");
        let mut config: Value =
            serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
        config[format!("{phase}_pause_at")] = json!(boundary);
        config[format!("{phase}_checkpoint")] = json!(marker);
        fs::write(&fixture.config, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        });
        if phase == "send" {
            fixture.until(|| {
                fixture
                    .text(&session)
                    .contains("Cloud handle durably retained")
            });
            fixture.live.request(Request::SendTerminal {
                session: session.id,
                text: "inspect".into(),
                submit: true,
            });
        }
        fixture.until(|| marker.exists());
        assert_eq!(
            fixture.live.request(Request::CancelCloudCreation {
                session: session.id,
                expected_run: session.run
            }),
            Response::Ok
        );
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        assert!(
            sessions
                .iter()
                .find(|s| s.id == session.id)
                .unwrap()
                .recovery
                .as_ref()
                .unwrap()
                .unavailable
                .as_ref()
                .unwrap()
                .contains("cancellation intent retained")
        );
        fs::write(marker.with_extension("release"), "release").unwrap();
        fixture.until(|| {
            let text = fixture.text(&session);
            if boundary == "before_dispatch" {
                text.contains("durable creation admission unavailable")
            } else if phase == "creation" {
                text.contains("Cloud handle durably retained")
            } else {
                text.contains("Ready: exact run")
            }
        });
        for (method, expected) in [
            ("CreateAgent", expected_create),
            ("Send", expected_send),
            ("CancelRun", expected_cancel),
        ] {
            assert_eq!(
                fixture
                    .events()
                    .iter()
                    .filter(|e| e["method"] == method)
                    .count(),
                expected,
                "{phase}/{boundary}/{method}"
            );
        }
        assert!(
            fixture
                .events()
                .iter()
                .all(|e| e["method"] != "ResumeAgent" && e["method"] != "ListRuns")
        );
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "/detach".into(),
            submit: true,
        });
    }
}

#[test]
fn managed_discard_removes_owned_publication_aliases_before_acknowledgement() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let marker = fixture.live.root.path().join("discard-alias");
    let mut config: Value = serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
    config["send_pause_at"] = json!("before_dispatch");
    config["send_checkpoint"] = json!(marker);
    fs::write(&fixture.config, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "private unresolved prompt".into(),
        submit: true,
    });
    fixture.until(|| marker.exists());
    let root = fixture.live.config.join("cloud-creations");
    let payload = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    fs::hard_link(payload, root.join(format!(".{}", "a".repeat(64)))).unwrap();
    assert_eq!(
        fixture.live.request(Request::DiscardCloudCreation {
            session: session.id,
            expected_run: session.run
        }),
        Response::Ok
    );
    assert_eq!(
        fs::read_dir(root).unwrap().count(),
        0,
        "Discard acknowledgement must cover all owned payload aliases"
    );
    fs::write(marker.with_extension("release"), "release").unwrap();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("durable creation admission unavailable")
    });
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send" || e["method"] == "CancelRun")
            .count(),
        0
    );
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/detach".into(),
        submit: true,
    });
}

#[test]
fn managed_failed_cancel_commits_permit_zero_cancel_rpcs_and_do_not_claim_durable_success() {
    for mode in ["intent", "claim"] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let marker = fixture.live.root.path().join("failed-cancel");
        let path = fixture.live.root.path().join("bridge.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["pause_after_identity"] = json!(marker);
        fs::write(path, config.to_string()).unwrap();
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cloud handle durably retained")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "inspect".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Observing exact run fixture-run-1")
        });
        let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
        db.execute_batch(if mode=="intent" {"CREATE TRIGGER fail_cancel BEFORE UPDATE ON cloud_creations WHEN json_extract(NEW.metadata,'$.cancel_requested')=1 BEGIN SELECT RAISE(ABORT,'fixture full'); END"} else {"CREATE TRIGGER fail_cancel BEFORE UPDATE ON cloud_creations WHEN json_extract(NEW.metadata,'$.cancel_claimed')=1 BEGIN SELECT RAISE(ABORT,'fixture full'); END"}).unwrap();
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "/cancel".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture.text(&session).contains(if mode == "intent" {
                "Cancellation not durably acknowledged"
            } else {
                "Cancellation intent durably retained"
            })
        });
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "CancelRun")
                .count(),
            0
        );
        assert!(
            !fixture
                .text(&session)
                .contains("Cancellation requested for exact run")
        );
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        assert_eq!(
            sessions
                .iter()
                .find(|s| s.id == session.id)
                .unwrap()
                .recovery
                .as_ref()
                .unwrap()
                .unavailable
                .as_ref()
                .unwrap()
                .contains("cancellation intent retained"),
            mode == "claim"
        );
        db.execute_batch("DROP TRIGGER fail_cancel").unwrap();
        fs::write(marker.with_extension("release"), "release").unwrap();
        fixture.until(|| fixture.text(&session).contains("Ready: exact run"));
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "CancelRun")
                .count(),
            0,
            "failed attempt must not automatically retry"
        );
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "/detach".into(),
            submit: true,
        });
    }
}

#[test]
fn managed_lost_reply_cancel_survives_restart_and_discard_cleans_crash_aliases_without_remote_calls()
 {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let path = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["drop_send_reply"] = json!(true);
    fs::write(path, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "retain this unresolved prompt".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Observation unknown/unverified")
    });
    assert_eq!(
        fixture.live.request(Request::CancelCloudCreation {
            session: session.id,
            expected_run: session.run
        }),
        Response::Ok
    );
    let root = fixture.live.config.join("cloud-creations");
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "Cancel alone preserves unresolved content"
    );
    fixture.live.request(Request::Shutdown { kill: true });
    fixture.live.join();
    let payload = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    fs::hard_link(payload, root.join(format!(".{}", "a".repeat(64)))).unwrap();
    fixture
        .live
        .start_binary_env(&[("OVRCR_CURSOR_CLOUD_FIXTURE", fixture.config.as_os_str())]);
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
            .contains("cancellation intent retained")
    );
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "restart removes owned temporary alias, retaining unresolved canonical bytes"
    );
    assert_eq!(
        fixture.live.request(Request::DiscardCloudCreation {
            session: session.id,
            expected_run: row.run
        }),
        Response::Ok
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "CreateAgent")
            .count(),
        1
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send")
            .count(),
        1
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "CancelRun")
            .count(),
        0
    );
}

#[test]
fn managed_discard_cleanup_failure_is_not_success_and_restart_retires_all_owned_copies() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let path = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["drop_send_reply"] = json!(true);
    fs::write(path, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "private unresolved bytes".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Observation unknown/unverified")
    });
    let root = fixture.live.config.join("cloud-creations");
    let payload = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    fs::hard_link(payload, root.join(format!(".{}", "a".repeat(64)))).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        fixture.live.request(Request::DiscardCloudCreation {
            session: session.id,
            expected_run: session.run
        }),
        Response::Error { .. }
    ));
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        2,
        "failed cleanup cannot claim removal"
    );
    fixture.live.request(Request::Shutdown { kill: true });
    fixture.live.join();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    fixture
        .live
        .start_binary_env(&[("OVRCR_CURSOR_CLOUD_FIXTURE", fixture.config.as_os_str())]);
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    assert!(
        sessions
            .iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .recovery
            .as_ref()
            .unwrap()
            .unavailable
            .as_ref()
            .unwrap()
            .contains("discarded locally")
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send")
            .count(),
        1
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "CancelRun")
            .count(),
        0
    );
}

#[test]
fn managed_known_run_stream_failure_preserves_exact_live_cancel_target() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let path = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["send_scenario"] = json!("unknown_shape");
    fs::write(path, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "inspect".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Observation unknown/unverified")
    });
    assert!(
        fixture
            .text(&session)
            .contains("Observing exact run fixture-run-1")
    );
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/cancel".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cancellation requested for exact run fixture-run-1")
    });
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "CancelRun")
            .count(),
        1
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "Send")
            .count(),
        1
    );
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/detach".into(),
        submit: true,
    });
}

#[test]
fn managed_recovery_reconciles_exact_completed_run_without_replay_or_ready() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained:")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "Inspect source".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Ready: exact run fixture-run-1")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/detach".into(),
        submit: true,
    });
    fixture.until(|| {
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        !sessions
            .iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .phase
            .is_live()
    });
    let bridge_config = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&bridge_config).unwrap()).unwrap();
    config["recovery_runs"] = json!({"fixture-run-1":"RUN_LIFECYCLE_STATUS_FINISHED"});
    fs::write(bridge_config, config.to_string()).unwrap();
    let settings_path = fixture.live.config.join("dashboard.toml");
    let mut settings: ovrcr::protocol::Settings =
        toml::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    settings.cursor_cloud_accounts[0].model = "changed-local-default".into();
    fs::write(settings_path, toml::to_string(&settings).unwrap()).unwrap();
    let reply = fixture.live.request(Request::ReopenSession {
        session: session.id,
        expected_run: session.run,
        acknowledge_stopped: true,
    });
    let Response::CreatedSession(reopened) = reply else {
        panic!("exact-run reconciliation refused: {reply:?}")
    };
    fixture.live.own_group(reopened.pid.unwrap() as i32);
    assert_eq!(reopened.id, session.id);
    fixture.until(|| {
        fixture
            .text(&reopened)
            .contains("Reconciled exact run fixture-run-1: Finished")
    });
    assert!(!fixture.text(&reopened).contains("Ready: exact run"));
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let row = sessions.iter().find(|s| s.id == session.id).unwrap();
    assert!(
        row.unread.is_none(),
        "historical reconciliation cannot produce Unread"
    );
    assert!(
        row.agent.as_ref().is_none_or(|a| a.effective_activity()
            != ovrcr::protocol::AgentActivity::ResponseReady
            && a.input_requests.is_empty()),
        "historical reconciliation cannot publish Ready or input requests"
    );
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "exit".into(),
        submit: true,
    });
    fixture.until(|| {
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        !sessions
            .iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .phase
            .is_live()
    });
    let closed = fixture
        .live
        .command()
        .args(["terminal", "close", &session.id.0.to_string()])
        .output()
        .unwrap();
    assert!(closed.status.success(), "{:?}", closed);
    assert!(
        String::from_utf8_lossy(&closed.stderr).contains("Remote work and charges may continue"),
        "close must warn on stderr: {:?}",
        closed
    );
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    assert!(
        sessions
            .iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .archived
    );
    let events = fixture.events();
    for (method, count) in [
        ("CreateAgent", 1),
        ("Send", 1),
        ("GetRun", 2),
        ("CancelRun", 0),
        ("ObserveRun", 0),
        ("ResumeAgent", 0),
    ] {
        assert_eq!(
            events.iter().filter(|e| e["method"] == method).count(),
            count,
            "{method}"
        );
    }
}

fn recovered_active_cancel(mode: &str) {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let bridge_config = fixture.live.root.path().join("bridge.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&bridge_config).unwrap()).unwrap();
    config["send_scenario"] = json!("unknown_shape");
    fs::write(&bridge_config, config.to_string()).unwrap();
    let session = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Cloud handle durably retained:")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "Inspect source".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&session)
            .contains("Observation unknown/unverified:")
    });
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/detach".into(),
        submit: true,
    });
    fixture.until(|| {
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
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
        fixture.live.request(Request::CancelCloudCreation {
            session: session.id,
            expected_run: session.run
        }),
        Response::Ok
    );
    fixture.live.request(Request::Shutdown { kill: true });
    fixture.live.join();
    fixture
        .live
        .start_binary_env(&[("OVRCR_CURSOR_CLOUD_FIXTURE", fixture.config.as_os_str())]);
    config["recovery_runs"] = json!({"fixture-run-1":"RUN_LIFECYCLE_STATUS_RUNNING"});
    config["cancel_snapshot_outcome"] = json!("RUN_LIFECYCLE_STATUS_FINISHED");
    fs::write(&bridge_config, config.to_string()).unwrap();
    let Response::CreatedSession(reopened) = fixture.live.request(Request::ReopenSession {
        session: session.id,
        expected_run: session.run,
        acknowledge_stopped: true,
    }) else {
        panic!("reopen")
    };
    fixture.live.own_group(reopened.pid.unwrap() as i32);
    fixture.until(|| {
        fixture
            .text(&reopened)
            .contains("Active reattachment unavailable")
    });
    assert!(
        fixture
            .text(&reopened)
            .contains("Saved cancellation intent retained")
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "CancelRun")
            .count(),
        0
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|e| e["method"] == "ObserveRun")
            .count(),
        0
    );
    if mode == "discard" {
        assert!(
            fixture
                .live
                .command()
                .args(["terminal", "cloud-discard", &session.id.0.to_string()])
                .status()
                .unwrap()
                .success()
        );
    }
    if mode == "storage" {
        let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER fail_recovery_cancel BEFORE UPDATE ON cloud_creations WHEN json_extract(NEW.metadata,'$.recovery_cancel_claimed')=1 BEGIN SELECT RAISE(ABORT,'fixture full'); END;").unwrap();
    }
    fixture.live.request(Request::SendTerminal {
        session: session.id,
        text: "/retry-cancel".into(),
        submit: true,
    });
    if mode != "completion" {
        fixture.until(|| {
            fixture
                .text(&reopened)
                .contains("Cursor cloud validation failed:")
        });
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "CancelRun")
                .count(),
            0,
            "{mode}"
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|e| e["method"] == "Send")
                .count(),
            1,
            "{mode}"
        );
        return;
    }
    fixture.until(|| {
        fixture
            .text(&reopened)
            .contains("Reconciled exact run fixture-run-1: Finished")
    });
    let methods = fixture.events();
    assert_eq!(
        methods
            .iter()
            .filter(|e| e["method"] == "CancelRun")
            .count(),
        1
    );
    assert_eq!(methods.iter().filter(|e| e["method"] == "Send").count(), 1);
    assert!(!fixture.text(&reopened).contains("Ready: exact run"));
}

#[test]
fn managed_recovery_duplicate_conversation_identifies_original_session() {
    let fixture = Fixture::new("composer-2.5", true, true, false);
    let original = fixture.launch();
    fixture.until(|| {
        fixture
            .text(&original)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: original.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&original)
            .contains("Cloud handle durably retained:")
    });
    let Response::CreatedSession(second) =
        fixture
            .live
            .request(Request::CreateSession(CreateSessionRequest {
                project: PROJECT.into(),
                workspace: WORKSPACE.into(),
                name: "second-cloud".into(),
                label: None,
                argv: vec![
                    fixture.live.executable.clone().into_os_string(),
                    "agent".into(),
                    "run".into(),
                    "cursor-cloud".into(),
                    "--".into(),
                    "work".into(),
                ],
                kind: SessionKind::Agent {
                    name: "cursor-cloud:work".into(),
                },
            }))
    else {
        panic!("second session")
    };
    fixture.live.own_group(second.pid.unwrap() as i32);
    fixture.until(|| {
        fixture
            .text(&second)
            .contains("Press Enter to close preview.")
    });
    fixture.live.request(Request::SendTerminal {
        session: second.id,
        text: "prepare".into(),
        submit: true,
    });
    fixture.until(|| {
        fixture
            .text(&second)
            .contains("Cursor cloud validation failed:")
    });
    assert!(
        !fixture
            .text(&second)
            .contains("Cloud handle durably retained:")
    );
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let reason = sessions
        .iter()
        .find(|s| s.id == second.id)
        .unwrap()
        .recovery
        .as_ref()
        .unwrap()
        .unavailable
        .as_ref()
        .unwrap();
    assert!(
        reason.contains(&format!("Session {}", original.id.0)),
        "{reason}"
    );
}

#[test]
fn managed_recovery_account_source_and_unknown_outcomes_fail_closed() {
    for mode in [
        "profile",
        "credential",
        "source",
        "identity",
        "status",
        "missing",
    ] {
        let fixture = Fixture::new("composer-2.5", true, true, false);
        let bridge_config = fixture.live.root.path().join("bridge.json");
        let session = fixture.launch();
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Press Enter to close preview.")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "prepare".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Cloud handle durably retained:")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "Inspect source".into(),
            submit: true,
        });
        fixture.until(|| {
            fixture
                .text(&session)
                .contains("Ready: exact run fixture-run-1")
        });
        fixture.live.request(Request::SendTerminal {
            session: session.id,
            text: "/detach".into(),
            submit: true,
        });
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
        let mut config: Value = serde_json::from_slice(&fs::read(&bridge_config).unwrap()).unwrap();
        config["recovery_runs"] = json!({"fixture-run-1":if mode=="status" {"UNKNOWN_STATUS"} else {"RUN_LIFECYCLE_STATUS_FINISHED"}});
        if mode == "identity" {
            config["send_scenario"] = json!("wrong_snapshot");
        }
        if mode == "missing" {
            config["recovery_missing"] = json!(true);
        }
        fs::write(&bridge_config, config.to_string()).unwrap();
        if mode == "profile" {
            let path = fixture.live.config.join("dashboard.toml");
            let mut settings: ovrcr::protocol::Settings =
                toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            settings.cursor_cloud_accounts[0].keychain_account = "other".into();
            fs::write(path, toml::to_string(&settings).unwrap()).unwrap();
        }
        if mode == "credential" {
            let mut value: Value =
                serde_json::from_slice(&fs::read(&fixture.config).unwrap()).unwrap();
            value["credential_changed"] = json!(true);
            fs::write(&fixture.config, value.to_string()).unwrap();
        }
        if mode == "source" {
            live::git(
                &fixture.workspace,
                &[
                    "remote",
                    "set-url",
                    "origin",
                    "https://github.com/acme/other",
                ],
            );
        }
        let Response::CreatedSession(reopened) = fixture.live.request(Request::ReopenSession {
            session: session.id,
            expected_run: session.run,
            acknowledge_stopped: true,
        }) else {
            panic!("reopen {mode}")
        };
        fixture.live.own_group(reopened.pid.unwrap() as i32);
        fixture.until(|| {
            fixture
                .text(&reopened)
                .contains("Cursor cloud validation failed:")
        });
        assert!(
            !fixture.text(&reopened).contains("Reconciled exact run"),
            "{mode}"
        );
        for (method, count) in [
            ("CreateAgent", 1),
            ("Send", 1),
            ("CancelRun", 0),
            ("ObserveRun", 0),
            ("ResumeAgent", 0),
        ] {
            assert_eq!(
                fixture
                    .events()
                    .iter()
                    .filter(|e| e["method"] == method)
                    .count(),
                count,
                "{mode} {method}"
            );
        }
    }
}

#[test]
fn managed_recovery_active_retry_cancel_is_explicit_and_uses_independent_snapshot() {
    recovered_active_cancel("completion");
}
#[test]
fn managed_recovery_discard_wins_before_explicit_cancel_claim() {
    recovered_active_cancel("discard");
}
#[test]
fn managed_recovery_failed_cancel_claim_permits_zero_cancel_rpc() {
    recovered_active_cancel("storage");
}
