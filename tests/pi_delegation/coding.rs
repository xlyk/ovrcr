//! Registered Pi tools through the real control client, binary Server and Git/PTY owners.
use super::*;

fn arguments(workspace: &str) -> serde_json::Value {
    serde_json::json!({
        "project": PROJECT, "workspace": workspace, "scope": "Implement the bounded fixture change.",
        "model_provider": "test", "model": "fixture-model", "acceptance_evidence": ["immutable candidate and actual checks"],
        "permitted_actions": ["read", "write", "edit", "check"],
        "coding": {"requested_base": "main", "branch": format!("coding/{workspace}"), "authorized_paths": ["README", "change.txt"], "required_checks": ["fixture-failure", "required-but-missing", "deliberately-unrun"]}
    })
}

fn start(fixture: &RecoveryFixture, workspace: &str) -> AssignmentSnapshot {
    serde_json::from_value(fixture.tool("ovrcr_start", arguments(workspace))).unwrap()
}

fn complete(fixture: &RecoveryFixture, started: &AssignmentSnapshot) -> AssignmentSnapshot {
    fs::write(fixture.live.root.path().join("allow-result"), "1").unwrap();
    wait_file(&started.brief.with_extension("json.published"));
    serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":started.handle}),
    ))
    .unwrap()
}

#[test]
fn public_coding_uses_distinct_workspaces_and_reuses_safe_child_for_followup() {
    let fixture = super::followups::OwnedFollowup::new();
    let root_bytes = fs::read(fixture.live.repo.join("README")).unwrap();
    let parent_workspace = cli_json(
        &fixture.live,
        &[
            "workspace",
            "get",
            "--project",
            PROJECT,
            "--branch",
            "recovery",
        ],
    );
    let parent_bytes =
        fs::read(Path::new(parent_workspace["path"].as_str().unwrap()).join("README")).unwrap();
    let inventory = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]);
    let parent_path = Path::new(parent_workspace["path"].as_str().unwrap());
    let parent_generation = ovrcr::git::worktree_identity(parent_path).unwrap();
    let parent_head = live::git(parent_path, &["rev-parse", "HEAD"]);
    let research: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    assert_eq!(
        ovrcr::git::worktree_identity(parent_path).unwrap(),
        parent_generation
    );
    assert_eq!(live::git(parent_path, &["rev-parse", "HEAD"]), parent_head);
    assert!(research.resources.coding.is_none());
    let after_research = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]);
    let mut expected_inventory = inventory.clone();
    let parent_row = expected_inventory
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == fixture.workspace)
        .unwrap();
    parent_row["terminal_count"] = (parent_row["terminal_count"].as_u64().unwrap() + 1).into();
    assert_eq!(
        after_research, expected_inventory,
        "research adds exactly one terminal to its existing workspace, preserving all workspace identities"
    );
    assert_eq!(
        live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        inventory.as_array().unwrap().len(),
        "read-only research does not create an unregistered worktree"
    );
    let first = start(&fixture, "coding-one");
    let second = start(&fixture, "coding-two");
    assert_eq!(first.dispatch, DispatchStatus::Applied, "{first:?}");
    assert_eq!(second.dispatch, DispatchStatus::Applied, "{second:?}");
    assert_ne!(first.identity.session, second.identity.session);
    let first_workspace = first
        .resources
        .coding
        .as_ref()
        .expect("actual workspace identity");
    let second_workspace = second.resources.coding.as_ref().unwrap();
    assert_ne!(first_workspace.path, second_workspace.path);
    assert_ne!(first_workspace.git_identity, second_workspace.git_identity);
    assert_eq!(first_workspace.creation, WorkspaceCreation::Registered);
    assert_eq!(
        first_workspace.base,
        live::git(&fixture.live.repo, &["rev-parse", "main"]).trim()
    );
    let first = complete(&fixture, &first);
    let second = complete(&fixture, &second);
    for child in [&first, &second] {
        let argv: Vec<String> = serde_json::from_slice(
            &fs::read(child.brief.with_extension("json.native-argv")).unwrap(),
        )
        .unwrap();
        for flag in ["--no-approve", "--no-extensions", "--no-skills"] {
            assert!(argv.iter().any(|arg| arg == flag), "{argv:?}");
        }
        assert!(!argv.iter().any(|arg| arg == "--approve"));
        let tools = argv.iter().position(|arg| arg == "--tools").unwrap();
        assert_eq!(argv[tools + 1], "read,write,edit,bash,grep,find,ls");
    }
    assert_eq!(
        fs::read(fixture.live.repo.join("README")).unwrap(),
        root_bytes
    );
    assert_eq!(
        fs::read(Path::new(parent_workspace["path"].as_str().unwrap()).join("README")).unwrap(),
        parent_bytes
    );
    assert_ne!(
        fs::read(
            first
                .resources
                .coding
                .as_ref()
                .unwrap()
                .path
                .join("change.txt")
        )
        .unwrap(),
        fs::read(
            second
                .resources
                .coding
                .as_ref()
                .unwrap()
                .path
                .join("change.txt")
        )
        .unwrap()
    );
    let checks = &first
        .submission
        .as_ref()
        .unwrap()
        .coding
        .as_ref()
        .unwrap()
        .checks;
    assert_eq!(
        checks
            .iter()
            .find(|check| check.name == "fixture-failure")
            .unwrap()
            .outcome,
        CheckOutcome::Failed
    );
    assert_eq!(
        checks
            .iter()
            .find(|check| check.name == "required-but-missing")
            .unwrap()
            .outcome,
        CheckOutcome::Missing
    );
    assert_eq!(
        checks
            .iter()
            .find(|check| check.name == "deliberately-unrun")
            .unwrap()
            .outcome,
        CheckOutcome::Unrun
    );
    assert_eq!(first.acceptance, "pending");
    let candidate = first
        .submission
        .as_ref()
        .unwrap()
        .coding
        .as_ref()
        .unwrap()
        .candidate
        .as_ref()
        .unwrap();
    let bytes = fs::read(&candidate.path).unwrap();
    assert!(
        !candidate
            .path
            .starts_with(&first.resources.coding.as_ref().unwrap().path)
    );
    let publication: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let snapshot: ovrcr::git::candidate::CandidateSnapshot =
        serde_json::from_value(publication["snapshot"].clone()).unwrap();
    assert!(snapshot.files.iter().any(|file| file.path == "change.txt"));
    let clarified: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_followup", serde_json::json!({"handle":first.handle,"expected_identity":first.identity,"expected_result_version":first.result_version,"expected_control_epoch":first.control.epoch,"text":"Repair the bounded fixture change using the same workspace."}))).unwrap();
    let repaired = complete(&fixture, &clarified);
    assert_eq!(repaired.identity, first.identity);
    assert_eq!(repaired.conversation, first.conversation);
    assert_eq!(repaired.resources.coding, first.resources.coding);
    assert_eq!(repaired.result_version, 2);
    assert_eq!(
        fs::read(&candidate.path).unwrap(),
        bytes,
        "prior candidate stays immutable after repair"
    );
    let historical: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":first.handle,"version":first.result_version}),
    ))
    .unwrap();
    assert_eq!(
        historical.historical_submission, first.submission,
        "selected original contract/result determines historical evidence"
    );
    assert_eq!(historical.selected_version, Some(first.result_version));
}

