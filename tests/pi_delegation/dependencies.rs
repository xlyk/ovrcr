//! Parent tools -> production client/binary Server -> managed PTYs/Git/private files.
use super::*;
use serde_json::{Value, json};

fn task(name: &str, integrate: bool) -> Value {
    json!({"task":name,"canonical_task":format!("https://example.test/tasks/{name}"),"request":format!("request-{name}"),"project":PROJECT,"workspace":format!("dependency-{name}"),"revision":1,"integrate_snapshots":integrate})
}
fn edge(from: &str, to: &str, artifact: &str) -> Value {
    json!({"predecessor":from,"successor":to,"output":format!("{from}-output"),"artifacts":[artifact],"required_reviews":[],"selected":null})
}
fn configure(f: &RecoveryFixture, version: u64, graph: &Value) -> WorkflowSnapshot {
    serde_json::from_value(f.tool(
        "ovrcr_dependencies",
        json!({"expected_version":version,"graph":graph}),
    ))
    .unwrap()
}
fn raw(f: &RecoveryFixture, name: &str, args: Value) -> Value {
    let id = f.files.get();
    f.files.set(id + 1);
    tool_reply(
        &f.live,
        f.parent,
        name,
        args,
        &f.live
            .root
            .path()
            .join(format!("dependency-reply-{id}.json")),
    )
}
fn arguments(name: &str, config: Value, paths: &[&str], base: &str) -> Value {
    json!({"task":name,"request":format!("request-{name}"),"project":PROJECT,"workspace":format!("dependency-{name}"),"scope":format!("DEPENDENCY_FIXTURE\n{config}"),"model_provider":"test","model":"fixture-model","acceptance_evidence":["actual inherited bytes and dependency-check"],"coding":{"requested_base":base,"branch":format!("dependency/{name}"),"authorized_paths":paths,"required_checks":["dependency-check"]}})
}
fn start(f: &RecoveryFixture, name: &str, config: Value, paths: &[&str]) -> AssignmentSnapshot {
    serde_json::from_value(f.tool("ovrcr_start", arguments(name, config, paths, "main"))).unwrap()
}
fn collect(f: &RecoveryFixture, started: &AssignmentSnapshot) -> AssignmentSnapshot {
    wait_file(&started.brief.with_extension("json.published"));
    let published: Value =
        serde_json::from_slice(&fs::read(started.brief.with_extension("json.published")).unwrap())
            .unwrap();
    assert!(published.get("error").is_none(), "{published}");
    serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":started.handle}))).unwrap()
}
fn accept(f: &RecoveryFixture, source: &AssignmentSnapshot) -> AssignmentSnapshot {
    let publication = source.publication.as_ref().unwrap();
    serde_json::from_value(f.tool("ovrcr_accept",json!({"handle":source.handle,"identity":source.identity,"result_version":source.result_version,"result_digest":publication.identity.digest,"expected_control_epoch":source.control.epoch}))).unwrap()
}
fn selection(source: &AssignmentSnapshot) -> Value {
    json!({"target":source.coding_target.as_ref().unwrap(),"acceptance":source.acceptance_ref.as_ref().unwrap(),"reviews":[]})
}
fn release(f: &RecoveryFixture) {
    fs::write(f.live.root.path().join("allow-result"), "1").unwrap();
}

#[test]
fn public_dependencies_readable_review_keeps_complete_authenticated_inherited_inputs() {
    use sha2::{Digest, Sha256};
    let f = super::followups::OwnedFollowup::new();
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    release(&f);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"complete inherited A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    let a = accept(&f, &collect(&f, &a));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    let b = start(
        &f,
        "b",
        json!({"expect":[{"path":"a.txt","content":"complete inherited A"}],"outputs":[{"path":"b.txt","content":"own B"}],"check_path":"b.txt"}),
        &["b.txt"],
    );
    let b = collect(&f, &b);
    let content = "Preserve inherited A and add own B.";
    let source = ReviewSource {
        source: ReviewSourceReference {
            kind: ReviewSourceKind::Spec,
            reference: "fixture://inherited-task".into(),
            digest: format!("{:x}", Sha256::digest(content.as_bytes())),
        },
        content: content.into(),
    };
    let declared: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_review",json!({"handle":b.handle,"operation":{"Declare":{
        "request":"inherited-readable-review","target":b.coding_target,"requirements":[{"axis":"Spec","sources":[source.source]}],"sources":[source],"repair_limit":null
    }}}))).unwrap();
    let selected: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_review",
        json!({"handle":declared.handle,"operation":{"Assignment":{"axis":"Spec"}}}),
    ))
    .unwrap();
    let assignment = selected.review_assignment.unwrap();
    let view = assignment.target.view.unwrap();
    let bytes = fs::read(&view.path).unwrap();
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    let inherited = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["kind"] == "inherited" && file["path"] == "a.txt")
        .unwrap();
    assert_eq!(
        fs::read_to_string(inherited["after"]["path"].as_str().unwrap()).unwrap(),
        "complete inherited A"
    );
    assert!(assignment.target.coding.candidate.input_baseline.is_some());
    fs::write(
        b.resources.coding.as_ref().unwrap().path.join("a.txt"),
        "later checkout bytes",
    )
    .unwrap();
    let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_review",
        json!({"handle":declared.handle,"operation":{"Assignment":{"axis":"Spec"}}}),
    ))
    .unwrap();
    assert_eq!(replay.review_assignment.unwrap().target.view, Some(view));
    assert_eq!(
        fs::read(
            manifest["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|file| file["kind"] == "inherited")
                .unwrap()["after"]["path"]
                .as_str()
                .unwrap()
        )
        .unwrap(),
        b"complete inherited A"
    );
    assert_eq!(
        f.child_count(),
        2,
        "view preparation launches no reviewer or extra implementation"
    );
}

