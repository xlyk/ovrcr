//! Registered Pi tools -> actual control client -> binary Server -> private files/PTY.
//! These deterministic model-work fixtures are separate from installed Pi UI proof.
use super::followups::OwnedFollowup;
use super::*;

fn start(f: &RecoveryFixture) -> AssignmentSnapshot {
    let started: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_start", serde_json::json!({
        "project": PROJECT, "workspace": f.workspace, "scope": "QUESTION_FILE_ONLY read README within existing authority",
        "model_provider": "test", "model": "fixture-model", "acceptance_evidence": ["README citation"]
    }))).unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied);
    wait_file(&started.brief.with_extension("json.question-published"));
    let published: serde_json::Value = serde_json::from_slice(
        &fs::read(started.brief.with_extension("json.question-published")).unwrap(),
    )
    .unwrap();
    assert!(published.get("error").is_none(), "{published}");
    let snapshot: AssignmentSnapshot =
        serde_json::from_value(published["details"].clone()).unwrap();
    assert_eq!(snapshot.questions.len(), 1);
    snapshot
}

fn reference(snapshot: &AssignmentSnapshot) -> QuestionReference {
    snapshot
        .questions
        .iter()
        .find(|q| q.current)
        .unwrap()
        .reference
        .clone()
}

fn collect(
    f: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
    question: &QuestionReference,
) -> AssignmentQuestion {
    let collected: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_collect_question",
        serde_json::json!({"handle": snapshot.handle, "reference": question}),
    ))
    .unwrap();
    collected
        .collected_question
        .expect("separately fetched exact question")
}

fn answer(
    f: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
    question: &QuestionReference,
) -> serde_json::Value {
    f.tool("ovrcr_answer_question", answer_args(snapshot, question))
}

fn answer_args(snapshot: &AssignmentSnapshot, question: &QuestionReference) -> serde_json::Value {
    serde_json::json!({"handle": snapshot.handle, "question": question,
        "expected_control_epoch": snapshot.control.epoch, "basis": "ExistingAuthority",
        "scope": "ANSWER_SCOPE_SECRET: Read only the assigned README.", "answer": "ANSWER_BODY_SECRET: Use the Introduction section."})
}

fn count(snapshot: &AssignmentSnapshot) -> usize {
    fs::read_to_string(snapshot.brief.with_extension("json.answer-count"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

fn received(snapshot: &AssignmentSnapshot) -> QuestionAnswer {
    wait_file(&snapshot.brief.with_extension("json.answer-received"));
    serde_json::from_slice(
        &fs::read(snapshot.brief.with_extension("json.answer-received")).unwrap(),
    )
    .unwrap()
}

fn command(
    _f: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
    sequence: u32,
    action: serde_json::Value,
) -> serde_json::Value {
    let path = snapshot.brief.with_extension("json.question-command");
    let mut action = action;
    action["sequence"] = sequence.into();
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec(&action).unwrap()).unwrap();
    fs::rename(temporary, path).unwrap();
    let reply = snapshot
        .brief
        .with_extension(format!("json.question-command-{sequence}"));
    wait_file(&reply);
    serde_json::from_slice(&fs::read(reply).unwrap()).unwrap()
}

fn original_answer(f: &RecoveryFixture) -> DelegationRequest {
    f.proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| matches!(r, DelegationRequest::AnswerQuestion { .. }))
        .unwrap()
        .clone()
}

fn assert_private_metadata(f: &RecoveryFixture) {
    let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
    let mut query = db.prepare("SELECT metadata FROM delegations").unwrap();
    for raw in query.query_map([], |r| r.get::<_, String>(0)).unwrap() {
        let raw = raw.unwrap();
        for secret in [
            "QUESTION_BODY_SECRET",
            "QUESTION_CONTEXT_SECRET",
            "ANSWER_BODY_SECRET",
            "ANSWER_SCOPE_SECRET",
        ] {
            assert!(
                !raw.contains(secret),
                "private question/answer bodies must not enter retained metadata"
            );
        }
    }
    let events = serde_json::to_string(&f.live.request(Request::Events { follow: false })).unwrap();
    for secret in [
        "QUESTION_BODY_SECRET",
        "QUESTION_CONTEXT_SECRET",
        "ANSWER_BODY_SECRET",
        "ANSWER_SCOPE_SECRET",
        "Introduction",
        "Installation",
    ] {
        assert!(
            !events.contains(secret),
            "Server Events cannot carry question/answer content"
        );
    }
}

#[cfg(feature = "acceptance-diagnostics")]
struct EffectRelease(std::path::PathBuf);
#[cfg(feature = "acceptance-diagnostics")]
impl Drop for EffectRelease {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "release owned answer");
    }
}

#[cfg(feature = "acceptance-diagnostics")]
fn held_answer(
    f: &RecoveryFixture,
    current: &AssignmentSnapshot,
) -> (std::path::PathBuf, EffectRelease) {
    let release = EffectRelease(f.live.root.path().join("fault.release"));
    f.proxy.arm(ControlRule {
        operation: "AnswerQuestion",
        loss: 2,
        fault: Some(("answer", 0)),
    });
    let output = f.live.root.path().join("held-answer.json");
    let command = format!(
        "tool:{}",
        serde_json::json!({"name": "ovrcr_answer_question", "args": answer_args(current, &reference(current)), "output": output, "background": true})
    );
    assert_eq!(
        f.live.request(Request::SendTerminal {
            session: f.parent,
            text: command,
            submit: true
        }),
        Response::Ok
    );
    wait_file(&f.live.root.path().join("fault.hit"));
    assert_eq!(
        fs::read_to_string(f.live.root.path().join("fault.hit")).unwrap(),
        "answer"
    );
    (output, release)
}

#[cfg(feature = "acceptance-diagnostics")]
fn finish_held(output: &Path, release: EffectRelease) -> serde_json::Value {
    drop(release);
    wait_file(output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(reply.get("error").is_none(), "{reply}");
    reply["details"].clone()
}

fn durable(f: &RecoveryFixture, handle: &str) -> serde_json::Value {
    let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [handle],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn replace_durable_fixture(f: &RecoveryFixture, handle: &str, metadata: &serde_json::Value) {
    // Explicit retained-storage fault injection; this does not impersonate a
    // registered publisher or establish authority from a private file.
    let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE delegations SET metadata=?1 WHERE handle=?2",
            rusqlite::params![serde_json::to_string(metadata).unwrap(), handle]
        )
        .unwrap(),
        1
    );
}

fn question_path(
    snapshot: &AssignmentSnapshot,
    reference: &QuestionReference,
) -> std::path::PathBuf {
    snapshot.brief.parent().unwrap().join(format!(
        "question-r{}-{}-v{}.json",
        reference.result_version, reference.question_id, reference.question_version
    ))
}