#[test]
fn public_coding_reload_retires_old_source_without_retagging_candidate() {
    let fixture = super::followups::OwnedFollowup::new();
    let first = complete(&fixture, &start(&fixture, "reload-coding"));
    let candidate = first
        .submission
        .as_ref()
        .unwrap()
        .coding
        .as_ref()
        .unwrap()
        .candidate
        .as_ref()
        .unwrap();
    let candidate_bytes = fs::read(&candidate.path).unwrap();
    let result_bytes = fs::read(&first.result_path).unwrap();
    let before: WorkflowSnapshot =
        serde_json::from_value(fixture.tool("ovrcr_recover", serde_json::json!({}))).unwrap();
    let DelegationRequest::Prepare { mut auth, .. } =
        fixture.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: fixture.parent,
            text: format!(
                "session_replace:{}:reload",
                before.authority.binding.conversation
            ),
            submit: true
        }),
        Response::Ok
    );
    let current: WorkflowSnapshot =
        serde_json::from_value(fixture.tool("ovrcr_recover", serde_json::json!({}))).unwrap();
    assert_eq!(current.authority.workflow, before.authority.workflow);
    assert_eq!(current.authority.epoch, before.authority.epoch + 1);
    assert_ne!(current.authority.producer, before.authority.producer);
    // Even the public new fence cannot upgrade a retired or omitted source key.
    auth.controller = Some(current.authority.fence());
    for source in [auth.source.clone(), None] {
        auth.source = source;
        assert!(matches!(
            fixture
                .live
                .request(Request::Delegation(Box::new(DelegationRequest::Collect {
                    auth: auth.clone(),
                    handle: first.handle.clone()
                }))),
            Response::Error { .. }
        ));
    }
    let collected: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_collect", serde_json::json!({"handle":first.handle})),
    )
    .unwrap();
    assert_eq!(
        collected.identity, first.identity,
        "current owner reads the original child provenance"
    );
    assert_eq!(collected.receipts, first.receipts);
    assert_eq!(collected.submission, first.submission);
    assert_eq!(fs::read(&candidate.path).unwrap(), candidate_bytes);
    assert_eq!(fs::read(&first.result_path).unwrap(), result_bytes);
}

#[test]
fn public_coding_refuses_root_existing_id_and_traversal_without_git_effects() {
    let fixture = super::followups::OwnedFollowup::new();
    let before = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]);
    let root = before
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| {
            workspace["path"]
                .as_str()
                .is_some_and(|path| Path::new(path) == fixture.live.repo)
        })
        .expect("registered protected root")["id"]
        .as_str()
        .unwrap();
    let git_before = live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"]);
    for workspace in [fixture.workspace.as_str(), root, "../escape"] {
        let result = fixture.tool("ovrcr_start", serde_json::json!({"project":PROJECT,"workspace":workspace,"scope":"Bounded code", "model_provider":"test","model":"fixture-model","acceptance_evidence":["candidate"],"permitted_actions":["read","write","edit","check"],"coding":{"requested_base":"main","branch":"coding/refused","authorized_paths":["."],"required_checks":["check"]}}));
        assert_eq!(result["dispatch"], "Refused", "{result}");
        let handle = result["handle"].as_str().unwrap();
        let request = fixture.proxy.requests.lock().unwrap().iter()
            .find(|request| matches!(request, DelegationRequest::Prepare { contract, .. } if contract.request == handle))
            .unwrap().clone();
        let expected = if workspace == "../escape" {
            ErrorCode::InvalidRequest
        } else {
            ErrorCode::AlreadyExists
        };
        let Response::Error { code, .. } =
            fixture.live.request(Request::Delegation(Box::new(request)))
        else {
            panic!("invalid coding preparation must fail before assignment persistence");
        };
        assert_eq!(code, expected);
        let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
        let rows: i64 = db
            .query_row(
                "SELECT count(*) FROM delegations WHERE handle=?1",
                [handle],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0, "definite refusal creates no retained assignment");
    }
    assert_eq!(
        live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"]),
        git_before
    );
    assert_eq!(fixture.child_count(), 0);
    assert_eq!(
        cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]),
        before
    );
    assert!(
        !fixture
            .live
            .workspace_root
            .parent()
            .unwrap()
            .join("escape")
            .exists()
    );
}

#[test]
fn public_coding_lost_launch_reply_reconciles_one_workspace_and_one_child() {
    let fixture = super::followups::OwnedFollowup::new();
    let before = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT])
        .as_array()
        .unwrap()
        .len();
    fixture.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 1,
        fault: None,
    });
    let first = fixture.tool("ovrcr_start", arguments("lost-reply"));
    assert_eq!(
        first["dispatch"], "Unknown",
        "lost reply must not invent known delivery"
    );
    let handle = first["handle"].as_str().unwrap();
    let allocated = fixture.reconcile(handle);
    let workspace = allocated
        .resources
        .coding
        .as_ref()
        .expect("workspace survives lost reply")
        .clone();
    assert_eq!(workspace.creation, WorkspaceCreation::Registered);
    assert!(workspace.path.is_dir());
    assert!(allocated.identity.session.is_some());
    let recovered: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
    assert_eq!(recovered.identity, allocated.identity);
    assert_eq!(recovered.resources.coding, Some(workspace));
    let completed = complete(&fixture, &recovered);
    let again: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
    assert_eq!(again.identity, completed.identity);
    assert_eq!(
        fixture.input_count(&completed),
        1,
        "initial input cannot be duplicated during reconciliation"
    );
    assert_eq!(
        cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT])
            .as_array()
            .unwrap()
            .len(),
        before + 1
    );
}

#[test]
fn public_coding_historical_selection_uses_original_contract_after_revision() {
    let fixture = super::followups::OwnedFollowup::new();
    let first = complete(&fixture, &start(&fixture, "contract-history"));
    let root = first.brief.parent().unwrap();
    let contract_bytes = fs::read(root.join("contract-v1.json")).unwrap();
    let result_bytes = fs::read(&first.result_path).unwrap();
    let candidate = first
        .submission
        .as_ref()
        .unwrap()
        .coding
        .as_ref()
        .unwrap()
        .candidate
        .as_ref()
        .unwrap();
    let candidate_bytes = fs::read(&candidate.path).unwrap();
    let mut revised: ResearchContract = serde_json::from_slice(&contract_bytes).unwrap();
    revised.scope = "Repair only the assigned fixture change and explain the correction.".into();
    revised.acceptance_evidence = vec!["revised correction evidence".into()];
    let next: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&fixture, &first, Some(revised)))
            .unwrap();
    assert_eq!(next.identity.contract, first.identity.contract + 1);
    assert_eq!(next.result_version, first.result_version + 1);
    assert_eq!(next.resources.coding, first.resources.coding);
    assert_eq!(next.conversation, first.conversation);
    let next = complete(&fixture, &next);
    let revised_bytes = fs::read(root.join("contract-v2.json")).unwrap();
    assert_ne!(contract_bytes, revised_bytes);
    let historical: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":next.handle,"version":first.result_version}),
    ))
    .unwrap();
    assert_eq!(historical.identity, next.identity);
    assert_eq!(historical.selected_version, Some(first.result_version));
    assert!(historical.submission.is_none());
    assert_eq!(historical.historical_submission, first.submission);
    assert_eq!(fs::read(&candidate.path).unwrap(), candidate_bytes);
    assert_eq!(fs::read(&first.result_path).unwrap(), result_bytes);
    assert_eq!(
        fs::read(root.join("contract-v1.json")).unwrap(),
        contract_bytes
    );
    // Historical collection must authenticate the selected contract, even though
    // the current contract and its candidate remain valid.
    fs::write(
        root.join("contract-v1.json"),
        b"{changed historical contract",
    )
    .unwrap();
    let refused = tool_reply(
        &fixture.live,
        fixture.parent,
        "ovrcr_collect",
        serde_json::json!({"handle":next.handle,"version":first.result_version}),
        &fixture
            .live
            .root
            .path()
            .join("bad-historical-contract.json"),
    );
    fs::write(root.join("contract-v1.json"), &contract_bytes).unwrap();
    assert!(refused["error"].is_string(), "{refused}");
    let current: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_collect", serde_json::json!({"handle":next.handle})),
    )
    .unwrap();
    assert_eq!(current.submission, next.submission);
    assert_eq!(
        fs::read(root.join("contract-v2.json")).unwrap(),
        revised_bytes
    );
    assert_eq!(fixture.child_count(), 1);
}