#[test]
fn public_dependencies_gate_inputs_preserve_complete_untracked_lineage_and_invalidate_only_descendants()
 {
    let f = super::followups::OwnedFollowup::new();
    let mut graph = json!({"tasks":[task("a",false),task("b",true),task("c",true),task("x",false)],"edges":[edge("a","b","a.txt"),edge("b","c","b.txt")]});
    let initial = configure(&f, 0, &graph);
    assert_eq!(initial.dependencies.as_ref().unwrap().version, 1);
    let blocked = f.tool(
        "ovrcr_start",
        arguments(
            "b",
            json!({"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
            &["b.txt"],
            "main",
        ),
    );
    assert_eq!(blocked["dispatch"], "Refused");
    assert_eq!(f.child_count(), 0);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"},{"path":"not-required.txt","content":"omit"}],"check_path":"a.txt"}),
        &["a.txt", "not-required.txt"],
    );
    let x = start(
        &f,
        "x",
        json!({"outputs":[{"path":"x.txt","content":"X"}],"check_path":"x.txt"}),
        &["x.txt"],
    );
    assert_eq!(a.dispatch, DispatchStatus::Applied);
    assert_eq!(x.dispatch, DispatchStatus::Applied);
    assert_ne!(a.identity.session, x.identity.session);
    assert_eq!(
        f.child_count(),
        2,
        "independent tasks overlap while B is blocked"
    );
    release(&f);
    let a = collect(&f, &a);
    let x = accept(&f, &collect(&f, &x));
    assert!(
        a.acceptance_ref.is_none(),
        "Ready/submission does not accept work"
    );
    let a = accept(&f, &a);
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    let b: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_start", json!({"handle":"request-b"}))).unwrap();
    assert_eq!(b.dispatch, DispatchStatus::Applied, "{b:?}");
    assert_eq!(
        fs::read_to_string(b.resources.coding.as_ref().unwrap().path.join("a.txt")).unwrap(),
        "A"
    );
    assert!(
        !b.resources
            .coding
            .as_ref()
            .unwrap()
            .path
            .join("not-required.txt")
            .exists()
    );
    let b = accept(&f, &collect(&f, &b));
    assert!(
        b.coding_target
            .as_ref()
            .unwrap()
            .candidate
            .input_baseline
            .is_some()
    );
    graph["edges"][1]["selected"] = selection(&b);
    configure(&f, 2, &graph);
    let c = start(
        &f,
        "c",
        json!({"expect":[{"path":"a.txt","content":"A"},{"path":"b.txt","content":"B"}],"outputs":[{"path":"c.txt","content":"C"}],"check_path":"c.txt"}),
        &["c.txt"],
    );
    assert_eq!(c.dispatch, DispatchStatus::Applied, "{c:?}");
    let c = accept(&f, &collect(&f, &c));
    let current = f.tool("ovrcr_start", json!({"handle":b.handle}));
    assert_eq!(current["identity"]["session"], json!(b.identity.session));
    assert_eq!(f.input_count(&b), 1);
    let resource = |row: &AssignmentSnapshot| {
        json!({
            "handle":row.handle,"identity":row.identity,"result_version":row.result_version,
            "obligation_revision":row.obligation_revision,
            "publication":row.publication.as_ref().unwrap().identity,
            "acceptance":row.acceptance_ref,"evidence":[],
            "completed_obligations":["accepted dependency implementation"],
            "retention":{"Keep":{"reason":"retain immutable dependency evidence"}},"previous":null
        })
    };
    let delivered: WorkflowSnapshot = serde_json::from_value(f.tool("ovrcr_deliver", json!({"declaration":{
        "request":"dependency-delivery-before-change","workflow":b.identity.workflow,
        "scope":"Deliver accepted B C and independent X","provenance":"explicit parent fixture decision",
        "resources":[resource(&b),resource(&c),resource(&x)]
    }}))).unwrap();
    let delivery = delivered.deliveries.last().unwrap().reference.clone();
    let original_x = fs::read(x.acceptance_ref.as_ref().unwrap().path.clone()).unwrap();
    let original_a = fs::read(a.coding_target.as_ref().unwrap().candidate.path.clone()).unwrap();
    graph["tasks"][0]["revision"] = json!(2);
    graph["tasks"][0]["request"] = json!("request-a-revised");
    let revised = configure(&f, 3, &graph);
    let affected = &revised.dependencies.as_ref().unwrap().invalidated_tasks;
    assert!(affected.iter().any(|task| task == "b") && affected.iter().any(|task| task == "c"));
    assert!(!affected.iter().any(|task| task == "x"));
    for source in [&b, &c] {
        let observed = f.tool("ovrcr_collect", json!({"handle":source.handle}));
        assert!(observed["dependency_gap"].is_string());
        assert_eq!(
            observed["obligation_revision"],
            source.obligation_revision + 1
        );
        let mut old_resource = resource(source);
        old_resource["previous"] = json!(delivery);
        let stale = f.tool("ovrcr_deliver", json!({"declaration":{
            "request":format!("stale-dependency-delivery-{}",source.handle),
            "workflow":source.identity.workflow,"scope":"old dependency declaration",
            "provenance":"original exact obligations retained for negative proof","resources":[old_resource]
        }}));
        assert_ne!(
            stale["dispatch"], "Applied",
            "stale delivery cannot become current: {stale}"
        );
        assert!(
            stale["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("obligations changed")),
            "{stale}"
        );
        let denied: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_archive", json!({
            "request":format!("stale-dependency-archive-{}",source.handle),"handle":source.handle,
            "expected_identity":source.identity,"expected_result_version":source.result_version,
            "expected_obligation_revision":source.obligation_revision,
            "expected_control_epoch":source.control.epoch,"delivery":delivery,"continuation_of":null
        }))).unwrap();
        assert!(denied.archive_receipts.iter().any(|receipt| receipt.outcome
            == ArchiveOutcome::Refused {
                reason: ArchiveRefusal::ObligationChanged
            }));
        assert_eq!(
            observed["identity"]["session"],
            json!(source.identity.session)
        );
    }
    let independent = f.tool("ovrcr_collect", json!({"handle":x.handle}));
    assert!(independent["dependency_gap"].is_null());
    assert_eq!(independent["acceptance"], "accepted");
    assert_eq!(independent["obligation_revision"], x.obligation_revision);
    let mut independent_resource = resource(&x);
    independent_resource["previous"] = json!(delivery);
    let independent_delivery: WorkflowSnapshot = serde_json::from_value(f.tool("ovrcr_deliver", json!({"declaration":{
        "request":"independent-dependency-delivery-remains-valid","workflow":x.identity.workflow,
        "scope":"Deliver independently accepted X","provenance":"unaffected original obligations",
        "resources":[independent_resource]
    }}))).unwrap();
    assert!(independent_delivery.deliveries.iter().any(|entry|entry.reference.request=="independent-dependency-delivery-remains-valid"));
    assert_eq!(
        fs::read(x.acceptance_ref.as_ref().unwrap().path.clone()).unwrap(),
        original_x
    );
    assert_eq!(
        fs::read(a.coding_target.as_ref().unwrap().candidate.path.clone()).unwrap(),
        original_a
    );
    assert_eq!(
        f.child_count(),
        4,
        "invalidation never interrupts or replaces running children"
    );
}

#[test]
fn public_dependencies_reject_cycles_missing_owners_conflicts_and_unaccepted_sources_before_effects()
 {
    let f = super::followups::OwnedFollowup::new();
    let graph = json!({"tasks":[task("a",false),task("x",false),task("join",true)],"edges":[edge("a","join","shared.txt"),edge("x","join","shared.txt")]});
    for invalid in [
        json!({"tasks":[task("a",false),task("join",true)],"edges":[edge("a","join","a.txt"),edge("join","a","b.txt")]}),
        json!({"tasks":[task("a",false)],"edges":[edge("missing","a","x.txt")]}),
        {
            let mut bad = graph.clone();
            bad["tasks"][1]["workspace"] = bad["tasks"][0]["workspace"].clone();
            bad
        },
    ] {
        let refused = raw(
            &f,
            "ovrcr_dependencies",
            json!({"expected_version":0,"graph":invalid}),
        );
        assert!(refused["error"].is_string(), "{refused}");
        assert_eq!(f.child_count(), 0);
    }
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"shared.txt","content":"A"}],"check_path":"shared.txt"}),
        &["shared.txt"],
    );
    let x = start(
        &f,
        "x",
        json!({"outputs":[{"path":"shared.txt","content":"X"}],"check_path":"shared.txt"}),
        &["shared.txt"],
    );
    release(&f);
    let a = collect(&f, &a);
    let x = accept(&f, &collect(&f, &x));
    let mut graph = graph;
    graph["edges"][0]["selected"] = json!({"target":a.coding_target,"acceptance":{"identity":a.identity,"result_version":1,"result_digest":a.publication.as_ref().unwrap().identity.digest,"path":a.brief.parent().unwrap().join("acceptance-v1.json"),"digest":"0".repeat(64)},"reviews":[]});
    graph["edges"][1]["selected"] = selection(&x);
    configure(&f, 1, &graph);
    let blocked = f.tool(
        "ovrcr_start",
        arguments(
            "join",
            json!({"outputs":[{"path":"join.txt","content":"join"}],"check_path":"join.txt"}),
            &["join.txt"],
            "main",
        ),
    );
    assert_eq!(blocked["dispatch"], "Refused");
    assert_eq!(f.child_count(), 2);
    let a = accept(&f, &a);
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 2, &graph);
    let conflict = f.tool("ovrcr_start", json!({"handle":"request-join"}));
    assert_eq!(conflict["dispatch"], "Refused");
    assert!(conflict["reason"].as_str().unwrap().contains("conflict"));
    assert_eq!(f.child_count(), 2);
}