fn bounded_public_control(live: &Live, request: Request) -> anyhow::Result<Response> {
    let mut stream = std::os::unix::net::UnixStream::connect(&live.socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    exchange_preamble(&mut stream)?;
    client::request(&mut stream, 1, request)
}

#[test]
fn public_missing_and_closed_question_publishers_refuse_without_blocking_control_or_mutating_evidence()
 {
    let f = super::observation::Fixture::new(10000);
    let started = f.start("QUESTION_FILE_ONLY bounded publisher admission");
    wait_file(&started.brief.with_extension("json.question-published"));
    let published: serde_json::Value = serde_json::from_slice(
        &fs::read(started.brief.with_extension("json.question-published")).unwrap(),
    )
    .unwrap();
    let current: AssignmentSnapshot = serde_json::from_value(published["details"].clone()).unwrap();
    let exact = reference(&current);
    let collected: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_collect_question",
        serde_json::json!({"handle": current.handle, "reference": exact}),
    ))
    .unwrap();
    let mut revised = collected.collected_question.unwrap();
    revised.question_version += 1;
    revised.supersedes = Some(exact);
    revised.question = "Which Introduction subsection should I read?".into();
    let stored = || {
        let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
        let raw: String = db
            .query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [&current.handle],
                |r| r.get(0),
            )
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        serde_json::json!({"questions": value["questions"], "answers": value["answers"], "publication_intent": value["publication_intent"], "receipts": value["receipts"], "result_digest": value["result_digest"], "result_version": value["result_version"]})
    };
    let files = || {
        let mut entries: Vec<_> = fs::read_dir(current.brief.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|p| {
                p.file_name().unwrap().to_str().is_some_and(|name| {
                    name.starts_with("question-") || name.starts_with("submission-")
                })
            })
            .map(|p| (p.file_name().unwrap().to_owned(), fs::read(&p).unwrap()))
            .collect();
        entries.sort();
        entries
    };
    // Capture ownership before the deliberate RED deadlock; teardown never queries
    // the blocked Server or the serial fault proxy.
    let root = f.live.root.path().to_owned();
    let server = f.live.server_pid().unwrap();
    let groups = f.live.owned_groups();
    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for (label, session) in [
            ("missing", SessionId(u64::MAX)),
            ("closed", current.identity.session.unwrap()),
        ] {
            if label == "closed" {
                let command_path = current.brief.with_extension("json.question-command");
                fs::write(
                    command_path,
                    serde_json::to_vec(&serde_json::json!({"sequence": 1, "question": revised}))
                        .unwrap(),
                )
                .unwrap();
                let reply_path = current.brief.with_extension("json.question-command-1");
                wait_file(&reply_path);
                let valid: serde_json::Value =
                    serde_json::from_slice(&fs::read(reply_path).unwrap()).unwrap();
                assert!(
                    valid.get("error").is_none(),
                    "valid child publication: {valid}"
                );
                assert_eq!(valid["details"]["questions"].as_array().unwrap().len(), 2);
                assert_eq!(
                    f.live.request(Request::CloseTerminal {
                        session,
                        expected_run: current.identity.run.unwrap()
                    }),
                    Response::Ok
                );
            }
            let before = stored();
            let before_files = files();
            let request = DelegationRequest::PublishQuestion {
                auth: DelegationAuth {
                    session,
                    capability: "deliberately-no-authority".into(),
                    controller: None,
                    source: None,
                },
                handle: current.handle.clone(),
                question: revised.clone(),
            };
            eprintln!("PUBLISHER_AUTH_REQUEST label={label} request={request:?}");
            let started = Instant::now();
            let publication =
                bounded_public_control(&f.live, Request::Delegation(Box::new(request)));
            let independent = bounded_public_control(&f.live, Request::Inspect);
            eprintln!(
                "PUBLISHER_AUTH_RESULT label={label} elapsed_ms={} publication={publication:?} independent={:?}",
                started.elapsed().as_millis(),
                independent
                    .as_ref()
                    .map(|r| matches!(r, Response::Inventory { .. }))
            );
            assert_eq!(
                stored(),
                before,
                "refusal cannot mutate publication/answer/receipt intents"
            );
            assert_eq!(
                files(),
                before_files,
                "refusal cannot link question/result files"
            );
            assert!(
                matches!(publication, Ok(Response::Error { .. })),
                "publisher must refuse within its bounded public request"
            );
            assert!(
                matches!(independent, Ok(Response::Inventory { .. })),
                "independent control remains responsive"
            );
        }
    }));
    // This exact child is still owned by Live and cannot have its PID reused
    // before it is reaped. Kill even on RED so no deadlocked cleanup request runs.
    let signal = unsafe { libc::kill(server as i32, libc::SIGKILL) };
    let stop = f.live.join_within(Duration::from_secs(2));
    eprintln!("PUBLISHER_AUTH_FORCED_STOP server={server} signal_result={signal} stop={stop:?}");
    assert_eq!(signal, 0);
    assert!(
        matches!(stop, live::Stop::Exited(status) if std::os::unix::process::ExitStatusExt::signal(&status) == Some(libc::SIGKILL)),
        "owned Server must be reaped after the intentional forced stop"
    );
    drop(f);
    let absent = |id| unsafe { libc::kill(id, 0) } == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
    let clean =
        absent(server as i32) && groups.iter().all(|group| absent(-*group)) && !root.exists();
    eprintln!(
        "PUBLISHER_AUTH_CLEANUP root={} server={server} groups={groups:?} clean={clean}",
        root.display()
    );
    assert!(clean);
    if let Err(panic) = verdict {
        std::panic::resume_unwind(panic);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_observation_parent_closed_after_first_auth_refuses_without_relocking_retained() {
    let mut f = RecoveryFixture::new();
    let current = start(&f);
    let DelegationRequest::SendInitial { auth, .. } =
        f.proxy.original(DispatchOperation::InitialInput)
    else {
        unreachable!()
    };
    // The real parent auth was captured on the registered production tool path.
    // Retire only this test's proxy; the request below uses the actual socket.
    f.proxy.retire();
    let root = f.live.root.path().to_owned();
    let server = f.live.server_pid().unwrap();
    let mut groups = f.live.owned_groups();
    groups.extend(f.live.session_groups());
    groups.sort_unstable();
    groups.dedup();
    for group in &groups {
        f.live.own_group(*group);
    }
    let stored = || {
        let db = rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap();
        let assignment: String = db
            .query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [&current.handle],
                |r| r.get(0),
            )
            .unwrap();
        let workflow: String = db
            .query_row(
                "SELECT metadata FROM delegation_workflows WHERE workflow=?1",
                [&current.identity.workflow],
                |r| r.get(0),
            )
            .unwrap();
        (assignment, workflow)
    };
    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fs::write(
            root.join("fault"),
            format!("assignment-after-auth|{}|0", current.handle),
        )
        .unwrap();
        let request = Request::Delegation(Box::new(DelegationRequest::Observe {
            auth,
            handles: vec![current.handle.clone()],
            after: None,
            wait_ms: 0,
        }));
        let mut observing = std::os::unix::net::UnixStream::connect(&f.live.socket).unwrap();
        observing
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        observing
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        exchange_preamble(&mut observing).unwrap();
        write_frame(
            &mut observing,
            &ClientMessage {
                request_id: 701,
                request,
            },
        )
        .unwrap();
        wait_file(&root.join("fault.hit"));
        assert_eq!(
            fs::read_to_string(root.join("fault.hit")).unwrap(),
            "assignment-after-auth"
        );
        let closed = bounded_public_control(
            &f.live,
            Request::CloseTerminal {
                session: f.parent,
                expected_run: SessionRunId(1),
            },
        );
        eprintln!(
            "PARENT_AUTH_CLOSED_AFTER_FIRST_AUTH parent={} result={closed:?}",
            f.parent.0
        );
        assert!(
            matches!(closed, Ok(Response::Ok)),
            "public close must complete while observation holds no retained lock"
        );
        let before = stored();
        let observation_path = current.brief.parent().unwrap().join("observation-v1.json");
        let checkpoint = fs::read(&observation_path).ok();
        fs::write(
            root.join("fault.release"),
            "resume exactly the authenticated observation",
        )
        .unwrap();
        let observed: anyhow::Result<ServerMessage> = read_frame(&mut observing);
        let independent = bounded_public_control(&f.live, Request::Inspect);
        eprintln!(
            "PARENT_AUTH_RECHECK_RESULT observed={observed:?} independent={:?}",
            independent
                .as_ref()
                .map(|r| matches!(r, Response::Inventory { .. }))
        );
        assert_eq!(
            stored(),
            before,
            "read-only refusal cannot alter assignment/workflow authority or receipts"
        );
        assert_eq!(
            fs::read(observation_path).ok(),
            checkpoint,
            "refused observation cannot publish progress"
        );
        assert!(
            matches!(
                observed,
                Ok(ServerMessage::Response {
                    request_id: 701,
                    response: Response::Error { .. }
                })
            ),
            "closed parent must refuse reauthentication within the request bound"
        );
        assert!(
            matches!(independent, Ok(Response::Inventory { .. })),
            "independent control must remain responsive"
        );
    }));
    let signal = unsafe { libc::kill(server as i32, libc::SIGKILL) };
    let stop = f.live.join_within(Duration::from_secs(2));
    eprintln!("PARENT_AUTH_FORCED_STOP server={server} signal_result={signal} stop={stop:?}");
    assert_eq!(signal, 0);
    assert!(
        matches!(stop, live::Stop::Exited(status) if std::os::unix::process::ExitStatusExt::signal(&status) == Some(libc::SIGKILL))
    );
    drop(f);
    let absent = |id| unsafe { libc::kill(id, 0) } == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
    let clean =
        absent(server as i32) && groups.iter().all(|group| absent(-*group)) && !root.exists();
    eprintln!(
        "PARENT_AUTH_RECHECK_CLEANUP root={} server={server} groups={groups:?} clean={clean}",
        root.display()
    );
    assert!(clean);
    if let Err(panic) = verdict {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn public_question_valid_private_file_without_authenticated_intent_cannot_be_collected_or_answered()
{
    let f = OwnedFollowup::new();
    let current = start(&f);
    let q = reference(&current);
    let path = question_path(&current, &q);
    let bytes = fs::read(&path).unwrap();
    let original = durable(&f, &current.handle);
    let mut missing_intent = original.clone();
    missing_intent["questions"] = serde_json::json!([]);
    replace_durable_fixture(&f, &current.handle, &missing_intent);
    // These exact complete bytes, digest, owner and permissions really came from
    // this child. Losing the authenticated intent still makes the file uncommitted.
    let refused = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_collect_question",
        serde_json::json!({"handle": current.handle, "reference": q}),
        &f.live.root.path().join("uncommitted-question.json"),
    );
    assert!(refused.get("error").is_some());
    let before = f.byte_count(&current);
    assert_eq!(answer(&f, &current, &q)["dispatch"], "Refused");
    assert_eq!(f.byte_count(&current), before);
    assert_eq!(count(&current), 0);
    assert!(f.reconcile(&current.handle).questions.is_empty());
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes,
        "uncommitted evidence is preserved, not adopted or deleted"
    );
    replace_durable_fixture(&f, &current.handle, &original);
    assert_eq!(collect(&f, &current, &q).identity, current.identity);
}