#[test]
fn public_coding_missing_or_corrupt_candidate_refuses_without_new_effects() {
    let fixture = super::followups::OwnedFollowup::new();
    let original = complete(&fixture, &start(&fixture, "candidate-storage"));
    let candidate = original
        .submission
        .as_ref()
        .unwrap()
        .coding
        .as_ref()
        .unwrap()
        .candidate
        .as_ref()
        .unwrap();
    let candidate_bytes = fs::read(&candidate.path).unwrap();
    let result_bytes = fs::read(&original.result_path).unwrap();
    let input_bytes = fixture.byte_count(&original);
    let workspaces = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]);
    for missing in [true, false] {
        if missing {
            fs::remove_file(&candidate.path).unwrap();
        } else {
            fs::write(&candidate.path, b"{partial candidate").unwrap();
        }
        let refused = tool_reply(
            &fixture.live,
            fixture.parent,
            "ovrcr_collect",
            serde_json::json!({"handle":original.handle}),
            &fixture
                .live
                .root
                .path()
                .join(format!("candidate-refused-{missing}.json")),
        );
        // Restore before assertions so failure keeps ordinary fixture cleanup viable.
        fs::write(&candidate.path, &candidate_bytes).unwrap();
        assert!(refused["error"].is_string(), "{refused}");
        let recovered: AssignmentSnapshot =
            serde_json::from_value(fixture.recover(&original.handle)).unwrap();
        assert_eq!(recovered.identity, original.identity);
        assert_eq!(recovered.receipts, original.receipts);
        assert_eq!(recovered.resources.coding, original.resources.coding);
        assert_eq!(fixture.byte_count(&original), input_bytes);
        assert_eq!(fixture.child_count(), 1);
        assert_eq!(fs::read(&original.result_path).unwrap(), result_bytes);
        assert_eq!(
            cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]),
            workspaces
        );
    }
    assert_eq!(fs::read(&candidate.path).unwrap(), candidate_bytes);
}

#[test]
fn public_coding_return_barrier_and_stale_control_epoch_cannot_start_repair() {
    let fixture = super::followups::OwnedFollowup::new();
    let original = complete(&fixture, &start(&fixture, "barrier-coding"));
    let candidate = original
        .submission
        .as_ref()
        .unwrap()
        .coding
        .as_ref()
        .unwrap()
        .candidate
        .as_ref()
        .unwrap();
    let candidate_bytes = fs::read(&candidate.path).unwrap();
    let result_bytes = fs::read(&original.result_path).unwrap();
    let before = fixture.byte_count(&original);
    let child = original.identity.session.unwrap();
    let run = original.identity.run.unwrap();
    let mut dashboard = connect_server(&fixture.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            2,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: child,
                        run,
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: Some(child),
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            3,
            Request::AssignmentControl(AssignmentControlRequest::Takeover {
                request: "take-coding".into(),
                session: child,
                run,
                expected_epoch: original.control.epoch,
                view_revision: 1,
            })
        )
        .unwrap(),
        Response::Ok
    );
    let taken = fixture.reconcile(&original.handle);
    assert_eq!(taken.control.owner, ControlOwner::Human);
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            4,
            Request::AssignmentControl(AssignmentControlRequest::Return {
                request: "return-coding".into(),
                session: child,
                run,
                expected_epoch: taken.control.epoch,
                view_revision: 1,
                evidence: Box::new(taken.control_evidence.clone().unwrap()),
            })
        )
        .unwrap(),
        Response::Ok
    );
    let returned = fixture.reconcile(&original.handle);
    assert!(matches!(
        returned.control.barrier,
        Some(ControlBarrier::HumanReturn { .. })
    ));
    for epoch in [original.control.epoch, returned.control.epoch] {
        let refused = fixture.tool("ovrcr_followup",serde_json::json!({
            "handle":original.handle,"expected_identity":original.identity,"expected_result_version":original.result_version,
            "expected_control_epoch":epoch,"text":"Repair must remain blocked until explicit return confirmation."}));
        assert_eq!(refused["dispatch"], "Refused", "{refused}");
        let current = fixture.reconcile(&original.handle);
        assert_eq!(current.result_version, original.result_version);
        assert_eq!(current.resources.coding, original.resources.coding);
        assert_eq!(current.control, returned.control);
    }
    assert_eq!(fixture.byte_count(&original), before);
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(fs::read(&candidate.path).unwrap(), candidate_bytes);
    assert_eq!(fs::read(&original.result_path).unwrap(), result_bytes);
}

#[test]
fn public_coding_blocked_git_keeps_ordinary_input_view_and_observe_responsive() {
    let fixture = super::followups::OwnedFollowup::new();
    let original = complete(&fixture, &start(&fixture, "observe-coding"));
    let DelegationRequest::Prepare { auth, .. } =
        fixture.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let ordinary = sessions
        .iter()
        .find(|session| session.name == "local")
        .unwrap();
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap();
    let entered = fixture.live.root.path().join("coding-git-entered");
    let release = fixture.live.root.path().join("coding-git-release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let _release = Release(release.clone());
    let shim = fixture.live.claude.command.parent().unwrap().join("git");
    fs::write(&shim,format!("#!/bin/sh\ncase \"$*\" in *\"worktree add\"*\"blocked-coding\"*) : > '{}'; while [ ! -f '{}' ]; do sleep 0.01; done;; esac\nexec '{}' \"$@\"\n",entered.display(),release.display(),git.display())).unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    let output = fixture.live.root.path().join("blocked-coding-result.json");
    assert_eq!(fixture.live.request(Request::SendTerminal { session:fixture.parent, text:format!("tool:{}",
        serde_json::json!({"name":"ovrcr_start","args":arguments("blocked-coding"),"output":output,"background":true})), submit:true }),Response::Ok);
    wait_file(&entered);
    let mut dashboard = connect_server(&fixture.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            2,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: ordinary.id,
                        run: ordinary.run,
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: Some(ordinary.id)
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    let marker = fixture.live.root.path().join("coding-input-responsive");
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            3,
            Request::Input {
                session: ordinary.id,
                run: ordinary.run,
                bytes: format!("printf delivered > '{}'\r", marker.display()).into_bytes()
            }
        )
        .unwrap(),
        Response::Ok
    );
    wait_file(&marker);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "delivered");
    // Bypass only the serial fault proxy: use the same production protocol/auth
    // while Launch's real CLI control request is held inside worktree creation.
    let observed = live::request_with_timeout(
        &fixture.live.socket,
        4,
        Request::Delegation(Box::new(DelegationRequest::Observe {
            auth,
            handles: vec![original.handle.clone()],
            after: None,
            wait_ms: 0,
        })),
        Duration::from_secs(1),
    );
    let Some(Response::DelegationObservation(observed)) = observed else {
        panic!("Observe blocked behind Git: {observed:?}")
    };
    assert_eq!(observed.observations.len(), 1);
    assert_eq!(observed.observations[0].identity, original.identity);
    assert!(
        !release.exists(),
        "input/view/Observe must finish before the Git gate releases"
    );
    assert!(
        !output.exists(),
        "coding create is still held at the forced Git boundary"
    );
    fs::write(&release, "release").unwrap();
    wait_file(&output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(reply.get("error").is_none(), "{reply}");
    let created: AssignmentSnapshot = serde_json::from_value(reply["details"].clone()).unwrap();
    assert_eq!(created.dispatch, DispatchStatus::Applied);
    assert_ne!(created.resources.coding, original.resources.coding);
    assert_eq!(fixture.child_count(), 2);
}