#[test]
fn public_dependencies_authenticate_exact_independent_review_and_refuse_inherited_write_scope_expansion()
 {
    let f = super::followups::OwnedFollowup::new();
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    let source = json!({"kind":"Spec","reference":"fixture:spec","digest":"b8d21caa27bc9cca1b5087d6d9b4e2e89afa68375aaa4d318fac1e28316ac7e5"});
    let requirement = json!({"axis":"Spec","sources":[source.clone()]});
    graph["edges"][0]["required_reviews"] = json!([requirement]);
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    let config = json!({"expect":[{"path":"a.txt","content":"A"}],"outputs":[{"path":"b.txt","content":"B"},{"path":"a.txt","content":"unauthorized"}],"check_path":"b.txt"});
    let blocked = f.tool("ovrcr_start", arguments("b", config, &["b.txt"], "main"));
    assert_eq!(blocked["dispatch"], "Refused");
    assert_eq!(f.child_count(), 1);
    let target = json!({"coding":a.coding_target,"sources":[source.clone()]});
    let reviewer:AssignmentSnapshot=serde_json::from_value(f.tool("ovrcr_start",json!({"project":PROJECT,"workspace":f.workspace,"scope":"DEPENDENCY_FIXTURE\n{}","model_provider":"test","model":"fixture-model","acceptance_evidence":["exact independent spec review"],"review":{"implementation":a.handle,"axis":"Spec","target":target,"source_documents":[{"source":source,"content":"actual fixture spec"}]}}))).unwrap();
    let reviewer = collect(&f, &reviewer);
    assert_ne!(reviewer.identity.session, a.identity.session);
    let review = json!({"axis":"Spec","target":target,"reviewer":reviewer.identity,"publication":reviewer.publication.as_ref().unwrap().identity});
    graph["edges"][0]["selected"]["reviews"] = json!([review]);
    configure(&f, 2, &graph);
    let b: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_start", json!({"handle":"request-b"}))).unwrap();
    assert_eq!(b.dispatch, DispatchStatus::Applied);
    wait_file(&b.brief.with_extension("json.published"));
    let publication: Value =
        serde_json::from_slice(&fs::read(b.brief.with_extension("json.published")).unwrap())
            .unwrap();
    assert!(
        publication["error"]
            .as_str()
            .is_some_and(|reason| reason.contains("outside authorized paths")),
        "{publication}"
    );
    let observed: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":b.handle}))).unwrap();
    assert!(observed.submission.is_none());
    assert!(observed.acceptance_ref.is_none());
}