#[test]
fn public_question_corruption_and_escaping_link_refuse_before_answer_admission() {
    for kind in ["corrupt", "symlink"] {
        let f = OwnedFollowup::new();
        let current = start(&f);
        let q = reference(&current);
        let path = question_path(&current, &q);
        let original = fs::read(&path).unwrap();
        let outside = f.live.root.path().join("private-outside-question.json");
        if kind == "corrupt" {
            let mut changed: AssignmentQuestion = serde_json::from_slice(&original).unwrap();
            changed.question.push_str(" unauthorized revision");
            fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        } else {
            fs::write(&outside, &original).unwrap();
            fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
            fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(&outside, &path).unwrap();
        }
        let collected = tool_reply(
            &f.live,
            f.parent,
            "ovrcr_collect_question",
            serde_json::json!({"handle": current.handle, "reference": q}),
            &f.live.root.path().join("bad-question.json"),
        );
        assert!(collected.get("error").is_some());
        let before = f.byte_count(&current);
        let result = answer(&f, &current, &q);
        assert_eq!(
            result["dispatch"], "Unknown",
            "an internal file-validation error conservatively reports transport uncertainty: {result}"
        );
        assert_eq!(f.byte_count(&current), before);
        assert_eq!(count(&current), 0);
        let retained = durable(&f, &current.handle);
        assert!(
            retained["answers"].as_array().unwrap().is_empty(),
            "invalid files cannot create answer intents"
        );
        assert_eq!(retained["questions"].as_array().unwrap().len(), 1);
        if kind == "symlink" {
            assert_eq!(fs::read(&outside).unwrap(), original);
        }
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_private_file_changes_at_attainable_pre_effect_pause_preserve_unsent_intent() {
    for artifact in [
        "question-corrupt",
        "question-symlink",
        "answer-corrupt",
        "answer-symlink",
    ] {
        let f = OwnedFollowup::new();
        let current = start(&f);
        let (output, release) = held_answer(&f, &current);
        let intent = durable(&f, &current.handle)["answers"][0].clone();
        let path = if artifact.starts_with("question") {
            question_path(&current, &reference(&current))
        } else {
            current.brief.parent().unwrap().join(format!(
                "answer-{}.json",
                intent["request"].as_str().unwrap()
            ))
        };
        let original = fs::read(&path).unwrap();
        let outside = f.live.root.path().join("outside-original.json");
        if artifact.ends_with("corrupt") {
            fs::write(&path, b"{incomplete-private-artifact").unwrap();
        } else {
            fs::write(&outside, &original).unwrap();
            fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
            fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(&outside, &path).unwrap();
        }
        let before = f.byte_count(&current);
        let result = finish_held(&output, release);
        assert_eq!(
            result["dispatch"],
            if artifact == "answer-corrupt" {
                "Refused"
            } else {
                "Unknown"
            },
            "an exact digest conflict refuses; internal file errors conservatively report uncertainty without delivery proof: {result}"
        );
        let observed = f.reconcile(&current.handle);
        let receipt = observed
            .receipts
            .iter()
            .find(|r| r.request == intent["request"].as_str().unwrap())
            .unwrap();
        assert_eq!(receipt.outcome, DispatchOutcome::Accepted);
        assert!(receipt.input.is_none());
        assert_eq!(durable(&f, &current.handle)["answers"][0], intent);
        assert_eq!(f.byte_count(&current), before);
        assert_eq!(count(&current), 0);
        if artifact.ends_with("symlink") {
            assert_eq!(fs::read(outside).unwrap(), original);
        }
    }
}

#[test]
fn public_question_history_capacity_refuses_new_version_without_losing_original_files() {
    let f = OwnedFollowup::new();
    let first = start(&f);
    let original_ref = reference(&first);
    let original_path = question_path(&first, &original_ref);
    let original = fs::read(&original_path).unwrap();
    let mut current = first.clone();
    for version in 2..=MAX_ASSIGNMENT_QUESTIONS as u32 {
        let body = next_question(&f, &current);
        assert_eq!(body.question_version, version);
        let reply = command(&f, &current, version, serde_json::json!({"question": body}));
        assert!(reply.get("error").is_none(), "version {version}: {reply}");
        current = serde_json::from_value(reply["details"].clone()).unwrap();
        assert_eq!(current.questions.len(), version as usize);
    }
    let before = durable(&f, &current.handle)["questions"].clone();
    let next = next_question(&f, &current);
    let refused = command(&f, &current, 65, serde_json::json!({"question": next}));
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("storage exhausted"),
        "{refused}"
    );
    let snapshot = f.reconcile(&current.handle);
    assert_eq!(snapshot.questions.len(), MAX_ASSIGNMENT_QUESTIONS);
    assert_eq!(reference(&snapshot), reference(&current));
    assert_eq!(durable(&f, &current.handle)["questions"], before);
    assert_eq!(fs::read(&original_path).unwrap(), original);
    assert_eq!(collect(&f, &snapshot, &original_ref).question_version, 1);
    assert_eq!(snapshot.questions.iter().filter(|q| q.current).count(), 1);
    assert!(
        !question_path(
            &current,
            &QuestionReference {
                question_version: 65,
                ..reference(&current)
            }
        )
        .exists()
    );
    assert_eq!(count(&current), 0);
    assert_eq!(snapshot.execution_attempts, Some(1));
    assert_private_metadata(&f);
}

#[test]
fn public_question_oversized_retained_record_refuses_publication_without_eviction_or_link() {
    let f = OwnedFollowup::new();
    let current = start(&f);
    let next = next_question(&f, &current);
    let mut record = durable(&f, &current.handle);
    record["reason"] = "preserved-oversized-metadata".repeat(12000).into();
    replace_durable_fixture(&f, &current.handle, &record);
    let refused = command(&f, &current, 1, serde_json::json!({"question": next}));
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("metadata storage exhausted"),
        "{refused}"
    );
    assert_eq!(
        durable(&f, &current.handle),
        record,
        "capacity refusal preserves original obligations exactly"
    );
    let q = reference(&current);
    assert!(
        !question_path(
            &current,
            &QuestionReference {
                question_version: 2,
                ..q.clone()
            }
        )
        .exists()
    );
    assert!(question_path(&current, &q).is_file());
    assert_eq!(count(&current), 0);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_zero_write_replay_stays_read_only_and_explicit_fresh_answer_can_deliver() {
    let f = OwnedFollowup::new();
    let current = start(&f);
    let q = reference(&current);
    let bytes = f.byte_count(&current);
    f.proxy.arm(ControlRule {
        operation: "AnswerQuestion",
        loss: 2,
        fault: Some(("input", 0)),
    });
    let rejected = answer(&f, &current, &q);
    let snapshot: AssignmentSnapshot = serde_json::from_value(rejected.clone()).unwrap();
    let receipt = snapshot
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::AnswerQuestion)
        .unwrap();
    assert_eq!(receipt.outcome, DispatchOutcome::Rejected);
    assert_eq!(receipt.input.as_ref().unwrap().written_bytes, 0);
    assert_eq!(f.byte_count(&current), bytes);
    assert_eq!(count(&current), 0);
    let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": current.handle, "request": receipt.request}),
    ))
    .unwrap();
    assert_eq!(replay.receipts, snapshot.receipts);
    assert_eq!(f.byte_count(&current), bytes);
    let fresh = answer(&f, &current, &q);
    assert_ne!(fresh["request"], rejected["request"]);
    let delivered = received(&current);
    assert_eq!(delivered.question, q);
    assert_eq!(count(&current), 1);
    assert_eq!(fresh["result_version"], 1);
    assert_eq!(fresh["execution_attempts"], 1);
    assert_eq!(
        durable(&f, &current.handle)["answers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn public_question_sixty_four_answer_effects_exhaust_storage_without_eviction_or_replay() {
    let f = OwnedFollowup::new();
    let first = start(&f);
    let mut current = first.clone();
    let mut original_request = String::new();
    let mut original_artifact = Vec::new();
    for number in 1..=MAX_ASSIGNMENT_QUESTIONS {
        if number > 1 {
            let revised = next_question(&f, &current);
            let published = command(
                &f,
                &current,
                number as u32,
                serde_json::json!({"question": revised}),
            );
            assert!(
                published.get("error").is_none(),
                "question {number}: {published}"
            );
            current = serde_json::from_value(published["details"].clone()).unwrap();
        }
        let result = answer(&f, &current, &reference(&current));
        current = serde_json::from_value(result.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while count(&current) != number {
            assert!(
                Instant::now() < deadline,
                "child must independently consume answer {number}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let delivered = received(&current);
        assert_eq!(delivered.question, reference(&current));
        assert_eq!(delivered.request, result["request"].as_str().unwrap());
        if number == 1 {
            original_request = delivered.request;
            original_artifact = fs::read(
                first
                    .brief
                    .parent()
                    .unwrap()
                    .join(format!("answer-{original_request}.json")),
            )
            .unwrap();
        }
        assert_eq!(current.execution_attempts, Some(1));
        assert_eq!(current.result_version, 1);
        assert_eq!(current.identity, first.identity);
    }
    let before = f.byte_count(&current);
    let frozen = durable(&f, &current.handle)["answers"].clone();
    assert_eq!(frozen.as_array().unwrap().len(), MAX_ASSIGNMENT_QUESTIONS);
    assert_eq!(
        current
            .receipts
            .iter()
            .filter(|r| r.operation == DispatchOperation::AnswerQuestion
                && r.outcome == DispatchOutcome::Applied)
            .count(),
        MAX_ASSIGNMENT_QUESTIONS
    );
    let refused = answer(&f, &current, &reference(&current));
    assert_eq!(refused["dispatch"], "Refused");
    assert!(
        refused["reason"]
            .as_str()
            .unwrap()
            .contains("answer storage exhausted")
    );
    let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": first.handle, "request": original_request}),
    ))
    .unwrap();
    assert_eq!(replay.receipts, current.receipts);
    assert_eq!(f.byte_count(&current), before);
    assert_eq!(count(&current), MAX_ASSIGNMENT_QUESTIONS);
    assert_eq!(durable(&f, &current.handle)["answers"], frozen);
    assert_eq!(
        fs::read(
            first
                .brief
                .parent()
                .unwrap()
                .join(format!("answer-{original_request}.json"))
        )
        .unwrap(),
        original_artifact
    );
    assert_eq!(f.child_count(), 1);
    assert_private_metadata(&f);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_answer_after_sparse_reserved_result_and_contract_keeps_one_execution() {
    let f = OwnedFollowup::new();
    let first = start(&f);
    let original_question = reference(&first);
    let original_answer = answer(&f, &first, &original_question);
    received(&first);
    let complete = super::followups::complete(&f, &first);
    let mut contract: ResearchContract = serde_json::from_slice(
        &fs::read(first.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
    )
    .unwrap();
    contract.scope = "QUESTION_FILE_ONLY reserved but never dispatched revision".into();
    f.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 2,
        fault: Some(("followup-after-files", 0)),
    });
    let reserved: AssignmentSnapshot = serde_json::from_value(super::followups::followup(
        &f,
        &complete,
        Some(contract.clone()),
    ))
    .unwrap();
    let reserved_receipt = reserved
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::Followup)
        .unwrap();
    assert_eq!(reserved_receipt.outcome, DispatchOutcome::Accepted);
    assert_eq!(reserved_receipt.result_version, 2);
    assert_eq!(reserved_receipt.identity.contract, 2);
    assert_eq!(reserved.result_version, 1);
    let request = reserved_receipt.request.clone();
    let mut dashboard = super::assignment_control::dashboard(&f, &first);
    let child = first.identity.session.unwrap();
    super::assignment_control::transition(
        &f,
        &mut dashboard,
        child,
        "sparse-question-take",
        true,
        1,
    );
    let returned = super::assignment_control::transition(
        &f,
        &mut dashboard,
        child,
        "sparse-question-return",
        false,
        1,
    );
    let confirmed = super::assignment_control::confirm(
        &f,
        &returned,
        "sparse-question-return",
        "sparse-question-confirm",
    );
    let rejected: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_followup",
        serde_json::json!({"handle": first.handle, "request": request}),
    ))
    .unwrap();
    assert_eq!(
        rejected
            .receipts
            .iter()
            .find(|r| r.request == request)
            .unwrap()
            .outcome,
        DispatchOutcome::Rejected
    );
    assert_eq!(rejected.result_version, 1);
    contract.scope = "QUESTION_FILE_ONLY actual revision after reserved gap".into();
    let latest: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&f, &confirmed, Some(contract))).unwrap();
    assert_eq!(latest.result_version, 3);
    assert_eq!(latest.identity.contract, 3);
    wait_file(&latest.brief.with_extension("json.question-published"));
    let current = f.reconcile(&latest.handle);
    let current_question = reference(&current);
    assert_eq!(current_question.result_version, 3);
    assert_eq!(current_question.assignment.contract, 3);
    let delivered = answer(&f, &current, &current_question);
    assert_eq!(delivered["result_version"], 3);
    let received = received(&current);
    assert_eq!(received.question, current_question);
    let after: AssignmentSnapshot = serde_json::from_value(delivered).unwrap();
    assert_eq!(after.execution_attempts, Some(1));
    assert_eq!(after.history.len(), 1);
    assert_eq!(after.history[0].result_version, 1);
    assert_eq!(
        collect(&f, &after, &original_question).identity,
        first.identity
    );
    let before = f.byte_count(&after);
    let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": first.handle, "request": original_answer["request"]}),
    ))
    .unwrap();
    assert_eq!(replay.receipts, after.receipts);
    assert_eq!(f.byte_count(&after), before);
    assert_eq!(f.child_count(), 1);
    assert_private_metadata(&f);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_cancellation_blocks_fresh_and_accepted_answers_but_keeps_late_evidence() {
    for mode in ["fresh", "accepted", "applied"] {
        let f = OwnedFollowup::new();
        let current = start(&f);
        super::parent_recovery::workflow(&f);
        if mode == "accepted" {
            f.proxy.arm(ControlRule {
                operation: "AnswerQuestion",
                loss: 2,
                fault: Some(("answer-before-input", 0)),
            });
        }
        let original = (mode != "fresh").then(|| answer(&f, &current, &reference(&current)));
        if mode == "applied" {
            received(&current);
        }
        let frozen = durable(&f, &current.handle)["answers"].clone();
        let cancelled: WorkflowSnapshot = serde_json::from_value(f.tool("ovrcr_cancel_workflow", serde_json::json!({"workflow": current.identity.workflow, "request": format!("question-cancel-{mode}")}))).unwrap();
        assert_eq!(
            cancelled.cancellation.unwrap().dispatch_closure,
            DispatchOutcome::Applied
        );
        let before = f.byte_count(&current);
        let revised = next_question(&f, &current);
        let late = command(&f, &current, 1, serde_json::json!({"question": revised}));
        assert!(
            late.get("error").is_none(),
            "cancellation retains late authenticated evidence: {late}"
        );
        let latest: AssignmentSnapshot = serde_json::from_value(late["details"].clone()).unwrap();
        let refused = answer(&f, &latest, &reference(&latest));
        assert_eq!(refused["dispatch"], "Refused");
        if mode == "fresh" {
            assert!(refused["reason"].as_str().unwrap().contains("cancelled"));
        }
        if let Some(original) = original {
            let replay = f.tool(
                "ovrcr_answer_question",
                serde_json::json!({"handle": current.handle, "request": original["request"]}),
            );
            if mode == "accepted" {
                assert_eq!(replay["dispatch"], "Refused");
                assert!(replay["reason"].as_str().unwrap().contains("cancelled"));
            } else {
                let replay: AssignmentSnapshot = serde_json::from_value(replay).unwrap();
                assert_eq!(
                    replay.questions[0].answer.as_ref().unwrap().outcome,
                    DispatchOutcome::Applied
                );
            }
        }
        assert_eq!(durable(&f, &current.handle)["answers"], frozen);
        assert_eq!(f.byte_count(&current), before);
        assert_eq!(latest.result_version, 1);
        assert_eq!(
            collect(&f, &latest, &reference(&current)).question_version,
            1
        );
    }
}