// Supporting coverage of the normal combined path reused by coding Launch.
// The assignment stage/retry cases below cover the retained coding metadata.
#[test]
fn combined_workspace_launch_refusal_retains_exact_registered_git_generation() {
    let fixture = super::followups::OwnedFollowup::new();
    let before = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]);
    let root_bytes = fs::read(fixture.live.repo.join("README")).unwrap();
    let children = fixture.child_count();
    let request = Request::CreateWorkspaceWithLaunch {
        project: PROJECT.into(),
        id: "combined-no-process".into(),
        branch: BranchRequest::New {
            branch: "combined-no-process".into(),
            base: "main".into(),
        },
        launch: Some(SessionLaunch {
            argv: vec![
                fixture
                    .live
                    .root
                    .path()
                    .join("absent-executable")
                    .into_os_string(),
            ],
            label: None,
            kind: SessionKind::Agent { name: "pi".into() },
        }),
    };
    let failed = fixture.live.request(request.clone());
    assert!(
        matches!(
            &failed,
            Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            }
        ),
        "{failed:?}"
    );
    let created = cli_json(
        &fixture.live,
        &[
            "workspace",
            "get",
            "--project",
            PROJECT,
            "--branch",
            "combined-no-process",
        ],
    );
    let path = Path::new(created["path"].as_str().unwrap());
    assert!(path.is_dir());
    let generation = ovrcr::git::worktree_identity(path).unwrap();
    assert!(!generation.is_empty());
    assert_eq!(
        live::git(path, &["rev-parse", "HEAD"]).trim(),
        live::git(&fixture.live.repo, &["rev-parse", "main"]).trim()
    );
    let Response::Error { message, .. } = failed else {
        unreachable!()
    };
    assert!(
        message.contains(path.to_str().unwrap()),
        "partial failure must identify its surviving worktree: {message}"
    );
    assert_eq!(
        fixture.child_count(),
        children,
        "failed native spawn does not create a child"
    );
    let repeated = fixture.live.request(request);
    assert!(
        matches!(
            repeated,
            Response::Error {
                code: ErrorCode::AlreadyExists,
                ..
            }
        ),
        "{repeated:?}"
    );
    assert_eq!(
        cli_json(
            &fixture.live,
            &[
                "workspace",
                "get",
                "--project",
                PROJECT,
                "--branch",
                "combined-no-process"
            ]
        ),
        created
    );
    assert_eq!(ovrcr::git::worktree_identity(path).unwrap(), generation);
    assert_eq!(
        cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT])
            .as_array()
            .unwrap()
            .len(),
        before.as_array().unwrap().len() + 1
    );
    assert_eq!(
        fs::read(fixture.live.repo.join("README")).unwrap(),
        root_bytes
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_coding_partial_workspace_failures_retry_one_exact_resource_with_new_attempt() {
    for (point, stage) in [
        (
            "coding-before-registration",
            WorkspaceCreation::CreatedUnregistered,
        ),
        ("coding-after-registration", WorkspaceCreation::Registered),
    ] {
        let fixture = super::followups::OwnedFollowup::new();
        let inventory = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]);
        fixture.proxy.arm(ControlRule {
            operation: "Launch",
            loss: 2,
            fault: Some((point, 0)),
        });
        let failed = start(&fixture, "partial-coding");
        assert_eq!(
            failed.dispatch,
            DispatchStatus::Refused,
            "{point}: {failed:?}"
        );
        assert_eq!(failed.execution_attempts, Some(0));
        let resource = failed.resources.coding.as_ref().unwrap().clone();
        assert_eq!(resource.creation, stage);
        assert!(resource.no_process_started);
        assert!(resource.path.is_dir());
        assert_eq!(
            ovrcr::git::worktree_identity(&resource.path).unwrap(),
            resource.git_identity.clone().unwrap()
        );
        assert_eq!(fixture.child_count(), 0);
        let receipts = failed.receipts.clone();
        let reason = failed.reason.clone();
        assert!(reason.as_ref().unwrap().contains("owned failure"));
        let contract = fs::read(failed.brief.parent().unwrap().join("contract-v1.json")).unwrap();
        let repeated: AssignmentSnapshot =
            serde_json::from_value(fixture.recover(&failed.handle)).unwrap();
        assert_eq!(repeated.receipts, receipts);
        assert_eq!(repeated.resources.coding, Some(resource.clone()));
        assert_eq!(
            fixture.child_count(),
            0,
            "plain start cannot mutate final rejected launch"
        );
        let successor: AssignmentSnapshot = serde_json::from_value(
            fixture.tool("ovrcr_retry", serde_json::json!({"handle":failed.handle})),
        )
        .unwrap();
        assert_ne!(successor.handle, failed.handle);
        let successor_contract: ResearchContract = serde_json::from_slice(
            &fs::read(successor.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            successor_contract.previous_request,
            Some(failed.handle.clone())
        );
        assert_eq!(
            fixture.reconcile(&failed.handle).successor_request,
            Some(successor.handle.clone())
        );
        assert_eq!(successor.identity.workflow, failed.identity.workflow);
        assert_eq!(successor.identity.task, failed.identity.task);
        assert_eq!(successor.identity.attempt, failed.identity.attempt + 1);
        let created = successor.resources.coding.as_ref().unwrap();
        assert_eq!(created.path, resource.path);
        assert_eq!(created.base, resource.base);
        assert_eq!(created.git_identity, resource.git_identity);
        assert_eq!(created.creation, WorkspaceCreation::Registered);
        let successor = complete(&fixture, &successor);
        let again: AssignmentSnapshot = serde_json::from_value(
            fixture.tool("ovrcr_retry", serde_json::json!({"handle":failed.handle})),
        )
        .unwrap();
        assert_eq!(again.identity, successor.identity);
        assert_eq!(fixture.child_count(), 1);
        assert_eq!(fixture.input_count(&successor), 1);
        let original = fixture.reconcile(&failed.handle);
        assert_eq!(original.receipts, receipts);
        assert_eq!(original.reason, reason);
        assert_eq!(
            fs::read(failed.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
            contract
        );
        assert_eq!(
            cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT])
                .as_array()
                .unwrap()
                .len(),
            inventory.as_array().unwrap().len() + 1
        );
        let denied = fixture.tool(
            "ovrcr_retry",
            serde_json::json!({"handle":successor.handle}),
        );
        assert_eq!(
            denied["dispatch"], "Refused",
            "a started implementation never qualifies for no-process retry"
        );
    }
}