#[test]
fn public_dependencies_verify_actual_commit_content_not_ancestry_and_keep_inherited_committed_inputs()
 {
    let f = super::followups::OwnedFollowup::new();
    let original_main = live::git(&f.live.repo, &["rev-parse", "main"])
        .trim()
        .to_owned();
    let mut graph = json!({"tasks":[task("a",false),task("good",false),task("reverted",false),task("cherry",false),task("deep",true)],"edges":[edge("a","good","a.txt"),edge("a","reverted","a.txt"),edge("a","cherry","a.txt"),edge("good","deep","good.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    let path = &a.resources.coding.as_ref().unwrap().path;
    // Explicit test-owned Git history; the product never performs these commits.
    fs::write(path.join("a.txt"), "A").unwrap();
    live::git(path, &["add", "a.txt"]);
    live::git(path, &["commit", "-m", "fixture accepted output"]);
    let accepted_commit = live::git(path, &["rev-parse", "HEAD"]).trim().to_owned();
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    fs::remove_file(path.join("a.txt")).unwrap();
    live::git(path, &["add", "a.txt"]);
    live::git(path, &["commit", "-m", "fixture revert accepted content"]);
    let reverted_commit = live::git(path, &["rev-parse", "HEAD"]).trim().to_owned();
    live::git(
        path,
        &["checkout", "-b", "fixture-matching-content", &original_main],
    );
    fs::write(path.join("a.txt"), "A").unwrap();
    live::git(path, &["add", "a.txt"]);
    live::git(
        path,
        &["commit", "-m", "fixture independent matching content"],
    );
    let cherry_commit = live::git(path, &["rev-parse", "HEAD"]).trim().to_owned();
    for index in 0..3 {
        graph["edges"][index]["selected"] = selection(&a);
    }
    configure(&f, 1, &graph);
    let refused=f.tool("ovrcr_start",arguments("reverted",json!({"outputs":[{"path":"reverted.txt","content":"must not start"}],"check_path":"reverted.txt"}),&["reverted.txt"],&reverted_commit));
    assert_eq!(refused["dispatch"], "Refused");
    assert_eq!(
        f.child_count(),
        1,
        "accepted ancestry with reverted bytes is insufficient"
    );
    let mut accepted_children = Vec::new();
    for (name, base) in [
        ("good", accepted_commit.as_str()),
        ("cherry", cherry_commit.as_str()),
    ] {
        let args = arguments(
            name,
            json!({"expect":[{"path":"a.txt","content":"A"}],"outputs":[{"path":format!("{name}.txt"),"content":name}],"check_path":format!("{name}.txt")}),
            &[&format!("{name}.txt")],
            base,
        );
        let started: AssignmentSnapshot =
            serde_json::from_value(f.tool("ovrcr_start", args)).unwrap();
        assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
        let completed = accept(&f, &collect(&f, &started));
        assert_eq!(
            live::git(
                &completed.resources.coding.as_ref().unwrap().path,
                &["rev-parse", "HEAD"]
            )
            .trim(),
            base,
            "snapshot capture and admission do not make hidden commits"
        );
        accepted_children.push(completed);
    }
    graph["edges"][3]["selected"] = selection(&accepted_children[0]);
    graph["edges"][3]["artifacts"] = json!(["a.txt", "good.txt"]);
    let baseline: Value = serde_json::from_slice(
        &fs::read(
            &accepted_children[0]
                .coding_target
                .as_ref()
                .unwrap()
                .candidate
                .input_baseline
                .as_ref()
                .unwrap()
                .path,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        baseline["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "a.txt")
    );
    assert!(
        !baseline["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "a.txt")
    );
    configure(&f, 2, &graph);
    let deep_response = f.tool(
        "ovrcr_start",
        arguments(
            "deep",
            json!({"expect":[{"path":"a.txt","content":"A"},{"path":"good.txt","content":"good"}],"outputs":[{"path":"deep.txt","content":"deep"}],"check_path":"deep.txt"}),
            &["deep.txt"],
            "main",
        ),
    );
    eprintln!(
        "DEPENDENCY_R1_NAMED_ADMISSION dispatch={} reason={}",
        deep_response["dispatch"], deep_response["reason"]
    );
    assert_eq!(deep_response["dispatch"], "Applied", "{deep_response}");
    let deep: AssignmentSnapshot = serde_json::from_value(deep_response).unwrap();
    eprintln!(
        "DEPENDENCY_R1_NAMED_INPUT dispatch={:?} reason={:?}",
        deep.dispatch, deep.reason
    );
    assert_eq!(deep.dispatch, DispatchStatus::Applied, "{deep:?}");
    let deep = accept(&f, &collect(&f, &deep));
    assert_eq!(
        live::git(
            &deep.resources.coding.as_ref().unwrap().path,
            &["rev-parse", "HEAD"]
        )
        .trim(),
        original_main
    );
    assert_eq!(
        live::git(&f.live.repo, &["rev-parse", "main"]).trim(),
        original_main
    );
    assert_eq!(
        live::git(path, &["rev-parse", "HEAD"]).trim(),
        cherry_commit,
        "predecessor mutable checkout is not recaptured"
    );
}

#[test]
fn public_dependencies_preserve_modes_deletion_symlink_and_block_missing_or_tampered_inputs() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph = json!({"tasks":[task("a",false),task("b",true),task("unavailable",false)],"edges":[edge("a","b","a.txt"),edge("a","unavailable","a.txt")]});
    graph["edges"][0]["artifacts"] = json!(["a.txt", "tool.sh", "link", "README"]);
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"},{"path":"tool.sh","content":"#!/bin/sh\nexit 0\n","executable":true},{"path":"link","symlink":"a.txt"},{"path":"README","missing":true}],"check_path":"a.txt"}),
        &["a.txt", "tool.sh", "link", "README"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    for index in 0..2 {
        graph["edges"][index]["selected"] = selection(&a);
    }
    configure(&f, 1, &graph);
    let unavailable = f.tool(
        "ovrcr_start",
        arguments(
            "unavailable",
            json!({"outputs":[{"path":"own.txt","content":"not started"}],"check_path":"own.txt"}),
            &["own.txt"],
            "main",
        ),
    );
    assert_eq!(unavailable["dispatch"], "Refused");
    assert_eq!(
        f.child_count(),
        1,
        "snapshot integration needs explicit authority"
    );
    let candidate = &a.coding_target.as_ref().unwrap().candidate;
    let original = fs::read(&candidate.path).unwrap();
    fs::write(&candidate.path, b"partial candidate").unwrap();
    let config = json!({"expect":[{"path":"a.txt","content":"A"},{"path":"tool.sh","content":"#!/bin/sh\nexit 0\n","executable":true},{"path":"link","symlink":"a.txt"},{"path":"README","missing":true}],"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"});
    let tampered = f.tool("ovrcr_start", arguments("b", config, &["b.txt"], "main"));
    assert_eq!(tampered["dispatch"], "Refused");
    assert_eq!(f.child_count(), 1);
    fs::write(&candidate.path, &original).unwrap();
    let b: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_start", json!({"handle":"request-b"}))).unwrap();
    assert_eq!(b.dispatch, DispatchStatus::Applied, "{b:?}");
    let b = accept(&f, &collect(&f, &b));
    let workspace = &b.resources.coding.as_ref().unwrap().path;
    assert_eq!(fs::read_to_string(workspace.join("a.txt")).unwrap(), "A");
    assert_ne!(
        fs::metadata(workspace.join("tool.sh")).unwrap().mode() & 0o111,
        0
    );
    assert_eq!(
        fs::read_link(workspace.join("link")).unwrap(),
        Path::new("a.txt")
    );
    assert!(!workspace.join("README").exists());
    assert_eq!(
        fs::read(&candidate.path).unwrap(),
        original,
        "no recapture/rewrite of accepted predecessor evidence"
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dependency_no_process_retry_reuses_exact_authenticated_dirty_input_baseline() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    f.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 2,
        fault: Some(("coding-after-registration", 0)),
    });
    let failed = start(
        &f,
        "b",
        json!({"expect":[{"path":"a.txt","content":"A"}],"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
        &["b.txt"],
    );
    assert_eq!(failed.dispatch, DispatchStatus::Refused, "{failed:?}");
    assert_eq!(failed.execution_attempts, Some(0));
    let resource = failed.resources.coding.as_ref().unwrap();
    assert_eq!(resource.creation, WorkspaceCreation::Registered);
    assert!(resource.no_process_started);
    assert!(failed.identity.session.is_none());
    assert!(
        failed
            .reason
            .as_ref()
            .unwrap()
            .contains("owned failure after workspace registration")
    );
    assert_eq!(
        fs::read_to_string(resource.path.join("a.txt")).unwrap(),
        "A"
    );
    assert!(!resource.path.join("b.txt").exists());
    assert!(
        !live::git(&resource.path, &["status", "--porcelain"])
            .trim()
            .is_empty(),
        "authenticated inherited inputs intentionally make the worktree dirty"
    );
    assert_eq!(f.child_count(), 1);
    let receipts = failed.receipts.clone();
    let retry = f.tool("ovrcr_retry", json!({"handle":failed.handle}));
    eprintln!(
        "DEPENDENCY_RETRY_OBSERVED dispatch={} reason={} child_count={}",
        retry["dispatch"],
        retry["reason"],
        f.child_count()
    );
    assert_eq!(
        retry["dispatch"], "Applied",
        "exact inherited baseline after authoritative no-process refusal must remain retryable: {retry}"
    );
    let retry: AssignmentSnapshot = serde_json::from_value(retry).unwrap();
    assert_ne!(retry.handle, failed.handle);
    assert_eq!(retry.identity.task, failed.identity.task);
    assert_eq!(retry.identity.attempt, failed.identity.attempt + 1);
    assert_eq!(retry.execution_attempts, Some(1));
    let reused = retry.resources.coding.as_ref().unwrap();
    assert_eq!(reused.path, resource.path);
    assert_eq!(reused.git_identity, resource.git_identity);
    assert_eq!(reused.base, resource.base);
    let retry = accept(&f, &collect(&f, &retry));
    assert_eq!(f.input_count(&retry), 1);
    assert_eq!(f.child_count(), 2);
    let replay: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_retry", json!({"handle":failed.handle}))).unwrap();
    assert_eq!(replay.identity, retry.identity);
    assert_eq!(f.input_count(&retry), 1);
    assert_eq!(f.child_count(), 2);
    let original: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":failed.handle}))).unwrap();
    assert_eq!(original.receipts, receipts);
    let denied = f.tool("ovrcr_retry", json!({"handle":retry.handle}));
    assert_eq!(
        denied["dispatch"], "Refused",
        "a native-started implementation never qualifies for no-process retry"
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dependency_retry_keeps_arbitrary_dirtiness_and_changed_graph_blocked() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    f.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 2,
        fault: Some(("coding-after-registration", 0)),
    });
    let failed = start(
        &f,
        "b",
        json!({"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
        &["b.txt"],
    );
    assert_eq!(failed.dispatch, DispatchStatus::Refused);
    assert!(failed.resources.coding.as_ref().unwrap().no_process_started);
    let workspace = &failed.resources.coding.as_ref().unwrap().path;
    fs::write(workspace.join("a.txt"), "changed inherited content").unwrap();
    let changed = f.tool("ovrcr_retry", json!({"handle":failed.handle}));
    assert_ne!(changed["dispatch"], "Applied");
    assert!(
        changed["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("baseline changed")),
        "{changed}"
    );
    assert_eq!(f.child_count(), 1);
    fs::write(workspace.join("a.txt"), "A").unwrap();
    fs::write(workspace.join("unrelated.txt"), "unapproved extra data").unwrap();
    let dirty = f.tool("ovrcr_retry", json!({"handle":failed.handle}));
    assert_ne!(dirty["dispatch"], "Applied");
    assert!(
        dirty["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("baseline changed")),
        "{dirty}"
    );
    assert_eq!(f.child_count(), 1);
    assert!(!workspace.join("b.txt").exists());
    fs::remove_file(workspace.join("unrelated.txt")).unwrap();
    graph["tasks"][0]["revision"] = json!(2);
    graph["tasks"][0]["request"] = json!("new-a-contract");
    configure(&f, 2, &graph);
    let stale = f.tool("ovrcr_retry", json!({"handle":failed.handle}));
    assert_ne!(stale["dispatch"], "Applied");
    assert_eq!(f.child_count(), 1);
    assert!(!workspace.join("b.txt").exists());
    assert_eq!(fs::read_to_string(workspace.join("a.txt")).unwrap(), "A");
    let original: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":failed.handle}))).unwrap();
    assert_eq!(original.receipts, failed.receipts);
    assert!(original.dependency_gap.is_some());
}

#[cfg(feature = "acceptance-diagnostics")]
fn retry_prepare(
    f: &RecoveryFixture,
    original: &AssignmentSnapshot,
    request: &str,
    previous: &str,
) -> DelegationRequest {
    let mut contract: ResearchContract = serde_json::from_slice(
        &fs::read(original.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
    )
    .unwrap();
    contract.request = request.to_owned();
    contract.previous_request = Some(previous.to_owned());
    let mut prepare = f.proxy.original(DispatchOperation::Prepare);
    let DelegationRequest::Prepare { contract: sent, .. } = &mut prepare else {
        unreachable!()
    };
    *sent = contract;
    prepare
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dependency_retry_rejects_forged_lineage_conflicting_successor_and_unknown_launch() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    f.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 2,
        fault: Some(("coding-after-registration", 0)),
    });
    let failed = start(
        &f,
        "b",
        json!({"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
        &["b.txt"],
    );
    assert_eq!(failed.dispatch, DispatchStatus::Refused);
    assert!(failed.resources.coding.as_ref().unwrap().no_process_started);
    // A real parent source does not make a forged retry predecessor authoritative.
    let forged = retry_prepare(&f, &failed, "forged-dependency-retry", &a.handle);
    assert!(matches!(
        f.live.request(Request::Delegation(Box::new(forged))),
        Response::Error { .. }
    ));
    let mut changed = retry_prepare(&f, &failed, "changed-dependency-retry", &failed.handle);
    let DelegationRequest::Prepare { contract, .. } = &mut changed else {
        unreachable!()
    };
    contract.scope.push_str(" changed original scope");
    assert!(matches!(
        f.live.request(Request::Delegation(Box::new(changed))),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(f.child_count(), 1);
    f.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 2,
        fault: Some(("launch-before-effect", 0)),
    });
    let uncertain = f.tool("ovrcr_retry", json!({"handle":failed.handle}));
    let handle = uncertain["handle"].as_str().unwrap();
    let durable = f.reconcile(handle);
    assert_eq!(
        durable.dispatch,
        DispatchStatus::LaunchUnknown,
        "{uncertain}"
    );
    assert_eq!(
        durable
            .receipts
            .iter()
            .find(|r| r.operation == DispatchOperation::Launch)
            .unwrap()
            .outcome,
        DispatchOutcome::Unknown
    );
    assert!(durable.identity.session.is_none());
    assert_eq!(f.child_count(), 1);
    let denied = f.tool("ovrcr_retry", json!({"handle":durable.handle}));
    assert_eq!(denied["dispatch"], "Refused", "{denied}");
    // The original refusal is already linked to the exact uncertain successor.
    let conflicting = retry_prepare(
        &f,
        &failed,
        "conflicting-dependency-successor",
        &failed.handle,
    );
    assert!(matches!(
        f.live.request(Request::Delegation(Box::new(conflicting))),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    let observed = f.reconcile(&durable.handle);
    assert_eq!(observed.receipts, durable.receipts);
    assert_eq!(f.child_count(), 1);
    let workspace = &failed.resources.coding.as_ref().unwrap().path;
    assert_eq!(fs::read_to_string(workspace.join("a.txt")).unwrap(), "A");
    assert!(!workspace.join("b.txt").exists());
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dependency_retry_refuses_partial_and_uncertain_native_input() {
    for (limit, expected) in [
        (7, DispatchOutcome::Partial),
        (usize::MAX, DispatchOutcome::Unknown),
    ] {
        let f = super::followups::OwnedFollowup::new();
        let mut graph =
            json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
        configure(&f, 0, &graph);
        let a = start(
            &f,
            "a",
            json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
            &["a.txt"],
        );
        release(&f);
        let a = accept(&f, &collect(&f, &a));
        graph["edges"][0]["selected"] = selection(&a);
        configure(&f, 1, &graph);
        f.proxy.arm(ControlRule {
            operation: "InitialInput",
            loss: 2,
            fault: Some(("input", limit)),
        });
        let first = start(
            &f,
            "b",
            json!({"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
            &["b.txt"],
        );
        let original = f.reconcile(&first.handle);
        let input = original
            .receipts
            .iter()
            .find(|r| r.operation == DispatchOperation::InitialInput)
            .unwrap();
        assert_eq!(input.outcome, expected);
        assert!(
            !original
                .resources
                .coding
                .as_ref()
                .unwrap()
                .no_process_started
        );
        if limit == 7 {
            assert_eq!(input.input.as_ref().unwrap().written_bytes, 7);
        } else {
            wait_file(&original.brief.with_extension("json.started"));
            assert_eq!(f.input_count(&original), 1);
        }
        let denied = f.tool("ovrcr_retry", json!({"handle":original.handle}));
        assert_eq!(denied["dispatch"], "Refused", "{denied}");
        let forged = retry_prepare(&f, &original, "unsafe-dependency-retry", &original.handle);
        assert!(matches!(
            f.live.request(Request::Delegation(Box::new(forged))),
            Response::Error { .. }
        ));
        assert_eq!(f.reconcile(&original.handle).receipts, original.receipts);
        assert_eq!(
            f.child_count(),
            2,
            "retry never allocates another native process"
        );
    }
}

#[test]
fn public_dependency_configured_capacity_refusal_releases_only_owned_predecessor_and_retries_same_task()
 {
    let f = super::followups::OwnedFollowup::new();
    let configured = f
        .live
        .command()
        .args(["settings", "set", "delegation.max_live_children", "1"])
        .output()
        .unwrap();
    assert!(configured.status.success());
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    let refused = start(
        &f,
        "b",
        json!({"expect":[{"path":"a.txt","content":"A"}],"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
        &["b.txt"],
    );
    assert_eq!(refused.dispatch, DispatchStatus::Refused);
    assert!(refused.retryable_capacity_refusal);
    assert_eq!(refused.execution_attempts, Some(0));
    assert!(refused.identity.session.is_none());
    let resource = refused.resources.coding.as_ref().unwrap();
    assert_eq!(resource.creation, WorkspaceCreation::Prepared);
    assert!(!resource.path.exists());
    assert_eq!(f.child_count(), 1);
    let receipts = refused.receipts.clone();
    assert_eq!(
        f.live.request(Request::CloseTerminal {
            session: a.identity.session.unwrap(),
            expected_run: a.identity.run.unwrap()
        }),
        Response::Ok
    );
    let retried: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_retry", json!({"handle":refused.handle}))).unwrap();
    assert_eq!(retried.dispatch, DispatchStatus::Applied, "{retried:?}");
    assert_eq!(retried.identity.task, refused.identity.task);
    assert_eq!(retried.identity.attempt, refused.identity.attempt + 1);
    assert_eq!(retried.execution_attempts, Some(1));
    assert_eq!(
        retried.resources.coding.as_ref().unwrap().path,
        resource.path
    );
    assert_eq!(
        retried.resources.coding.as_ref().unwrap().base,
        resource.base
    );
    let completed = accept(&f, &collect(&f, &retried));
    assert_eq!(
        fs::read_to_string(resource.path.join("a.txt")).unwrap(),
        "A"
    );
    assert_eq!(f.input_count(&completed), 1);
    let replay: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_retry", json!({"handle":refused.handle}))).unwrap();
    assert_eq!(replay.identity, completed.identity);
    assert_eq!(f.input_count(&completed), 1);
    assert_eq!(
        f.child_count(),
        2,
        "one closed owned predecessor and one successor"
    );
    assert_eq!(f.reconcile(&refused.handle).receipts, receipts);
}

#[test]
fn public_dependency_changed_graph_or_selection_after_launch_blocks_original_initial_input() {
    for change in ["contract", "selection"] {
        let f = super::followups::OwnedFollowup::new();
        let mut graph =
            json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
        configure(&f, 0, &graph);
        let a = start(
            &f,
            "a",
            json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
            &["a.txt"],
        );
        release(&f);
        let a = accept(&f, &collect(&f, &a));
        graph["edges"][0]["selected"] = selection(&a);
        configure(&f, 1, &graph);
        // A lost committed Launch reply stops the public tool before SendInitial.
        f.proxy.arm(ControlRule {
            operation: "Launch",
            loss: 1,
            fault: None,
        });
        let first = f.tool(
            "ovrcr_start",
            arguments(
                "b",
                json!({"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
                &["b.txt"],
                "main",
            ),
        );
        assert_eq!(first["dispatch"], "Unknown");
        let launched = f.reconcile("request-b");
        assert!(launched.identity.session.is_some());
        assert_eq!(
            launched
                .receipts
                .iter()
                .find(|r| r.operation == DispatchOperation::Launch)
                .unwrap()
                .outcome,
            DispatchOutcome::Applied
        );
        assert!(
            !launched
                .receipts
                .iter()
                .any(|r| r.operation == DispatchOperation::InitialInput)
        );
        assert_eq!(f.input_count(&launched), 0);
        assert_eq!(f.byte_count(&launched), 0);
        assert_eq!(f.child_count(), 2);
        if change == "contract" {
            graph["tasks"][0]["revision"] = json!(2);
            graph["tasks"][0]["request"] = json!("replacement-a");
        } else {
            graph["edges"][0]["selected"] = Value::Null;
        }
        configure(&f, 2, &graph);
        let refused = f.recover(&launched.handle);
        assert_eq!(refused["dispatch"], "Refused", "{change}: {refused}");
        let observed = f.reconcile(&launched.handle);
        assert_eq!(observed.identity, launched.identity);
        assert!(observed.dependency_gap.is_some());
        assert_eq!(f.input_count(&observed), 0);
        assert_eq!(f.byte_count(&observed), 0);
        assert_eq!(f.child_count(), 2);
        assert!(
            !observed
                .resources
                .coding
                .as_ref()
                .unwrap()
                .path
                .join("b.txt")
                .exists()
        );
    }
}

fn dependency_control(
    f: &RecoveryFixture,
    operation: DispatchOperation,
    handle: &str,
) -> DelegationRequest {
    f.proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|request| match (operation, request) {
            (DispatchOperation::Prepare, DelegationRequest::Prepare { contract, .. }) => {
                contract.request == handle
            }
            (
                DispatchOperation::Launch,
                DelegationRequest::Launch {
                    handle: request, ..
                },
            )
            | (
                DispatchOperation::InitialInput,
                DelegationRequest::SendInitial {
                    handle: request, ..
                },
            ) => request == handle,
            _ => false,
        })
        .unwrap()
        .clone()
}

#[test]
fn public_dependency_lost_requests_and_replies_reconcile_original_baseline_without_duplicate_effects()
 {
    for operation in ["Prepare", "Launch", "InitialInput"] {
        for loss in [0, 1] {
            let f = super::followups::OwnedFollowup::new();
            let mut graph =
                json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
            configure(&f, 0, &graph);
            let a = start(
                &f,
                "a",
                json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
                &["a.txt"],
            );
            release(&f);
            let a = accept(&f, &collect(&f, &a));
            let candidate = a.coding_target.as_ref().unwrap().candidate.clone();
            let candidate_bytes = fs::read(&candidate.path).unwrap();
            graph["edges"][0]["selected"] = selection(&a);
            configure(&f, 1, &graph);
            f.proxy.arm(ControlRule {
                operation,
                loss,
                fault: None,
            });
            let first = f.tool("ovrcr_start", arguments("b", json!({"expect":[{"path":"a.txt","content":"A"}],"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}), &["b.txt"], "main"));
            assert_eq!(first["dispatch"], "Unknown", "{operation}/{loss}: {first}");
            assert!(
                first.get("receipts").is_none(),
                "transport loss is not a fabricated receipt"
            );
            let recovered: AssignmentSnapshot =
                serde_json::from_value(f.recover("request-b")).unwrap();
            assert_eq!(
                recovered.dispatch,
                DispatchStatus::Applied,
                "{operation}/{loss}: {recovered:?}"
            );
            let completed = accept(&f, &collect(&f, &recovered));
            let baseline = completed
                .coding_target
                .as_ref()
                .unwrap()
                .candidate
                .input_baseline
                .as_ref()
                .unwrap();
            let baseline_bytes = fs::read(&baseline.path).unwrap();
            assert_eq!(
                fs::read_to_string(
                    completed
                        .resources
                        .coding
                        .as_ref()
                        .unwrap()
                        .path
                        .join("a.txt")
                )
                .unwrap(),
                "A"
            );
            for kind in [DispatchOperation::Launch, DispatchOperation::InitialInput] {
                let same = f.control(dependency_control(&f, kind, "request-b"));
                assert_eq!(same.identity, completed.identity);
                assert_eq!(same.receipts, completed.receipts);
            }
            let replay: AssignmentSnapshot =
                serde_json::from_value(f.recover("request-b")).unwrap();
            assert_eq!(replay.identity, completed.identity);
            assert_eq!(f.input_count(&completed), 1);
            assert_eq!(f.child_count(), 2);
            assert_eq!(fs::read(&baseline.path).unwrap(), baseline_bytes);
            assert_eq!(fs::read(&candidate.path).unwrap(), candidate_bytes);
        }
    }
}

#[test]
fn public_dependency_rejects_foreign_stale_wrong_axis_and_changed_immutable_review_sources() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    let source = json!({"kind":"Spec","reference":"fixture:spec","digest":"b8d21caa27bc9cca1b5087d6d9b4e2e89afa68375aaa4d318fac1e28316ac7e5"});
    let target = json!({"coding":a.coding_target,"sources":[source.clone()]});
    let reviewer:AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_start", json!({"project":PROJECT,"workspace":f.workspace,"scope":"DEPENDENCY_FIXTURE\n{}","model_provider":"test","model":"fixture-model","acceptance_evidence":["exact independent spec review"],"review":{"implementation":a.handle,"axis":"Spec","target":target,"source_documents":[{"source":source,"content":"actual fixture spec"}]}}))).unwrap();
    let reviewer = collect(&f, &reviewer);
    let stale_review = json!({"axis":"Spec","target":target,"reviewer":reviewer.identity,"publication":reviewer.publication.as_ref().unwrap().identity});
    // A real authenticated second publication makes the original exact evidence stale.
    let continued: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&f, &reviewer, None)).unwrap();
    assert_eq!(continued.result_version, reviewer.result_version + 1);
    let reviewer = collect(&f, &continued);
    assert_eq!(reviewer.publication.as_ref().unwrap().identity.version, 2);
    let review = json!({"axis":"Spec","target":target,"reviewer":reviewer.identity,"publication":reviewer.publication.as_ref().unwrap().identity});
    graph["edges"][0]["required_reviews"] = json!([{"axis":"Spec","sources":[source]}]);
    graph["edges"][0]["selected"] = selection(&a);
    graph["edges"][0]["selected"]["reviews"] = json!([review]);
    let valid = graph.clone();
    let contract_path = reviewer.brief.parent().unwrap().join("contract-v1.json");
    let contract_bytes = fs::read(&contract_path).unwrap();
    let mut version = 1;
    for attack in [
        "foreign-worker",
        "stale-publication",
        "wrong-axis",
        "wrong-source",
        "changed-source-bytes",
    ] {
        let mut bad = valid.clone();
        match attack {
            "foreign-worker" => {
                bad["edges"][0]["selected"]["reviews"][0]["reviewer"] = json!(a.identity);
                bad["edges"][0]["selected"]["reviews"][0]["publication"] =
                    json!(a.publication.as_ref().unwrap().identity);
            }
            "stale-publication" => {
                bad["edges"][0]["selected"]["reviews"][0] = stale_review.clone();
            }
            "wrong-axis" => bad["edges"][0]["selected"]["reviews"][0]["axis"] = json!("Standards"),
            "wrong-source" => {
                bad["edges"][0]["required_reviews"][0]["sources"][0]["digest"] =
                    json!("0".repeat(64))
            }
            "changed-source-bytes" => {
                let mut changed: Value = serde_json::from_slice(&contract_bytes).unwrap();
                changed["review"]["source_documents"][0]["content"] =
                    json!("changed immutable source bytes");
                fs::write(&contract_path, serde_json::to_vec(&changed).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        configure(&f, version, &bad);
        version += 1;
        let denied = f.tool(
            "ovrcr_start",
            arguments(
                "b",
                json!({"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
                &["b.txt"],
                "main",
            ),
        );
        // Restore immutable source before any assertion can unwind the fixture.
        fs::write(&contract_path, &contract_bytes).unwrap();
        assert_eq!(denied["dispatch"], "Refused", "{attack}: {denied}");
        assert_eq!(
            f.child_count(),
            2,
            "{attack}: no implementation child before authenticated review"
        );
        assert!(!f.live.workspace_root.join("dependency-b").exists());
    }
    configure(&f, version, &valid);
    let b: AssignmentSnapshot = serde_json::from_value(f.recover("request-b")).unwrap();
    assert_eq!(b.dispatch, DispatchStatus::Applied);
    let _ = accept(&f, &collect(&f, &b));
    assert_eq!(f.child_count(), 3);
    assert_eq!(fs::read(&contract_path).unwrap(), contract_bytes);
}

#[test]
fn public_dependency_baseline_references_cannot_substitute_foreign_or_tampered_inherited_content() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph = json!({"tasks":[task("a",false),task("b",true),task("c",true)],"edges":[edge("a","b","a.txt"),edge("b","c","b.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    let b = start(
        &f,
        "b",
        json!({"expect":[{"path":"a.txt","content":"A"}],"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
        &["b.txt"],
    );
    let b = accept(&f, &collect(&f, &b));
    graph["edges"][1]["selected"] = selection(&b);
    let valid = graph.clone();
    let baseline = b
        .coding_target
        .as_ref()
        .unwrap()
        .candidate
        .input_baseline
        .as_ref()
        .unwrap();
    let original = fs::read(&baseline.path).unwrap();
    let foreign = f.live.root.path().join("foreign-baseline.json");
    fs::write(&foreign, &original).unwrap();
    let mut version = 2;
    for attack in ["foreign-path", "forged-digest", "tampered-bytes"] {
        let mut bad = valid.clone();
        match attack {
            "foreign-path" => {
                bad["edges"][1]["selected"]["target"]["candidate"]["input_baseline"]["path"] =
                    json!(foreign)
            }
            "forged-digest" => {
                bad["edges"][1]["selected"]["target"]["candidate"]["input_baseline"]["digest"] =
                    json!("0".repeat(64))
            }
            "tampered-bytes" => {
                fs::write(&baseline.path, b"partial immutable baseline").unwrap();
            }
            _ => unreachable!(),
        }
        configure(&f, version, &bad);
        version += 1;
        let denied = f.tool(
            "ovrcr_start",
            arguments(
                "c",
                json!({"outputs":[{"path":"c.txt","content":"C"}],"check_path":"c.txt"}),
                &["c.txt"],
                "main",
            ),
        );
        fs::write(&baseline.path, &original).unwrap();
        assert_eq!(denied["dispatch"], "Refused", "{attack}: {denied}");
        assert_eq!(f.child_count(), 2);
        assert!(!f.live.workspace_root.join("dependency-c").exists());
    }
    configure(&f, version, &valid);
    let c: AssignmentSnapshot = serde_json::from_value(f.recover("request-c")).unwrap();
    assert_eq!(c.dispatch, DispatchStatus::Applied);
    let _ = accept(&f, &collect(&f, &c));
    assert_eq!(fs::read(&baseline.path).unwrap(), original);
    assert_eq!(fs::read(&foreign).unwrap(), original);
    assert_eq!(f.child_count(), 3);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_dependency_partial_materialization_retains_bytes_without_native_start_or_blind_replay() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph =
        json!({"tasks":[task("a",false),task("b",true)],"edges":[edge("a","b","a-first.txt")]});
    graph["edges"][0]["artifacts"] = json!(["a-first.txt", "z-blocked/last.txt"]);
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a-first.txt","content":"first inherited bytes"},{"path":"z-blocked/last.txt","content":"last inherited bytes"}],"check_path":"a-first.txt"}),
        &["a-first.txt", "z-blocked/last.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    let workspace = f.live.workspace_root.join("dependency-b");
    let blocked = workspace.join("z-blocked");
    let release_path = f.live.root.path().join("fault.release");
    struct Restore {
        directory: std::path::PathBuf,
        release: std::path::PathBuf,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o755));
            let _ = fs::write(&self.release, "release");
        }
    }
    let restore = Restore {
        directory: blocked.clone(),
        release: release_path.clone(),
    };
    f.proxy.arm(ControlRule {
        operation: "Launch",
        loss: 2,
        fault: Some(("dependency-before-materialization", 0)),
    });
    let output = f.live.root.path().join("partial-materialization.json");
    assert_eq!(f.live.request(Request::SendTerminal { session:f.parent, text:format!("tool:{}", json!({"name":"ovrcr_start","args":arguments("b", json!({"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}), &["b.txt"], "main"),"output":output})), submit:true }), Response::Ok);
    wait_file(&f.live.root.path().join("fault.hit"));
    assert!(
        workspace.exists(),
        "barrier follows creation and registration"
    );
    fs::create_dir(&blocked).unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o555)).unwrap();
    fs::write(&release_path, "release").unwrap();
    wait_file(&output);
    // Restore even when later assertions fail; Drop also releases a panicking barrier.
    drop(restore);
    let value: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert!(value.get("error").is_none(), "{value}");
    let failed: AssignmentSnapshot = serde_json::from_value(value["details"].clone()).unwrap();
    assert_eq!(failed.dispatch, DispatchStatus::Refused, "{failed:?}");
    assert!(
        failed
            .reason
            .as_ref()
            .unwrap()
            .contains("create inherited file"),
        "{failed:?}"
    );
    assert!(failed.resources.coding.as_ref().unwrap().no_process_started);
    assert_eq!(
        failed.resources.coding.as_ref().unwrap().creation,
        WorkspaceCreation::Registered
    );
    assert!(failed.identity.session.is_none());
    assert_eq!(failed.execution_attempts, Some(0));
    assert_eq!(
        fs::read_to_string(workspace.join("a-first.txt")).unwrap(),
        "first inherited bytes"
    );
    assert!(!blocked.join("last.txt").exists());
    assert!(!workspace.join("b.txt").exists());
    let receipts = failed.receipts.clone();
    let recovered = f.recover(&failed.handle);
    assert_eq!(recovered["dispatch"], "Refused");
    assert_eq!(f.reconcile(&failed.handle).receipts, receipts);
    let denied = f.tool("ovrcr_retry", json!({"handle":failed.handle}));
    assert_ne!(
        denied["dispatch"], "Applied",
        "partial input materialization is not an exact reusable baseline"
    );
    assert_eq!(f.child_count(), 1);
    assert_eq!(
        fs::read_to_string(workspace.join("a-first.txt")).unwrap(),
        "first inherited bytes"
    );
    assert!(!blocked.join("last.txt").exists());
    assert!(fs::read_dir(&workspace).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ovrcr-input-")
    }));
}

#[test]
fn public_dependency_invalidation_overflow_and_store_failure_leave_graph_and_obligations_atomic() {
    let f = super::followups::OwnedFollowup::new();
    let mut graph = json!({"tasks":[task("a",false),task("b",true),task("x",false)],"edges":[edge("a","b","a.txt")]});
    configure(&f, 0, &graph);
    let a = start(
        &f,
        "a",
        json!({"outputs":[{"path":"a.txt","content":"A"}],"check_path":"a.txt"}),
        &["a.txt"],
    );
    let x = start(
        &f,
        "x",
        json!({"outputs":[{"path":"x.txt","content":"X"}],"check_path":"x.txt"}),
        &["x.txt"],
    );
    release(&f);
    let a = accept(&f, &collect(&f, &a));
    let x = accept(&f, &collect(&f, &x));
    graph["edges"][0]["selected"] = selection(&a);
    configure(&f, 1, &graph);
    let b = start(
        &f,
        "b",
        json!({"expect":[{"path":"a.txt","content":"A"}],"outputs":[{"path":"b.txt","content":"B"}],"check_path":"b.txt"}),
        &["b.txt"],
    );
    let b = accept(&f, &collect(&f, &b));
    let db_path = f.live.config.join("registry.sqlite3");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    let record_bytes = |handle: &str| {
        db.query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [handle],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };
    let workflow_bytes = || {
        db.query_row(
            "SELECT metadata FROM delegation_workflows WHERE workflow=?1",
            [&a.identity.workflow],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };
    let original_a = record_bytes(&a.handle);
    let original_b = record_bytes(&b.handle);
    let original_x = record_bytes(&x.handle);
    let original_workflow = workflow_bytes();
    let mut revised = graph.clone();
    revised["tasks"][0]["revision"] = json!(2);
    revised["tasks"][0]["request"] = json!("revised-a-contract");
    let acceptance_bytes = fs::read(b.acceptance_ref.as_ref().unwrap().path.clone()).unwrap();
    // Inject only the existing counter in this fixture's durable record, not authority.
    let mut exhausted: Value = serde_json::from_str(&original_b).unwrap();
    exhausted["obligation_revision"] = json!(u64::MAX);
    let exhausted = serde_json::to_string(&exhausted).unwrap();
    db.execute(
        "UPDATE delegations SET metadata=?1 WHERE handle=?2",
        rusqlite::params![exhausted, b.handle],
    )
    .unwrap();
    let overflow = raw(
        &f,
        "ovrcr_dependencies",
        json!({"expected_version":2,"graph":revised}),
    );
    assert!(
        overflow["error"]
            .as_str()
            .is_some_and(|reason| reason.contains("dependency obligation revision exhausted")),
        "{overflow}"
    );
    assert_eq!(
        record_bytes(&a.handle),
        original_a,
        "earlier affected member cannot commit before a later overflow"
    );
    assert_eq!(record_bytes(&b.handle), exhausted);
    assert_eq!(record_bytes(&x.handle), original_x);
    assert_eq!(workflow_bytes(), original_workflow);
    db.execute(
        "UPDATE delegations SET metadata=?1 WHERE handle=?2",
        rusqlite::params![original_b, b.handle],
    )
    .unwrap();
    // The real SQLite write fails after assignment updates, forcing their rollback too.
    db.execute_batch("CREATE TRIGGER owned_dependency_graph_abort BEFORE UPDATE ON delegation_workflows BEGIN SELECT RAISE(ABORT, 'owned dependency graph commit failure'); END;").unwrap();
    struct DropTrigger(std::path::PathBuf);
    impl Drop for DropTrigger {
        fn drop(&mut self) {
            if let Ok(db) = rusqlite::Connection::open(&self.0) {
                let _ = db.execute_batch("DROP TRIGGER IF EXISTS owned_dependency_graph_abort;");
            }
        }
    }
    let trigger = DropTrigger(db_path);
    let failed = raw(
        &f,
        "ovrcr_dependencies",
        json!({"expected_version":2,"graph":revised}),
    );
    drop(trigger);
    assert!(
        failed["error"]
            .as_str()
            .is_some_and(|reason| reason.contains("owned dependency graph commit failure")),
        "{failed}"
    );
    assert_eq!(record_bytes(&a.handle), original_a);
    assert_eq!(record_bytes(&b.handle), original_b);
    assert_eq!(record_bytes(&x.handle), original_x);
    assert_eq!(
        workflow_bytes(),
        original_workflow,
        "graph and obligations commit together"
    );
    let committed = configure(&f, 2, &revised);
    assert_eq!(committed.dependencies.as_ref().unwrap().version, 3);
    for child in [&a, &b] {
        let persisted: Value = serde_json::from_str(&record_bytes(&child.handle)).unwrap();
        assert_eq!(
            persisted["obligation_revision"],
            child.obligation_revision + 1
        );
    }
    assert_eq!(record_bytes(&x.handle), original_x);
    assert_eq!(
        fs::read(b.acceptance_ref.as_ref().unwrap().path.clone()).unwrap(),
        acceptance_bytes
    );
    assert_eq!(
        f.child_count(),
        3,
        "failed metadata updates never allocate or stop native children"
    );
}

#[test]
fn public_dependency_removed_isolated_task_revokes_stale_delivery_and_archive_eligibility() {
    // Separate fixtures prevent the original bug's new delivery from superseding
    // the old archive head before the archive arm can exercise it.
    let mut violations = Vec::new();
    for arm in ["delivery", "archive"] {
        let f = super::followups::OwnedFollowup::new();
        let graph = json!({"tasks":[task("x",false),task("y",false)],"edges":[]});
        configure(&f, 0, &graph);
        let x = start(
            &f,
            "x",
            json!({"outputs":[{"path":"x.txt","content":"X"}],"check_path":"x.txt"}),
            &["x.txt"],
        );
        let y = start(
            &f,
            "y",
            json!({"outputs":[{"path":"y.txt","content":"Y"}],"check_path":"y.txt"}),
            &["y.txt"],
        );
        release(&f);
        let x = accept(&f, &collect(&f, &x));
        let y = accept(&f, &collect(&f, &y));
        let continued: AssignmentSnapshot =
            serde_json::from_value(super::followups::followup(&f, &x, None)).unwrap();
        assert_eq!(continued.result_version, x.result_version + 1);
        let x = accept(&f, &collect(&f, &continued));
        assert_eq!(
            x.history.len(),
            1,
            "preserve actual nonempty version history"
        );
        let history = serde_json::to_vec(&x.history).unwrap();
        let mut immutable = vec![
            x.brief.clone(),
            x.result_path.clone(),
            x.acceptance_ref.as_ref().unwrap().path.clone(),
            x.coding_target.as_ref().unwrap().candidate.path.clone(),
        ];
        for version in &x.history {
            immutable.extend([
                version.contract.clone(),
                version.brief.clone(),
                version.acceptance.as_ref().unwrap().path.clone(),
            ]);
            let brief: Value = serde_json::from_slice(&fs::read(&version.brief).unwrap()).unwrap();
            immutable.push(std::path::PathBuf::from(brief["output"].as_str().unwrap()));
        }
        let immutable: Vec<_> = immutable
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
        let record = |handle: &str| {
            db.query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [handle],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        let original_y = record(&y.handle);
        let resource = |child: &AssignmentSnapshot| DeliveryResource {
            handle: child.handle.clone(),
            identity: child.identity.clone(),
            result_version: child.result_version,
            obligation_revision: child.obligation_revision,
            publication: child.publication.as_ref().map(|p| p.identity.clone()),
            acceptance: child.acceptance_ref.clone(),
            evidence: vec![],
            completed_obligations: vec!["accepted isolated dependency task".into()],
            retention: RetentionChoice::ArchiveEligible,
            previous: None,
        };
        let original = DeliveryDeclaration {
            request: format!("removed-x-original-{arm}"),
            workflow: x.identity.workflow.clone(),
            scope: "Deliver accepted isolated X".into(),
            provenance: "Explicit parent archive eligibility before removal".into(),
            resources: vec![resource(&x)],
        };
        let delivered: WorkflowSnapshot =
            serde_json::from_value(f.tool("ovrcr_deliver", json!({"declaration":original})))
                .unwrap();
        let old_delivery = delivered.deliveries.last().unwrap().clone();
        let old_delivery_bytes = serde_json::to_vec(&old_delivery).unwrap();
        let revised = json!({"tasks":[task("y",false)],"edges":[]});
        let changed = configure(&f, 1, &revised);
        let current: AssignmentSnapshot =
            serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":x.handle}))).unwrap();
        let persisted: Value = serde_json::from_str(&record(&x.handle)).unwrap();
        eprintln!(
            "DEPENDENCY_R1_REMOVED arm={arm} invalidated={:?} old_obligation={} current_obligation={}",
            changed.dependencies.as_ref().unwrap().invalidated_tasks,
            x.obligation_revision,
            current.obligation_revision
        );
        if changed.dependencies.as_ref().unwrap().version != 2
            || !changed
                .dependencies
                .as_ref()
                .unwrap()
                .invalidated_tasks
                .contains(&"x".into())
            || current.obligation_revision != x.obligation_revision + 1
            || persisted["obligation_revision"] != x.obligation_revision + 1
        {
            violations.push(format!(
                "{arm}: removed X obligation did not advance with the graph"
            ));
        }
        assert!(
            current
                .dependency_gap
                .as_ref()
                .is_some_and(|gap| gap.contains("removed"))
        );
        assert_eq!(serde_json::to_vec(&current.history).unwrap(), history);
        // Replaying an immutable declaration remains historical inspection; it
        // cannot restore the old obligation or grant current archive authority.
        let replay: WorkflowSnapshot =
            serde_json::from_value(f.tool("ovrcr_deliver", json!({"request":original.request})))
                .unwrap();
        assert_eq!(
            serde_json::to_vec(
                replay
                    .deliveries
                    .iter()
                    .find(|entry| entry.reference == old_delivery.reference)
                    .unwrap()
            )
            .unwrap(),
            old_delivery_bytes
        );
        if arm == "delivery" {
            let mut stale = original.clone();
            stale.request = "removed-x-new-stale-delivery".into();
            stale.resources[0].previous = Some(old_delivery.reference.clone());
            let response = f.tool("ovrcr_deliver", json!({"declaration":stale}));
            eprintln!(
                "DEPENDENCY_R1_REMOVED_NEW_DELIVERY dispatch={} reason={} recorded={}",
                response["dispatch"],
                response["reason"],
                response["deliveries"].is_array()
            );
            if !response["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("obligations changed"))
                || response["deliveries"].is_array()
            {
                violations.push("new stale delivery was admitted for removed X".into());
            }
        } else {
            let request = ArchiveRequest {
                request: "removed-x-stale-archive".into(),
                handle: x.handle.clone(),
                expected_identity: x.identity.clone(),
                expected_result_version: x.result_version,
                expected_obligation_revision: x.obligation_revision,
                expected_control_epoch: x.control.epoch,
                delivery: old_delivery.reference.clone(),
                continuation_of: None,
            };
            let response: AssignmentSnapshot = serde_json::from_value(
                f.tool("ovrcr_archive", serde_json::to_value(&request).unwrap()),
            )
            .unwrap();
            let receipt = response
                .archive_receipts
                .iter()
                .find(|receipt| receipt.intent.request.request == request.request)
                .unwrap();
            eprintln!(
                "DEPENDENCY_R1_REMOVED_ARCHIVE outcome={:?}",
                receipt.outcome
            );
            if receipt.outcome
                != (ArchiveOutcome::Refused {
                    reason: ArchiveRefusal::ObligationChanged,
                })
            {
                violations.push(format!(
                    "old ArchiveEligible declaration admitted removed X: {:?}",
                    receipt.outcome
                ));
            }
        }
        for (path, bytes) in &immutable {
            assert_eq!(
                &fs::read(path).unwrap(),
                bytes,
                "immutable acceptance/result/history file {}",
                path.display()
            );
        }
        assert_eq!(
            record(&y.handle),
            original_y,
            "unrelated Y durable bytes are unchanged"
        );
        let y_now: AssignmentSnapshot =
            serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":y.handle}))).unwrap();
        assert!(y_now.dependency_gap.is_none());
        assert_eq!(y_now.obligation_revision, y.obligation_revision);
        let y_delivery = DeliveryDeclaration {
            request: format!("unrelated-y-still-current-{arm}"),
            workflow: y.identity.workflow.clone(),
            scope: "Deliver unaffected Y".into(),
            provenance: "Independent accepted task retains its exact obligations".into(),
            resources: vec![resource(&y)],
        };
        let y_delivery: WorkflowSnapshot =
            serde_json::from_value(f.tool("ovrcr_deliver", json!({"declaration":y_delivery})))
                .unwrap();
        assert!(y_delivery.deliveries.iter().any(
            |delivery| delivery.reference.request == format!("unrelated-y-still-current-{arm}")
        ));
        assert_eq!(record(&y.handle), original_y);
    }
    assert!(
        violations.is_empty(),
        "removed-task stale-authority violations: {violations:?}"
    );
}