fn next_question(f: &RecoveryFixture, current: &AssignmentSnapshot) -> AssignmentQuestion {
    let old = reference(current);
    let mut question = collect(f, current, &old);
    question.question_version += 1;
    question.supersedes = Some(old);
    question.question = "QUESTION_BODY_SECRET: Which exact Introduction subsection?".into();
    question
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_publication_first_refuses_the_old_accepted_answer_without_discarding_it() {
    let f = OwnedFollowup::new();
    let current = start(&f);
    let old = reference(&current);
    f.proxy.arm(ControlRule {
        operation: "AnswerQuestion",
        loss: 2,
        fault: Some(("answer-before-input", 0)),
    });
    let pending = answer(&f, &current, &old);
    let request = pending["request"].as_str().unwrap();
    let original = durable(&f, &current.handle)["answers"].clone();
    let bytes = f.byte_count(&current);
    let published = command(
        &f,
        &current,
        1,
        serde_json::json!({"question": next_question(&f, &current)}),
    );
    let revised: AssignmentSnapshot = serde_json::from_value(published["details"].clone()).unwrap();
    assert_eq!(
        reference(&revised).question_version,
        2,
        "publication-first commits before answer admission"
    );
    let refused = f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": current.handle, "request": request}),
    );
    assert_eq!(refused["dispatch"], "Refused");
    assert_eq!(answer(&f, &revised, &old)["dispatch"], "Refused");
    let observed = f.reconcile(&current.handle);
    assert_eq!(
        observed
            .receipts
            .iter()
            .find(|r| r.request == request)
            .unwrap()
            .outcome,
        DispatchOutcome::Accepted
    );
    assert_eq!(
        durable(&f, &current.handle)["answers"],
        original,
        "supersession preserves the suspended intent and captured epochs"
    );
    assert_eq!(f.byte_count(&current), bytes);
    assert_eq!(count(&current), 0);
    assert_eq!(observed.result_version, 1);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_answer_first_proves_competing_publication_is_blocked_until_one_delivery() {
    let f = OwnedFollowup::new();
    let current = start(&f);
    let old = reference(&current);
    let mut publication = f
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| matches!(r, DelegationRequest::PublishQuestion { .. }))
        .unwrap()
        .clone();
    let revised = next_question(&f, &current);
    let DelegationRequest::PublishQuestion { question, .. } = &mut publication else {
        unreachable!()
    };
    *question = revised;
    let (output, release) = held_answer(&f, &current);
    let bytes = f.byte_count(&current);
    assert_eq!(count(&current), 0);
    fs::write(
        f.live.root.path().join("fault"),
        format!("question-mutation-blocked|{}|0", current.handle),
    )
    .unwrap();
    let socket = f.live.socket.clone();
    // The existing serial proxy owns the registered answer leg. The concurrent
    // leg uses the same actual child capability and public Server control API.
    let pending = std::thread::spawn(move || {
        let mut stream = connect_server(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        client::request(&mut stream, 1, Request::Delegation(Box::new(publication))).unwrap()
    });
    wait_file(&f.live.root.path().join("fault.blocked"));
    assert_eq!(
        fs::read_to_string(f.live.root.path().join("fault.blocked")).unwrap(),
        "question-mutation-blocked",
        "WouldBlock proves actual admission contention, not thread arrival"
    );
    assert_eq!(
        durable(&f, &current.handle)["questions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.byte_count(&current), bytes);
    let answered = finish_held(&output, release);
    let Response::Delegation(revised) = pending.join().unwrap() else {
        panic!("blocked publication must commit after answer")
    };
    assert_eq!(revised.questions.len(), 2);
    assert_eq!(reference(&revised).question_version, 2);
    assert_eq!(answered["questions"][0]["answer"]["outcome"], "Applied");
    assert_eq!(received(&current).question, old);
    assert_eq!(count(&current), 1);
    assert!(revised.questions[1].answer.is_none());
    assert_eq!(revised.result_version, 1);
    assert_eq!(revised.execution_attempts, Some(1));
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_accepted_answer_keeps_captured_parent_epoch_after_actual_transfer() {
    let f = OwnedFollowup::new();
    let current = start(&f);
    let original_workflow = super::parent_recovery::workflow(&f);
    f.proxy.arm(ControlRule {
        operation: "AnswerQuestion",
        loss: 2,
        fault: Some(("answer-before-input", 0)),
    });
    let pending = answer(&f, &current, &reference(&current));
    let request = pending["request"].as_str().unwrap();
    let before = durable(&f, &current.handle);
    let private = fs::read(
        current
            .brief
            .parent()
            .unwrap()
            .join(format!("answer-{request}.json")),
    )
    .unwrap();
    let bytes = f.byte_count(&current);
    let second = super::parent_recovery::create_parent(&f);
    let target = super::parent_recovery::target(&f, second.id);
    let moved: WorkflowSnapshot = serde_json::from_value(f.tool("ovrcr_transfer", serde_json::json!({"request": "question-parent-transfer", "parent": target.session, "parent_run": target.run, "binding": target.binding, "producer": target.producer}))).unwrap();
    assert_eq!(moved.authority.epoch, original_workflow.authority.epoch + 1);
    super::parent_recovery::parent_tool(
        &f,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow": moved.authority.workflow}),
    );
    let refused = super::parent_recovery::parent_tool(
        &f,
        second.id,
        "ovrcr_answer_question",
        serde_json::json!({"handle": current.handle, "request": request}),
    );
    assert_eq!(refused["dispatch"], "Refused");
    let observed: AssignmentSnapshot = serde_json::from_value(super::parent_recovery::parent_tool(
        &f,
        second.id,
        "ovrcr_inspect",
        serde_json::json!({"handle": current.handle}),
    ))
    .unwrap();
    assert_eq!(
        observed.identity, current.identity,
        "original question provenance remains historical, never rewritten to the successor"
    );
    assert_eq!(
        observed
            .receipts
            .iter()
            .find(|r| r.request == request)
            .unwrap()
            .outcome,
        DispatchOutcome::Accepted
    );
    let fresh = super::parent_recovery::parent_tool(
        &f,
        second.id,
        "ovrcr_answer_question",
        answer_args(&observed, &reference(&observed)),
    );
    assert_eq!(
        fresh["dispatch"], "Refused",
        "a current parent cannot bypass the suspended original intent"
    );
    let after = durable(&f, &current.handle);
    assert_eq!(after["answers"], before["answers"]);
    assert_eq!(after["receipts"], before["receipts"]);
    assert_eq!(
        fs::read(
            current
                .brief
                .parent()
                .unwrap()
                .join(format!("answer-{request}.json"))
        )
        .unwrap(),
        private
    );
    assert_eq!(f.byte_count(&current), bytes);
    assert_eq!(count(&current), 0);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_accepted_answer_cannot_adopt_returned_child_control_epoch() {
    let f = OwnedFollowup::new();
    let current = start(&f);
    f.proxy.arm(ControlRule {
        operation: "AnswerQuestion",
        loss: 2,
        fault: Some(("answer-before-input", 0)),
    });
    let pending = answer(&f, &current, &reference(&current));
    let request = pending["request"].as_str().unwrap();
    let original = durable(&f, &current.handle)["answers"].clone();
    let bytes = f.byte_count(&current);
    let mut dashboard = super::assignment_control::dashboard(&f, &current);
    let child = current.identity.session.unwrap();
    let human =
        super::assignment_control::transition(&f, &mut dashboard, child, "question-take", true, 1);
    assert_eq!(human.control.owner, ControlOwner::Human);
    let returned = super::assignment_control::transition(
        &f,
        &mut dashboard,
        child,
        "question-return",
        false,
        1,
    );
    let confirmed =
        super::assignment_control::confirm(&f, &returned, "question-return", "question-confirm");
    assert_eq!(confirmed.control.owner, ControlOwner::Controller);
    assert!(confirmed.control.epoch > current.control.epoch);
    let refused = f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": current.handle, "request": request}),
    );
    assert_eq!(refused["dispatch"], "Refused");
    let observed = f.reconcile(&current.handle);
    assert_eq!(
        observed
            .receipts
            .iter()
            .find(|r| r.request == request)
            .unwrap()
            .outcome,
        DispatchOutcome::Accepted
    );
    assert_eq!(durable(&f, &current.handle)["answers"], original);
    assert_eq!(
        answer(&f, &confirmed, &reference(&current))["dispatch"],
        "Refused"
    );
    assert_eq!(f.byte_count(&current), bytes);
    assert_eq!(count(&current), 0);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_binding_and_native_input_replacement_after_admission_fence_final_delivery() {
    for operation in ["native-input", "rebind"] {
        let f = OwnedFollowup::new();
        let current = start(&f);
        let before = durable(&f, &current.handle)["binding"].clone();
        let (output, release) = held_answer(&f, &current);
        let bytes = f.byte_count(&current);
        let original_intent = durable(&f, &current.handle)["answers"].clone();
        command(&f, &current, 1, serde_json::json!({"operation": operation}));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
                panic!("inventory")
            };
            let snapshot = sessions
                .iter()
                .find(|s| Some(s.id) == current.identity.session)
                .unwrap()
                .agent
                .as_ref()
                .unwrap();
            if (operation == "native-input" && !snapshot.input_requests.is_empty())
                || (operation == "rebind"
                    && snapshot.binding.generation > before["generation"].as_u64().unwrap())
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "replacement must reach the authoritative Reporter before release"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let result = finish_held(&output, release);
        let snapshot: AssignmentSnapshot = if operation == "rebind" {
            // Reporter attribution invalidation takes the earlier control fence;
            // no final writer receipt can be invented for this suspended intent.
            assert_eq!(result["dispatch"], "Refused", "{result}");
            assert_eq!(result["transport"], "Unknown", "{result}");
            assert!(
                result["reason"]
                    .as_str()
                    .unwrap()
                    .contains("controller input is fenced")
            );
            let observed = f.reconcile(&current.handle);
            assert_eq!(observed.control.owner, ControlOwner::Uncertain);
            assert_eq!(
                observed.control.barrier,
                Some(ControlBarrier::AttributionChanged)
            );
            assert!(observed.control.epoch > current.control.epoch);
            let receipt = observed
                .receipts
                .iter()
                .find(|r| r.operation == DispatchOperation::AnswerQuestion)
                .unwrap();
            assert_eq!(receipt.outcome, DispatchOutcome::Accepted);
            assert!(receipt.input.is_none());
            let replay = f.tool(
                "ovrcr_answer_question",
                serde_json::json!({"handle": current.handle, "request": receipt.request}),
            );
            assert_eq!(replay["dispatch"], "Refused", "{replay}");
            assert_eq!(f.reconcile(&current.handle).receipts, observed.receipts);
            assert_eq!(durable(&f, &current.handle)["answers"], original_intent);
            observed
        } else {
            let snapshot: AssignmentSnapshot = serde_json::from_value(result.clone())
                .unwrap_or_else(|error| panic!("{operation}: {error}; raw={result}"));
            let receipt = snapshot
                .receipts
                .iter()
                .find(|r| r.operation == DispatchOperation::AnswerQuestion)
                .unwrap();
            assert_eq!(receipt.outcome, DispatchOutcome::Rejected);
            let input = receipt.input.as_ref().unwrap();
            assert_eq!(input.written_bytes, 0);
            assert!(!input.submit_written);
            snapshot
        };
        assert_eq!(f.byte_count(&current), bytes);
        assert_eq!(count(&current), 0);
        assert_eq!(snapshot.result_version, 1);
    }
}

#[test]
fn public_human_decision_requires_declared_explicit_basis_before_any_answer_intent() {
    let f = OwnedFollowup::new();
    let first = start(&f);
    let old = reference(&first);
    let mut human_choice = collect(&f, &first, &old);
    human_choice.question_version += 1;
    human_choice.supersedes = Some(old);
    human_choice.kind = QuestionKind::HumanDecision;
    human_choice.question = "Choose whether the task may include Installation.".into();
    human_choice.context = "The existing assignment only authorizes the Introduction; expanding it requires an explicit human choice.".into();
    let published = command(&f, &first, 1, serde_json::json!({"question": human_choice}));
    let current: AssignmentSnapshot = serde_json::from_value(published["details"].clone()).unwrap();
    let exact = reference(&current);
    let bytes = f.byte_count(&current);
    let refused = answer(&f, &current, &exact);
    assert_eq!(refused["dispatch"], "Refused", "wrong basis: {refused}");
    assert_eq!(f.byte_count(&current), bytes);
    assert_eq!(count(&current), 0);
    let untouched = f.reconcile(&current.handle);
    assert!(
        !untouched
            .receipts
            .iter()
            .any(|r| r.operation == DispatchOperation::AnswerQuestion)
    );
    assert!(
        durable(&f, &current.handle)["answers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // This fixture asserts declared provenance, not an actual person's approval.
    let mut args = answer_args(&current, &exact);
    args["basis"] = "ExplicitHumanDecision".into();
    let applied = f.tool("ovrcr_answer_question", args);
    let request = applied["request"].as_str().unwrap();
    let delivered: AssignmentSnapshot = serde_json::from_value(applied.clone()).unwrap();
    assert_eq!(
        delivered
            .receipts
            .iter()
            .find(|r| r.request == request)
            .unwrap()
            .outcome,
        DispatchOutcome::Applied
    );
    assert_eq!(received(&current).basis, AnswerBasis::ExplicitHumanDecision);
    assert_eq!(count(&current), 1);
    assert_eq!(delivered.result_version, current.result_version);
    let after = f.byte_count(&current);
    let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": current.handle, "request": request}),
    ))
    .unwrap();
    assert_eq!(replay.receipts, delivered.receipts);
    assert_eq!(f.byte_count(&current), after);
    assert_eq!(count(&current), 1);
}

#[test]
fn public_question_exact_answer_is_scoped_delivered_once_and_does_not_advance_result() {
    let f = OwnedFollowup::new();
    let current = start(&f);
    let q = reference(&current);
    let body = collect(&f, &current, &q);
    assert_eq!(
        body.question,
        "QUESTION_BODY_SECRET: Which README section is within the authorized scope?"
    );
    assert_eq!(body.choices, ["Introduction", "Installation"]);
    let answered = answer(&f, &current, &q);
    let request = answered["request"].as_str().unwrap();
    let snapshot: AssignmentSnapshot = serde_json::from_value(answered.clone()).unwrap();
    let delivery_receipt = snapshot
        .receipts
        .iter()
        .find(|receipt| receipt.request == request)
        .expect("answer receipt");
    assert_eq!(
        delivery_receipt.outcome,
        DispatchOutcome::Applied,
        "{snapshot:?}"
    );
    let delivered = received(&snapshot);
    assert_eq!(delivered.question, q);
    assert_eq!(delivered.basis, AnswerBasis::ExistingAuthority);
    assert_eq!(snapshot.identity, current.identity);
    assert_eq!(snapshot.result_version, current.result_version);
    assert_eq!(snapshot.execution_attempts, Some(1));
    assert_eq!(snapshot.acceptance, "pending");
    let receipt = snapshot
        .receipts
        .iter()
        .find(|r| r.request == request)
        .unwrap();
    assert_eq!(receipt.operation, DispatchOperation::AnswerQuestion);
    assert_eq!(receipt.outcome, DispatchOutcome::Applied);
    assert!(receipt.input.as_ref().unwrap().submit_written);
    let before = f.byte_count(&snapshot);
    let duplicate: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": current.handle, "request": request}),
    ))
    .unwrap();
    assert_eq!(duplicate.receipts, snapshot.receipts);
    assert_eq!(f.byte_count(&snapshot), before);
    assert_eq!(count(&snapshot), 1);
    assert_eq!(
        f.input_count(&snapshot),
        1,
        "only initial input counts as model assignment dispatch"
    );
    assert_eq!(f.child_count(), 1);
    let again = answer(&f, &current, &q);
    assert_eq!(
        again["dispatch"], "Refused",
        "fresh request cannot answer an already answered question: {again}"
    );
    let mut changed = original_answer(&f);
    let DelegationRequest::AnswerQuestion { answer, .. } = &mut changed else {
        unreachable!()
    };
    answer.answer.push_str(" changed");
    assert!(matches!(
        f.live.request(Request::Delegation(Box::new(changed))),
        Response::Error { .. }
    ));
    assert_eq!(f.byte_count(&snapshot), before);
    assert_private_metadata(&f);
}

#[test]
fn public_question_supersession_and_old_applied_replay_never_answer_the_new_question() {
    let f = OwnedFollowup::new();
    let first = start(&f);
    let old = reference(&first);
    let original = answer(&f, &first, &old);
    received(&first);
    let mut revised = collect(&f, &first, &old);
    revised.question_version = 2;
    revised.supersedes = Some(old.clone());
    revised.question = "QUESTION_BODY_SECRET: Clarify which Introduction subsection.".into();
    let published = command(&f, &first, 1, serde_json::json!({"question": revised}));
    let latest: AssignmentSnapshot = serde_json::from_value(published["details"].clone()).unwrap();
    let new = reference(&latest);
    assert_eq!(new.question_version, 2);
    assert_eq!(latest.questions.len(), 2);
    assert!(!latest.questions[0].current);
    let before = f.byte_count(&latest);
    let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_answer_question",
        serde_json::json!({"handle": first.handle, "request": original["request"]}),
    ))
    .unwrap();
    assert_eq!(reference(&replay), new);
    assert!(replay.questions[1].answer.is_none());
    assert_eq!(f.byte_count(&latest), before);
    assert_eq!(count(&latest), 1);
    let stale = answer(&f, &latest, &old);
    assert_eq!(stale["dispatch"], "Refused");
    let exact = answer(&f, &latest, &new);
    assert_eq!(exact["questions"][1]["answer"]["outcome"], "Applied");
    let deadline = Instant::now() + Duration::from_secs(20);
    while count(&latest) != 2 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(received(&latest).question, new);
    assert_eq!(
        collect(&f, &latest, &old).question_version,
        1,
        "historical evidence remains separately readable"
    );
    assert_eq!(latest.result_version, 1);
    assert_private_metadata(&f);
}