#[test]
fn public_coding_pins_display_ref_once_before_launch_effects() {
    let fixture = super::followups::OwnedFollowup::new();
    let original = live::git(&fixture.live.repo, &["rev-parse", "main"])
        .trim()
        .to_owned();
    fixture.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 0,
        fault: None,
    });
    let unresolved = fixture.tool("ovrcr_start", arguments("pinned-coding"));
    assert_eq!(unresolved["dispatch"], "Unknown");
    let handle = unresolved["handle"].as_str().unwrap();
    let prepared = fixture.reconcile(handle);
    assert_eq!(prepared.resources.coding.as_ref().unwrap().base, original);
    let moved = live::git(
        &fixture.live.repo,
        &[
            "commit-tree",
            "HEAD^{tree}",
            "-p",
            &original,
            "-m",
            "move display ref",
        ],
    )
    .trim()
    .to_owned();
    live::git(
        &fixture.live.repo,
        &["update-ref", "refs/heads/main", &moved],
    );
    assert_ne!(original, moved);
    let launched: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
    let resource = launched.resources.coding.as_ref().unwrap();
    assert_eq!(resource.base, original);
    assert_eq!(
        live::git(&resource.path, &["rev-parse", "HEAD"]).trim(),
        original
    );
    assert_eq!(
        live::git(&fixture.live.repo, &["rev-parse", "main"]).trim(),
        moved
    );
    let completed = complete(&fixture, &launched);
    assert_eq!(
        completed
            .submission
            .as_ref()
            .unwrap()
            .coding
            .as_ref()
            .unwrap()
            .candidate
            .as_ref()
            .unwrap()
            .base,
        original
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_coding_authenticated_intent_recovers_without_recapture_and_refuses_storage_gaps() {
    for damage in ["none", "missing", "corrupt", "valid-replacement"] {
        let fixture = super::followups::OwnedFollowup::new();
        fixture.proxy.arm(ControlRule {
            operation: "Publish",
            loss: 2,
            fault: Some(("coding-after-candidate", 0)),
        });
        let original = start(&fixture, "intent-coding");
        // Keep automatic Observe recovery from publishing before storage damage is staged.
        fs::create_dir(&original.result_path).unwrap();
        fs::write(fixture.live.root.path().join("allow-result"), "1").unwrap();
        wait_file(&original.brief.with_extension("json.published"));
        let hit = fs::read_to_string(fixture.live.root.path().join("fault.hit")).unwrap();
        assert_eq!(hit, "coding-after-candidate");
        let path = original.brief.parent().unwrap().join("candidate-v1.json");
        let bytes = fs::read(&path).unwrap();
        assert!(
            original.result_path.is_dir(),
            "test-owned gate prevents result publication at the forced boundary"
        );
        let workspace = original.resources.coding.as_ref().unwrap();
        let captured = fs::read(workspace.path.join("change.txt")).unwrap();
        fs::write(
            workspace.path.join("change.txt"),
            "mutable checkout after authenticated intent",
        )
        .unwrap();
        match damage {
            "missing" => fs::remove_file(&path).unwrap(),
            "corrupt" => fs::write(&path, b"{partial").unwrap(),
            "valid-replacement" => {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                value["snapshot"]["head"] = "2".repeat(40).into();
                fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            _ => {}
        }
        fs::remove_dir(&original.result_path).unwrap();
        if damage != "none" {
            let refused = tool_reply(
                &fixture.live,
                fixture.parent,
                "ovrcr_collect",
                serde_json::json!({"handle":original.handle}),
                &fixture.live.root.path().join("damaged-intent-collect.json"),
            );
            assert!(refused["error"].is_string(), "{damage}: {refused}");
            assert!(
                !original.result_path.exists(),
                "a damaged candidate cannot reconstruct authenticated result"
            );
            let db =
                rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
            let raw: String = db
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle=?1",
                    [&original.handle],
                    |row| row.get(0),
                )
                .unwrap();
            let stored: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert!(
                stored["result_digest"].is_null() && stored["acceptance"].is_null(),
                "damaged storage cannot become a committed or accepted submission: {stored}"
            );
            let restore = path.with_extension("restore");
            fs::write(&restore, &bytes).unwrap();
            fs::set_permissions(&restore, fs::Permissions::from_mode(0o600)).unwrap();
            fs::rename(&restore, &path).unwrap();
        }
        let recovered: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_collect",
            serde_json::json!({"handle":original.handle}),
        ))
        .unwrap();
        assert_eq!(recovered.identity, original.identity);
        assert_eq!(recovered.receipts, original.receipts);
        assert_eq!(recovered.resources.coding, original.resources.coding);
        assert_eq!(fixture.child_count(), 1);
        assert_eq!(fixture.input_count(&recovered), 1);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let publication: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let snapshot: ovrcr::git::candidate::CandidateSnapshot =
            serde_json::from_value(publication["snapshot"].clone()).unwrap();
        assert_eq!(
            snapshot
                .files
                .iter()
                .find(|file| file.path == "change.txt")
                .unwrap()
                .bytes()
                .unwrap()
                .unwrap(),
            captured
        );
        assert_ne!(
            fs::read(workspace.path.join("change.txt")).unwrap(),
            captured
        );
        assert!(original.result_path.exists());
        assert!(recovered.submission.is_some());
    }
}

#[test]
fn public_coding_replaced_generation_refuses_capture_and_valid_original_can_publish() {
    for repository in [false, true] {
        let fixture = super::followups::OwnedFollowup::new();
        let original = start(&fixture, "generation-coding");
        let workspace = original.resources.coding.as_ref().unwrap();
        let owner = if repository {
            &fixture.live.repo
        } else {
            &workspace.path
        };
        let admin = live::git(owner, &["rev-parse", "--absolute-git-dir"]);
        let marker = Path::new(admin.trim()).join("ovrcr-identity");
        let generation = fs::read(&marker).unwrap();
        fs::write(&marker, "replacement generation").unwrap();
        fs::write(fixture.live.root.path().join("allow-result"), "1").unwrap();
        wait_file(&original.brief.with_extension("json.published"));
        let reply: serde_json::Value = serde_json::from_slice(
            &fs::read(original.brief.with_extension("json.published")).unwrap(),
        )
        .unwrap();
        fs::write(&marker, &generation).unwrap();
        assert!(reply["error"].is_string(), "{reply}");
        assert!(!original.result_path.exists());
        assert!(
            !original
                .brief
                .parent()
                .unwrap()
                .join("candidate-v1.json")
                .exists()
        );
        let request = fixture
            .proxy
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request, DelegationRequest::Publish { .. }))
            .unwrap()
            .clone();
        assert!(matches!(
            fixture.live.request(Request::Delegation(Box::new(request))),
            Response::Delegation(_)
        ));
        let collected: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_collect",
            serde_json::json!({"handle":original.handle}),
        ))
        .unwrap();
        assert_eq!(collected.identity, original.identity);
        assert_eq!(collected.resources.coding, original.resources.coding);
        assert_eq!(fixture.child_count(), 1);
        assert_eq!(fixture.input_count(&collected), 1);
    }
}