#[test]
fn public_question_requires_child_role_exact_predecessor_and_one_unanswered_current_question() {
    let f = OwnedFollowup::new();
    let first = start(&f);
    let q = reference(&first);
    let mut body = collect(&f, &first, &q);
    let parent = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_publish_question",
        serde_json::json!({"handle": first.handle, "question": body}),
        &f.live.root.path().join("parent-question-refusal.json"),
    );
    assert!(
        parent.get("error").is_some(),
        "parent capability cannot impersonate allocated child"
    );
    body.question_id = "other-question".into();
    let other = command(&f, &first, 1, serde_json::json!({"question": body}));
    assert!(
        other.get("error").is_some(),
        "unanswered evidence is never silently evicted: {other}"
    );
    body = collect(&f, &first, &q);
    body.question_version = 2;
    let missing = command(&f, &first, 2, serde_json::json!({"question": body}));
    assert!(missing.get("error").is_some());
    body.supersedes = Some(q.clone());
    body.supersedes.as_mut().unwrap().digest = "0".repeat(64);
    let foreign = command(&f, &first, 3, serde_json::json!({"question": body}));
    assert!(foreign.get("error").is_some());
    let second = start(&f);
    body = collect(&f, &first, &q);
    let sibling = command(&f, &second, 1, serde_json::json!({"question": body}));
    assert!(sibling.get("error").is_some());
    assert_eq!(reference(&f.reconcile(&first.handle)), q);
    assert_eq!(count(&first), 0);
}

#[test]
fn public_question_lost_answer_request_or_reply_recovers_the_immutable_original_once() {
    for loss in [0, 1] {
        let f = OwnedFollowup::new();
        let current = start(&f);
        let q = reference(&current);
        f.proxy.arm(ControlRule {
            operation: "AnswerQuestion",
            loss,
            fault: None,
        });
        let lost = answer(&f, &current, &q);
        assert_eq!(lost["dispatch"], "Unknown", "{lost}");
        assert!(lost.get("receipts").is_none());
        let recovered: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_answer_question",
            serde_json::json!({"handle": current.handle, "request": lost["request"]}),
        ))
        .unwrap();
        let delivered = received(&recovered);
        assert_eq!(delivered.question, q);
        assert_eq!(count(&recovered), 1);
        assert_eq!(recovered.result_version, 1);
        assert_eq!(recovered.execution_attempts, Some(1));
        let before = f.byte_count(&recovered);
        f.tool(
            "ovrcr_answer_question",
            serde_json::json!({"handle": current.handle, "request": lost["request"]}),
        );
        assert_eq!(f.byte_count(&recovered), before);
        assert_eq!(f.child_count(), 1);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_partial_unknown_and_pending_answers_share_the_original_input_barrier() {
    for (point, limit, expected) in [
        ("input", 7, DispatchOutcome::Partial),
        ("input", usize::MAX, DispatchOutcome::Unknown),
        ("answer-after-effect", 0, DispatchOutcome::Unknown),
        ("answer-before-input", 0, DispatchOutcome::Accepted),
    ] {
        let f = OwnedFollowup::new();
        let current = start(&f);
        let q = reference(&current);
        f.proxy.arm(ControlRule {
            operation: "AnswerQuestion",
            loss: 2,
            fault: Some((point, limit)),
        });
        let uncertain = answer(&f, &current, &q);
        let snapshot: AssignmentSnapshot = serde_json::from_value(uncertain.clone()).unwrap();
        let receipt = snapshot
            .receipts
            .iter()
            .find(|r| r.operation == DispatchOperation::AnswerQuestion)
            .unwrap();
        assert_eq!(receipt.outcome, expected);
        if expected == DispatchOutcome::Partial {
            assert_eq!(receipt.input.as_ref().unwrap().written_bytes, 7);
        }
        if point == "answer-after-effect" || limit == usize::MAX {
            received(&snapshot);
        }
        let before = f.byte_count(&snapshot);
        let fresh = answer(&f, &snapshot, &q);
        assert_eq!(fresh["dispatch"], "Refused");
        let followup = super::followups::followup(&f, &snapshot, None);
        assert_eq!(
            followup["dispatch"], "Refused",
            "followup cannot bypass the same unresolved-input barrier"
        );
        assert_eq!(f.byte_count(&snapshot), before);
        if expected != DispatchOutcome::Accepted {
            let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
                "ovrcr_answer_question",
                serde_json::json!({"handle": current.handle, "request": receipt.request}),
            ))
            .unwrap();
            assert_eq!(replay.receipts, snapshot.receipts);
            assert_eq!(f.byte_count(&snapshot), before);
        } else {
            let recovered: AssignmentSnapshot = serde_json::from_value(f.tool(
                "ovrcr_answer_question",
                serde_json::json!({"handle": current.handle, "request": receipt.request}),
            ))
            .unwrap();
            received(&recovered);
            assert_eq!(count(&recovered), 1);
        }
        assert_eq!(snapshot.result_version, 1);
        assert_eq!(snapshot.execution_attempts, Some(1));
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_question_intent_before_link_recovers_only_the_original_authenticated_body() {
    let f = OwnedFollowup::new();
    f.proxy.arm(ControlRule {
        operation: "PublishQuestion",
        loss: 2,
        fault: Some(("question-before-link", 0)),
    });
    let current = start(&f);
    let q = reference(&current);
    assert_eq!(
        current.questions[0].issue,
        Some(PublicationIssue::Unavailable)
    );
    let missing = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_collect_question",
        serde_json::json!({"handle": current.handle, "reference": q}),
        &f.live.root.path().join("missing-question.json"),
    );
    assert!(missing.get("error").is_some());
    let original = f
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find_map(|r| match r {
            DelegationRequest::PublishQuestion { question, .. } => Some(question.clone()),
            _ => None,
        })
        .unwrap();
    let mut changed = original.clone();
    changed.question.push_str(" changed");
    let refused = command(&f, &current, 1, serde_json::json!({"question": changed}));
    assert!(refused.get("error").is_some());
    let recovered = command(&f, &current, 2, serde_json::json!({"question": original}));
    let snapshot: AssignmentSnapshot =
        serde_json::from_value(recovered["details"].clone()).unwrap();
    assert_eq!(snapshot.questions.len(), 1);
    assert!(snapshot.questions[0].issue.is_none());
    assert_eq!(collect(&f, &snapshot, &q), original);
    assert_eq!(count(&snapshot), 0);
}