struct GitGate {
    entered: std::path::PathBuf,
    release: std::path::PathBuf,
    shim: std::path::PathBuf,
}
impl GitGate {
    fn install(
        fixture: &RecoveryFixture,
        cwd: &Path,
        command: &str,
        occurrence: usize,
        after: bool,
    ) -> Self {
        let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|path| path.join("git"))
            .find(|path| path.is_file())
            .unwrap();
        let root = fixture.live.root.path();
        let gate = Self {
            entered: root.join("git-entered"),
            release: root.join("git-release"),
            shim: fixture.live.claude.command.parent().unwrap().join("git"),
        };
        let count = root.join("git-count");
        let run_first = if after {
            format!("'{}' \"$@\" || exit $?\n", git.display())
        } else {
            String::new()
        };
        let finish = if after {
            "exit 0\n".to_owned()
        } else {
            format!("exec '{}' \"$@\"\n", git.display())
        };
        fs::write(&gate.shim, format!("#!/bin/sh\nif [ \"$PWD\" = '{}' ]; then\ncase \"$*\" in *\"{}\"*)\nn=0; [ ! -f '{}' ] || n=$(cat '{}'); n=$((n+1)); printf %s \"$n\" > '{}'\nif [ \"$n\" = '{}' ]; then\n{}: > '{}'; while [ ! -f '{}' ]; do sleep 0.01; done\n{}fi\n;; esac\nfi\nexec '{}' \"$@\"\n",
            cwd.display(), command, count.display(), count.display(), count.display(), occurrence,
            run_first, gate.entered.display(), gate.release.display(), finish, git.display())).unwrap();
        fs::set_permissions(&gate.shim, fs::Permissions::from_mode(0o700)).unwrap();
        gate
    }
    fn release(&self) {
        fs::write(&self.release, "release").unwrap();
    }
}
impl Drop for GitGate {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, "release");
    }
}

fn published_request(fixture: &RecoveryFixture) -> DelegationRequest {
    fixture
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|request| matches!(request, DelegationRequest::Publish { .. }))
        .unwrap()
        .clone()
}
fn assert_no_publication(fixture: &RecoveryFixture, assignment: &AssignmentSnapshot) {
    assert!(!assignment.result_path.exists());
    assert!(
        !assignment
            .brief
            .parent()
            .unwrap()
            .join("candidate-v1.json")
            .exists()
    );
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&assignment.handle],
            |row| row.get(0),
        )
        .unwrap();
    let record: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(record["publication_intent"].is_null(), "{record}");
    assert!(record["result_digest"].is_null(), "{record}");
}
fn replay_valid_publication(fixture: &RecoveryFixture, assignment: &AssignmentSnapshot) {
    let request = published_request(fixture);
    assert!(matches!(
        fixture.live.request(Request::Delegation(Box::new(request))),
        Response::Delegation(_)
    ));
    let collected: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":assignment.handle}),
    ))
    .unwrap();
    assert_eq!(collected.identity, assignment.identity);
    assert_eq!(collected.result_version, 1);
    assert_eq!(collected.resources.coding, assignment.resources.coding);
    assert!(
        collected
            .submission
            .as_ref()
            .unwrap()
            .coding
            .as_ref()
            .unwrap()
            .candidate
            .is_some()
    );
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(fixture.input_count(&collected), 1);
}

#[test]
fn public_coding_capture_concurrent_edit_refuses_then_same_version_can_publish() {
    let fixture = super::followups::OwnedFollowup::new();
    let original = start(&fixture, "capture-race");
    let path = &original.resources.coding.as_ref().unwrap().path;
    let gate = GitGate::install(&fixture, path, "ls-files -v -z", 2, false);
    fs::write(fixture.live.root.path().join("allow-result"), "1").unwrap();
    wait_file(&gate.entered);
    assert_eq!(
        fs::read_to_string(fixture.live.root.path().join("git-count")).unwrap(),
        "2"
    );
    println!("CODING_CAPTURE_CONCURRENCY complete_scans=1 second_scan_entered=true");
    let captured = fs::read(path.join("change.txt")).unwrap();
    fs::write(
        path.join("change.txt"),
        "concurrent edit between complete scans",
    )
    .unwrap();
    assert_no_publication(&fixture, &original);
    gate.release();
    wait_file(&original.brief.with_extension("json.published"));
    let reply: serde_json::Value =
        serde_json::from_slice(&fs::read(original.brief.with_extension("json.published")).unwrap())
            .unwrap();
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("changed during capture"),
        "{reply}"
    );
    assert_no_publication(&fixture, &original);
    fs::write(path.join("change.txt"), captured).unwrap();
    replay_valid_publication(&fixture, &original);
}