#[test]
fn public_question_late_old_result_or_contract_publication_and_answers_remain_historical() {
    for revise_contract in [false, true] {
        let f = OwnedFollowup::new();
        let first = start(&f);
        let q = reference(&first);
        let old_body = collect(&f, &first, &q);
        let original = answer(&f, &first, &q);
        received(&first);
        let completed = super::followups::complete(&f, &first);
        let contract = revise_contract.then(|| {
            let mut contract: ResearchContract = serde_json::from_slice(
                &fs::read(first.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
            )
            .unwrap();
            contract.scope = "QUESTION_FILE_ONLY read the revised README scope".into();
            contract
        });
        let revised: AssignmentSnapshot =
            serde_json::from_value(super::followups::followup(&f, &completed, contract)).unwrap();
        assert_eq!(
            revised.identity.contract,
            if revise_contract { 2 } else { 1 }
        );
        wait_file(&revised.brief.with_extension("json.question-published"));
        let before = f.byte_count(&revised);
        let late = command(&f, &first, 1, serde_json::json!({"question": old_body}));
        assert!(late.get("error").is_some());
        let replay: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_answer_question",
            serde_json::json!({"handle": first.handle, "request": original["request"]}),
        ))
        .unwrap();
        assert_eq!(replay.result_version, 2);
        assert_eq!(reference(&replay).result_version, 2);
        assert!(
            replay
                .questions
                .iter()
                .find(|state| state.current)
                .unwrap()
                .answer
                .is_none()
        );
        assert_eq!(f.byte_count(&revised), before);
        assert_eq!(collect(&f, &revised, &q), old_body);
        let stale = answer(&f, &revised, &q);
        assert_eq!(stale["dispatch"], "Refused");
    }
}

#[test]
fn public_question_known_native_prompt_and_changed_binding_refuse_structured_answer_bytes() {
    for operation in ["native-input", "rebind"] {
        let f = OwnedFollowup::new();
        let current = start(&f);
        let q = reference(&current);
        let action = command(&f, &current, 1, serde_json::json!({"operation": operation}));
        assert!(action.get("error").is_none());
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let observed: DelegationObservationBatch = serde_json::from_value(f.tool(
                "ovrcr_wait",
                serde_json::json!({"handles": [current.handle], "wait_ms": 0}),
            ))
            .unwrap();
            let gap = observed.observations[0].native_question_gap;
            if (operation == "native-input"
                && gap == Some(NativeQuestionGap::IncompletePromptObservation))
                || (operation == "rebind"
                    && matches!(
                        gap,
                        Some(
                            NativeQuestionGap::AttributionChanged
                                | NativeQuestionGap::UnavailableBinding
                        )
                    ))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "authoritative reporter change not observed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let before = f.byte_count(&current);
        let refused = answer(&f, &current, &q);
        assert_eq!(refused["dispatch"], "Refused", "{operation}: {refused}");
        assert_eq!(f.byte_count(&current), before);
        assert_eq!(count(&current), 0);
    }
}