#[test]
fn public_coding_final_capture_control_fence_refuses_then_same_version_can_publish() {
    let fixture = super::followups::OwnedFollowup::new();
    let original = start(&fixture, "capture-control");
    let path = &original.resources.coding.as_ref().unwrap().path;
    // Third inspection is the final Git check after both complete content scans.
    let gate = GitGate::install(
        &fixture,
        path,
        "status --porcelain=v1 --untracked-files=all",
        3,
        true,
    );
    let mut dashboard = super::assignment_control::dashboard(&fixture, &original);
    let taken = super::assignment_control::transition(
        &fixture,
        &mut dashboard,
        original.identity.session.unwrap(),
        "capture-take",
        true,
        1,
    );
    let input_bytes = fixture.byte_count(&original);
    fs::write(fixture.live.root.path().join("allow-result"), "1").unwrap();
    wait_file(&gate.entered);
    assert_eq!(
        fs::read_to_string(fixture.live.root.path().join("git-count")).unwrap(),
        "3"
    );
    println!(
        "CODING_CAPTURE_FINAL complete_scans=2 baseline_epoch={} baseline_human_revision={}",
        taken.control.epoch, taken.control.human_input_revision
    );
    assert_no_publication(&fixture, &original);
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            60,
            Request::AssignedHumanInput {
                session: original.identity.session.unwrap(),
                run: original.identity.run.unwrap(),
                control_epoch: taken.control.epoch,
                view_revision: 1,
                bytes: b" ".to_vec(),
            }
        )
        .unwrap(),
        Response::Ok
    );
    gate.release();
    wait_file(&original.brief.with_extension("json.published"));
    let reply: serde_json::Value =
        serde_json::from_slice(&fs::read(original.brief.with_extension("json.published")).unwrap())
            .unwrap();
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("authority or version changed during candidate capture"),
        "{reply}"
    );
    assert_no_publication(&fixture, &original);
    let changed = fixture.reconcile(&original.handle);
    assert_eq!(
        changed.control.human_input_revision,
        taken.control.human_input_revision + 1
    );
    println!(
        "CODING_CAPTURE_REJECTED final_epoch={} final_human_revision={} candidate=false result=false intent=false digest=false",
        changed.control.epoch, changed.control.human_input_revision
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.byte_count(&original) != input_bytes + 1 {
        assert!(
            Instant::now() < deadline,
            "human input byte must reach the actual child"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let returned = super::assignment_control::transition(
        &fixture,
        &mut dashboard,
        original.identity.session.unwrap(),
        "capture-return",
        false,
        1,
    );
    let confirmed = super::assignment_control::confirm(
        &fixture,
        &returned,
        "capture-return",
        "capture-confirm",
    );
    assert!(confirmed.control.barrier.is_none());
    replay_valid_publication(&fixture, &original);
}

#[test]
fn public_coding_prelaunch_repository_or_root_substitution_refuses_without_effects() {
    for repository in [true, false] {
        let fixture = super::followups::OwnedFollowup::new();
        fixture.proxy.arm(ControlRule {
            operation: "Launch",
            loss: 0,
            fault: None,
        });
        let reply = fixture.tool("ovrcr_start", arguments("substitution-coding"));
        assert_eq!(reply["dispatch"], "Unknown");
        let handle = reply["handle"].as_str().unwrap();
        let prepared = fixture.reconcile(handle);
        let source = if repository {
            fixture.live.repo.clone()
        } else {
            fixture.live.workspace_root.clone()
        };
        let held = source.with_extension("held");
        fs::rename(&source, &held).unwrap();
        std::os::unix::fs::symlink(&held, &source).unwrap();
        struct Restore(std::path::PathBuf, std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
                let _ = fs::rename(&self.1, &self.0);
            }
        }
        let restore = Restore(source, held);
        let failed: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
        assert_eq!(failed.dispatch, DispatchStatus::Refused, "{failed:?}");
        assert!(
            failed
                .reason
                .as_ref()
                .unwrap()
                .contains("repository or workspace root changed")
        );
        assert!(failed.identity.session.is_none());
        assert!(!prepared.resources.coding.as_ref().unwrap().path.exists());
        assert_eq!(fixture.child_count(), 0);
        drop(restore);
        assert_eq!(
            live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"])
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count(),
            2
        );
    }
}

#[test]
fn public_coding_git_effect_with_error_retains_uncertain_resource_without_adoption() {
    let fixture = super::followups::OwnedFollowup::new();
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap();
    let shim = fixture.live.claude.command.parent().unwrap().join("git");
    fs::write(&shim, format!("#!/bin/sh\ncase \"$*\" in *\"worktree add\"*\"uncertain-git\"*) '{}' \"$@\" || exit $?; printf 'owned failure after actual Git effect\\n' >&2; exit 73;; esac\nexec '{}' \"$@\"\n", git.display(),git.display())).unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    let failed = start(&fixture, "uncertain-git");
    assert_eq!(failed.dispatch, DispatchStatus::Refused);
    let resource = failed.resources.coding.as_ref().unwrap();
    assert_eq!(resource.creation, WorkspaceCreation::Unresolved);
    assert!(resource.path.is_dir());
    assert!(resource.git_identity.is_none());
    assert!(failed.identity.session.is_none());
    let before = live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"]);
    assert_eq!(
        before
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        3
    );
    let repeated: AssignmentSnapshot =
        serde_json::from_value(fixture.recover(&failed.handle)).unwrap();
    assert_eq!(repeated.receipts, failed.receipts);
    assert_eq!(repeated.resources.coding, failed.resources.coding);
    let retry = fixture.tool("ovrcr_retry", serde_json::json!({"handle":failed.handle}));
    assert_eq!(retry["dispatch"], "Refused", "{retry}");
    assert_eq!(
        live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"]),
        before
    );
    assert_eq!(fixture.child_count(), 0);
    assert_eq!(
        cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT])
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn public_coding_actual_registry_failure_retains_generation_and_retry_reuses_it() {
    let fixture = super::followups::OwnedFollowup::new();
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER owned_registration_failure BEFORE INSERT ON workspaces WHEN NEW.id = 'registry-failure' BEGIN SELECT RAISE(FAIL, 'owned registration failure'); END;").unwrap();
    let failed = start(&fixture, "registry-failure");
    assert_eq!(failed.dispatch, DispatchStatus::Refused);
    assert!(
        failed
            .reason
            .as_ref()
            .unwrap()
            .contains("registry write failed")
    );
    let resource = failed.resources.coding.as_ref().unwrap().clone();
    assert_eq!(resource.creation, WorkspaceCreation::CreatedUnregistered);
    assert!(resource.no_process_started);
    assert!(resource.path.is_dir());
    assert_eq!(
        ovrcr::git::worktree_identity(&resource.path).unwrap(),
        resource.git_identity.clone().unwrap()
    );
    assert_eq!(fixture.child_count(), 0);
    assert_eq!(
        cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT])
            .as_array()
            .unwrap()
            .len(),
        2
    );
    db.execute_batch("DROP TRIGGER owned_registration_failure;")
        .unwrap();
    let successor: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_retry", serde_json::json!({"handle":failed.handle})),
    )
    .unwrap();
    let after = successor.resources.coding.as_ref().unwrap();
    assert_eq!(after.path, resource.path);
    assert_eq!(after.base, resource.base);
    assert_eq!(after.git_identity, resource.git_identity);
    assert_eq!(after.creation, WorkspaceCreation::Registered);
    assert_eq!(successor.identity.attempt, failed.identity.attempt + 1);
    let completed = complete(&fixture, &successor);
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(fixture.input_count(&completed), 1);
    assert_eq!(
        live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        3
    );
    assert_eq!(fixture.reconcile(&failed.handle).receipts, failed.receipts);
}

#[test]
fn public_coding_real_native_spawn_refusal_retains_registered_generation_for_one_retry() {
    let fixture = super::followups::OwnedFollowup::with_owned_executable();
    let executable = &fixture.live.executable;
    assert_eq!(executable.parent(), Some(fixture.live.root.path()));
    assert_eq!(executable.file_name().unwrap(), "owned-ovrcr");
    assert_ne!(executable, Path::new(env!("CARGO_BIN_EXE_ovrcr")));
    let original_mode = fs::metadata(executable).unwrap().permissions().mode();
    struct RestoreExecutable(std::path::PathBuf, u32);
    impl Drop for RestoreExecutable {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1)).unwrap();
        }
    }
    let restore = RestoreExecutable(executable.clone(), original_mode);
    fs::set_permissions(executable, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(fs::metadata(executable).unwrap().mode() & 0o111, 0);
    let failed = start(&fixture, "native-spawn-refusal");
    // Restore only the owned copy before any assertion or explicit retry can fail.
    drop(restore);
    assert_eq!(failed.dispatch, DispatchStatus::Refused, "{failed:?}");
    assert!(
        failed
            .reason
            .as_ref()
            .unwrap()
            .contains("Launch failed before a process started")
    );
    assert_eq!(failed.execution_attempts, Some(0));
    let resource = failed.resources.coding.as_ref().unwrap().clone();
    assert_eq!(resource.creation, WorkspaceCreation::Registered);
    assert!(resource.no_process_started);
    assert_eq!(
        ovrcr::git::worktree_identity(&resource.path).unwrap(),
        resource.git_identity.clone().unwrap()
    );
    assert_eq!(fixture.child_count(), 0);
    assert!(
        !failed
            .receipts
            .iter()
            .any(|receipt| receipt.operation == DispatchOperation::InitialInput)
    );
    let successor: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_retry", serde_json::json!({"handle":failed.handle})),
    )
    .unwrap();
    let retried = successor.resources.coding.as_ref().unwrap();
    assert_eq!(retried.path, resource.path);
    assert_eq!(retried.base, resource.base);
    assert_eq!(retried.git_identity, resource.git_identity);
    assert_eq!(successor.identity.attempt, failed.identity.attempt + 1);
    let completed = complete(&fixture, &successor);
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(fixture.input_count(&completed), 1);
    let again: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_retry", serde_json::json!({"handle":failed.handle})),
    )
    .unwrap();
    assert_eq!(again.identity, completed.identity);
    assert_eq!(fixture.reconcile(&failed.handle).receipts, failed.receipts);
    assert_eq!(
        live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        3
    );
    assert_eq!(
        fs::metadata(executable).unwrap().mode() & 0o777,
        original_mode & 0o777
    );
}

struct DirectoryReplacement {
    original: std::path::PathBuf,
    held: std::path::PathBuf,
    replacement: std::path::PathBuf,
}
impl DirectoryReplacement {
    fn install(original: &Path, replacement: &Path) -> Self {
        let held = original.with_extension("original-held");
        assert!(!held.exists());
        fs::rename(original, &held).unwrap();
        fs::rename(replacement, original).unwrap();
        Self {
            original: original.into(),
            held,
            replacement: replacement.into(),
        }
    }
}
impl Drop for DirectoryReplacement {
    fn drop(&mut self) {
        fs::rename(&self.original, &self.replacement).unwrap();
        fs::rename(&self.held, &self.original).unwrap();
    }
}

#[test]
fn public_coding_same_path_repository_replacement_refuses_before_creation() {
    let fixture = super::followups::OwnedFollowup::new();
    fixture.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 0,
        fault: None,
    });
    let pending = fixture.tool("ovrcr_start", arguments("same-path-repo"));
    assert_eq!(pending["dispatch"], "Unknown");
    let handle = pending["handle"].as_str().unwrap();
    let prepared = fixture.reconcile(handle);
    let original_generation = ovrcr::git::worktree_identity(&fixture.live.repo).unwrap();
    let replacement = fixture.live.root.path().join("replacement-repository");
    live::git(
        &fixture.live.repo,
        &[
            "clone",
            "--no-hardlinks",
            "--no-local",
            ".",
            replacement.to_str().unwrap(),
        ],
    );
    let replacement_generation = ovrcr::git::capture_worktree_identity(&replacement).unwrap();
    assert_ne!(replacement_generation, original_generation);
    assert_eq!(
        live::git(&replacement, &["rev-parse", "HEAD"]).trim(),
        prepared.resources.coding.as_ref().unwrap().base
    );
    let inventory = cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]);
    let git_before = live::git(&replacement, &["worktree", "list", "--porcelain"]);
    let swap = DirectoryReplacement::install(&fixture.live.repo, &replacement);
    assert_eq!(
        fs::canonicalize(&fixture.live.repo).unwrap(),
        prepared.resources.coding.as_ref().unwrap().repository
    );
    let reply = fixture.recover(handle);
    let git_after = live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"]);
    let created = prepared.resources.coding.as_ref().unwrap().path.exists();
    drop(swap);
    let result: AssignmentSnapshot = serde_json::from_value(reply).unwrap();
    assert_eq!(
        result.dispatch,
        DispatchStatus::Refused,
        "a different repository generation at the same path must not create an assignment: {result:?}"
    );
    assert!(result.identity.session.is_none());
    assert!(
        !created,
        "replacement repository must receive no coding worktree"
    );
    // The root line names its current location; any second worktree is an effect.
    assert_eq!(
        git_after
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        git_before
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count()
    );
    assert_eq!(fixture.child_count(), 0);
    assert_eq!(
        cli_json(&fixture.live, &["workspace", "list", "--project", PROJECT]),
        inventory
    );
    assert_eq!(
        ovrcr::git::worktree_identity(&fixture.live.repo).unwrap(),
        original_generation
    );
    let swap = DirectoryReplacement::install(&fixture.live.repo, &replacement);
    let refused_prepare = fixture.tool("ovrcr_start", arguments("same-path-repo-prepare"));
    let effects = fixture
        .live
        .workspace_root
        .join("same-path-repo-prepare")
        .exists();
    drop(swap);
    assert_eq!(refused_prepare["dispatch"], "Refused", "{refused_prepare}");
    assert!(
        refused_prepare["reason"]
            .as_str()
            .unwrap()
            .contains("registered repository root Git identity changed")
    );
    let handle = refused_prepare["handle"].as_str().unwrap();
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let retained: i64 = db
        .query_row(
            "SELECT count(*) FROM delegations WHERE handle=?1",
            [handle],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        retained, 0,
        "repository identity is checked before a Prepare intent is stored"
    );
    assert!(!effects);
    assert_eq!(fixture.child_count(), 0);
}

#[test]
fn public_coding_replaced_parent_directory_preserves_containment_and_unrelated_work() {
    let fixture = super::followups::OwnedFollowup::new();
    fixture.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 0,
        fault: None,
    });
    let pending = fixture.tool("ovrcr_start", arguments("same-path-root"));
    assert_eq!(pending["dispatch"], "Unknown");
    let handle = pending["handle"].as_str().unwrap();
    let prepared = fixture.reconcile(handle);
    let replacement = fixture.live.root.path().join("replacement-workspace-root");
    fs::create_dir(&replacement).unwrap();
    let root_inode = fs::metadata(&fixture.live.workspace_root).unwrap().ino();
    assert_ne!(fs::metadata(&replacement).unwrap().ino(), root_inode);
    let parent_relative = Path::new(&fixture.workspace);
    let parent_bytes = fs::read(
        fixture
            .live
            .workspace_root
            .join(parent_relative)
            .join("README"),
    )
    .unwrap();
    let parent_generation =
        ovrcr::git::worktree_identity(&fixture.live.workspace_root.join(parent_relative)).unwrap();
    let repository_bytes = fs::read(fixture.live.repo.join("README")).unwrap();
    let repository_head = live::git(&fixture.live.repo, &["rev-parse", "HEAD"]);
    let git_before = live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"]);
    let swap = DirectoryReplacement::install(&fixture.live.workspace_root, &replacement);
    assert_eq!(
        fs::canonicalize(&fixture.live.workspace_root)
            .unwrap()
            .join("same-path-root"),
        prepared.resources.coding.as_ref().unwrap().path
    );
    let result: AssignmentSnapshot = serde_json::from_value(fixture.recover(handle)).unwrap();
    let resource = result.resources.coding.as_ref().unwrap();
    let applied = result.dispatch == DispatchStatus::Applied;
    let exists = resource.path.exists();
    let containment = !exists
        || fs::canonicalize(&resource.path).unwrap().parent()
            == Some(
                fs::canonicalize(&fixture.live.workspace_root)
                    .unwrap()
                    .as_path(),
            );
    let generation = exists.then(|| ovrcr::git::worktree_identity(&resource.path).unwrap());
    let original_parent_bytes = fs::read(swap.held.join(parent_relative).join("README")).unwrap();
    let original_parent_generation =
        ovrcr::git::worktree_identity(&swap.held.join(parent_relative)).unwrap();
    let git_after = live::git(&fixture.live.repo, &["worktree", "list", "--porcelain"]);
    let children = fixture.child_count();
    drop(swap);
    assert!(
        containment,
        "replacing the parent directory must not let creation escape its canonical root"
    );
    assert_eq!(original_parent_bytes, parent_bytes);
    assert_eq!(original_parent_generation, parent_generation);
    assert_eq!(
        fs::read(fixture.live.repo.join("README")).unwrap(),
        repository_bytes
    );
    assert_eq!(
        live::git(&fixture.live.repo, &["rev-parse", "HEAD"]),
        repository_head
    );
    if applied {
        assert_eq!(resource.creation, WorkspaceCreation::Registered);
        assert_eq!(generation, resource.git_identity);
        assert!(result.identity.session.is_some());
        assert_eq!(children, 1);
        assert_eq!(fixture.input_count(&result), 1);
        assert_eq!(
            git_after
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count(),
            git_before
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count()
                + 1
        );
    } else {
        assert_eq!(result.dispatch, DispatchStatus::Refused, "{result:?}");
        assert!(!exists);
        assert_eq!(children, 0);
        assert_eq!(git_after, git_before);
    }
    println!(
        "CODING_PARENT_DIRECTORY_REPLACEMENT applied={applied} containment=true unrelated_work_unchanged=true"
    );
    assert_eq!(
        fs::metadata(&fixture.live.workspace_root).unwrap().ino(),
        root_inode
    );
}
