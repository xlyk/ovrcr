//! Common two-tool adapter through the production CLI and isolated binary Server.
use super::followups::{OwnedFollowup, complete, followup};
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::process::{Child, Stdio};

// Own only this adapter process. Live/OwnedFollowup own Server and child PTYs.
struct Adapter(Child);
impl Drop for Adapter {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn fixture() -> (OwnedFollowup, AssignmentSnapshot, DelegationAuth) {
    let f = OwnedFollowup::new();
    // The ordinary managed Pi child makes a rejected stale publication. Capture
    // its scoped auth in the existing proxy's memory; never print/persist it.
    let started: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_start", json!({
        "project":PROJECT,"workspace":f.workspace,"scope":"STALE_RESULT: Read README.",
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["README citation"]
    }))).unwrap();
    let missing = complete(&f, &started);
    assert!(missing.submission.is_none());
    let auth = f
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find_map(|request| match request {
            DelegationRequest::Publish { auth, handle, .. } if handle == &started.handle => {
                Some(auth.clone())
            }
            _ => None,
        })
        .expect("real managed child publication authority");
    assert_eq!(Some(auth.session), started.identity.session);
    (f, missing, auth)
}

fn submission(snapshot: &AssignmentSnapshot) -> Value {
    json!({"schema":1,"identity":snapshot.identity,"version":snapshot.result_version,
        "outcome":"success","summary":"Native adapter fixture read README.",
        "evidence":["README:1 controlled fixture"],"unresolved":[]})
}

fn assert_publish_refusal(
    f: &RecoveryFixture,
    auth: &DelegationAuth,
    handle: &str,
    body: &Value,
    expected_code: ErrorCode,
    expected_message: &str,
) {
    let submission = serde_json::from_value(body.clone()).unwrap();
    let response = f
        .live
        .request(Request::Delegation(Box::new(DelegationRequest::Publish {
            auth: auth.clone(),
            handle: handle.into(),
            submission,
        })));
    let Response::Error { code, message } = response else {
        panic!("expected pre-effect publication refusal, got {response:?}");
    };
    assert_eq!(code, expected_code);
    assert_eq!(message, expected_message);
}

fn question(snapshot: &AssignmentSnapshot) -> Value {
    json!({"schema":1,"identity":snapshot.identity,"result_version":snapshot.result_version,
        "question_id":"section","question_version":1,"supersedes":null,"kind":"Clarification",
        "question":"Which README section is within scope?","context":"Introduction and Installation.",
        "choices":["Introduction","Installation"]})
}

fn tool_call(name: &str, handle: &str, body: Value) -> Value {
    let field = if name == "ovrcr_publish" {
        "submission"
    } else {
        "question"
    };
    let mut arguments = json!({"handle":handle});
    arguments[field] = body;
    json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":name,"arguments":arguments}})
}

fn run(f: &RecoveryFixture, auth: Option<&DelegationAuth>, calls: Vec<Value>) -> Vec<Value> {
    run_on(f, auth, calls, &f.proxy.path)
}

fn run_on(
    f: &RecoveryFixture,
    auth: Option<&DelegationAuth>,
    calls: Vec<Value>,
    socket: &Path,
) -> Vec<Value> {
    let mut frames = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"ovrcr-regression","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ];
    frames.extend(calls);
    let mut input = Vec::new();
    for frame in frames {
        input.extend(serde_json::to_vec(&frame).unwrap());
        input.push(b'\n');
    }
    run_bytes_on(f, auth, &input, socket)
}

fn run_bytes(f: &RecoveryFixture, auth: Option<&DelegationAuth>, input: &[u8]) -> Vec<Value> {
    run_bytes_on(f, auth, input, &f.proxy.path)
}

fn run_bytes_on(
    f: &RecoveryFixture,
    auth: Option<&DelegationAuth>,
    input: &[u8],
    socket: &Path,
) -> Vec<Value> {
    let sequence = f.files.get();
    f.files.set(sequence + 1);
    let stdout = f.live.root.path().join(format!("mcp-{sequence}.stdout"));
    let stderr = f.live.root.path().join(format!("mcp-{sequence}.stderr"));
    let mut command = f.live.command();
    command
        .arg("delegation-publisher")
        .env_remove("OVRCR_DELEGATION_SESSION")
        .env_remove("OVRCR_DELEGATION_TOKEN")
        .env_remove("OVRCR_DELEGATION_SOCKET")
        .env_remove("OVRCR_DELEGATION_HOME")
        .stdin(Stdio::piped())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap());
    if let Some(auth) = auth {
        command
            .env("OVRCR_DELEGATION_SESSION", auth.session.0.to_string())
            .env("OVRCR_DELEGATION_TOKEN", &auth.capability)
            .env("OVRCR_DELEGATION_SOCKET", socket)
            .env("OVRCR_DELEGATION_HOME", &f.live.config);
    }
    let mut adapter = Adapter(command.spawn().unwrap());
    let mut stdin = adapter.0.stdin.take().unwrap();
    if let Err(error) = stdin.write_all(input) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "adapter stdin failed"
        );
    }
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = adapter.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "owned adapter did not exit after stdin EOF"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        status.success(),
        "adapter exit failed; see private fixture stderr"
    );
    assert!(
        fs::read(&stderr).unwrap().is_empty(),
        "adapter should not log task bodies"
    );
    let output = fs::read_to_string(stdout).unwrap();
    if let Some(auth) = auth {
        assert!(
            !output.contains(&auth.capability),
            "capability leaked to MCP response"
        );
    }
    output
        .lines()
        .map(|line| {
            let value: Value =
                serde_json::from_str(line).expect("every stdout line is a protocol message");
            assert_eq!(value["jsonrpc"], "2.0");
            value
        })
        .collect()
}

fn result(replies: &[Value]) -> &Value {
    &replies.last().unwrap()["result"]["structuredContent"]
}

fn count(f: &RecoveryFixture, operation: &str) -> usize {
    f.proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| {
            matches!(
                (operation, *request),
                ("Publish", DelegationRequest::Publish { .. })
                    | ("PublishQuestion", DelegationRequest::PublishQuestion { .. })
            )
        })
        .count()
}

#[test]
fn native_publisher_cli_first_result_duplicate_question_and_same_child_version_two() {
    let (f, first, auth) = fixture();
    let body = submission(&first);
    let initial = run(
        &f,
        Some(&auth),
        vec![tool_call("ovrcr_publish", &first.handle, body.clone())],
    );
    assert_eq!(result(&initial)["status"], "published");
    let current: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":first.handle}))).unwrap();
    assert_eq!(
        current.submission.as_ref().unwrap().summary,
        "Native adapter fixture read README."
    );
    assert_eq!(current.acceptance, "pending");
    let bytes = fs::read(&current.result_path).unwrap();
    assert_eq!(
        result(&initial)["publication"]["digest"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    let duplicate = run(
        &f,
        Some(&auth),
        vec![tool_call("ovrcr_publish", &first.handle, body)],
    );
    assert_eq!(result(&duplicate), result(&initial));
    assert_eq!(fs::read(&current.result_path).unwrap(), bytes);
    let second: AssignmentSnapshot = serde_json::from_value(followup(&f, &current, None)).unwrap();
    wait_file(&second.brief.with_extension("json.published"));
    let second_result = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish",
            &second.handle,
            submission(&second),
        )],
    );
    assert_eq!(result(&second_result)["status"], "published");
    assert_eq!(result(&second_result)["publication"]["version"], 2);
    assert_eq!(second.identity, first.identity);
    assert_eq!(second.conversation, first.conversation);
    let asked = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish_question",
            &second.handle,
            question(&second),
        )],
    );
    assert_eq!(result(&asked)["status"], "published");
    let reference = result(&asked)["publication"].clone();
    let question_bytes = fs::read(
        second
            .brief
            .parent()
            .unwrap()
            .join("question-r2-section-v1.json"),
    )
    .unwrap();
    assert_eq!(
        reference["digest"],
        format!("{:x}", Sha256::digest(question_bytes))
    );
    let listed: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":second.handle}))).unwrap();
    assert_eq!(
        serde_json::to_value(listed.questions[0].reference.publication()).unwrap(),
        reference
    );
    assert_eq!(listed.acceptance, "pending");
}

#[test]
fn native_publisher_cli_lost_reply_reconciles_same_payload_and_explicit_refusal_stays_distinct() {
    let (f, current, auth) = fixture();
    f.proxy.arm(ControlRule {
        operation: "Publish",
        loss: 1,
        fault: None,
    });
    let body = submission(&current);
    let lost = run(
        &f,
        Some(&auth),
        vec![tool_call("ovrcr_publish", &current.handle, body.clone())],
    );
    assert_eq!(result(&lost)["status"], "unknown");
    let found: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":current.handle}))).unwrap();
    assert!(found.submission.is_some());
    let original = fs::read(&found.result_path).unwrap();
    let retry = run(
        &f,
        Some(&auth),
        vec![tool_call("ovrcr_publish", &current.handle, body.clone())],
    );
    assert_eq!(result(&retry)["status"], "published");
    assert_eq!(fs::read(&found.result_path).unwrap(), original);
    let mut changed = body;
    changed["summary"] = "This different payload may not overwrite committed evidence.".into();
    assert_publish_refusal(
        &f,
        &auth,
        &current.handle,
        &changed,
        ErrorCode::Conflict,
        "submission version is already published",
    );
    let refused = run(
        &f,
        Some(&auth),
        vec![tool_call("ovrcr_publish", &current.handle, changed)],
    );
    assert_eq!(result(&refused)["status"], "refused");
    assert_eq!(result(&refused)["code"], "Conflict");
    assert_eq!(refused.last().unwrap()["result"]["isError"], true);
    assert_eq!(fs::read(&found.result_path).unwrap(), original);

    let mut wrong = auth.clone();
    wrong.capability = if auth.capability == "0".repeat(64) {
        "1".repeat(64)
    } else {
        "0".repeat(64)
    };
    let body = submission(&current);
    assert_publish_refusal(
        &f,
        &wrong,
        &current.handle,
        &body,
        ErrorCode::Conflict,
        "only the assigned child may publish with its current capability",
    );
    let unauthorized = run(
        &f,
        Some(&wrong),
        vec![tool_call("ovrcr_publish", &current.handle, body)],
    );
    assert_eq!(result(&unauthorized)["status"], "refused");
    assert_eq!(result(&unauthorized)["code"], "Conflict");
    assert_eq!(fs::read(&found.result_path).unwrap(), original);
}

#[test]
fn native_publisher_cli_auth_catalog_malformed_and_oversized_input_have_no_server_effect() {
    let (f, current, auth) = fixture();
    let before = count(&f, "Publish");
    let missing = run(
        &f,
        None,
        vec![tool_call(
            "ovrcr_publish",
            &current.handle,
            submission(&current),
        )],
    );
    assert_eq!(result(&missing)["status"], "refused");
    let catalog = run(
        &f,
        Some(&auth),
        vec![json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})],
    );
    let tools = catalog.last().unwrap()["result"]["tools"]
        .as_array()
        .unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["ovrcr_publish", "ovrcr_publish_question"]
    );
    let denied = run(
        &f,
        Some(&auth),
        vec![
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ovrcr_accept","arguments":{"handle":current.handle}}}),
        ],
    );
    assert_eq!(denied.last().unwrap()["error"]["code"], -32602);
    let mut forged = tool_call("ovrcr_publish", &current.handle, submission(&current));
    forged["params"]["arguments"]["auth"] =
        json!({"session":auth.session,"capability":"caller-auth-is-forbidden"});
    assert_eq!(
        result(&run(&f, Some(&auth), vec![forged]))["status"],
        "refused"
    );
    for invalid in [b"{broken\n".to_vec(), vec![b'x'; 524_289]] {
        let replies = run_bytes(&f, Some(&auth), &invalid);
        assert_eq!(replies.last().unwrap()["error"]["code"], -32700);
    }
    assert_eq!(count(&f, "Publish"), before);
}

#[test]
fn native_publisher_cli_foreign_handle_stale_version_and_retired_run_cannot_publish() {
    let (f, first, auth) = fixture();
    let before = f.reconcile(&first.handle);
    let mut foreign_identity = submission(&first);
    foreign_identity["identity"]["contract"] = json!(first.identity.contract + 1);
    assert_publish_refusal(
        &f,
        &auth,
        &first.handle,
        &foreign_identity,
        ErrorCode::Conflict,
        "foreign or stale submission identity",
    );
    let unchanged = f.reconcile(&first.handle);
    assert_eq!(unchanged.identity, before.identity);
    assert_eq!(unchanged.receipts, before.receipts);
    assert!(unchanged.submission.is_none());
    assert!(unchanged.publication.is_none());
    assert!(!first.result_path.exists());
    assert_publish_refusal(
        &f,
        &auth,
        "foreign-handle",
        &submission(&first),
        ErrorCode::NotFound,
        "assignment not found",
    );
    assert!(!first.result_path.exists());
    let foreign = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish",
            "foreign-handle",
            submission(&first),
        )],
    );
    assert_eq!(result(&foreign)["status"], "refused");
    assert_eq!(result(&foreign)["code"], "NotFound");
    assert!(!first.result_path.exists());
    let published = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish",
            &first.handle,
            submission(&first),
        )],
    );
    assert_eq!(result(&published)["status"], "published");
    let original = fs::read(&first.result_path).unwrap();
    let second: AssignmentSnapshot = serde_json::from_value(followup(&f, &first, None)).unwrap();
    wait_file(&second.brief.with_extension("json.published"));
    assert_publish_refusal(
        &f,
        &auth,
        &first.handle,
        &submission(&first),
        ErrorCode::Conflict,
        "submission belongs to a stale result version",
    );
    let stale = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish",
            &first.handle,
            submission(&first),
        )],
    );
    assert_eq!(result(&stale)["status"], "refused");
    assert_eq!(result(&stale)["code"], "Conflict");
    assert_eq!(fs::read(&first.result_path).unwrap(), original);
    assert!(!second.result_path.exists());
    for group in f.live.session_groups() {
        f.live.own_group(group);
    }
    assert_eq!(
        f.live.request(Request::CloseTerminal {
            session: auth.session,
            expected_run: first.identity.run.unwrap()
        }),
        Response::Ok
    );
    let retired = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish",
            &second.handle,
            submission(&second),
        )],
    );
    assert_eq!(result(&retired)["status"], "refused");
    assert_eq!(result(&retired)["code"], "Conflict");
    assert!(f.reconcile(&second.handle).submission.is_none());
    assert!(!second.result_path.exists());
    assert_eq!(fs::read(&first.result_path).unwrap(), original);
}

#[test]
fn native_publisher_cli_lost_question_reply_and_duplicate_keep_one_exact_reference() {
    let (f, current, auth) = fixture();
    f.proxy.arm(ControlRule {
        operation: "PublishQuestion",
        loss: 1,
        fault: None,
    });
    let body = question(&current);
    let lost = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish_question",
            &current.handle,
            body.clone(),
        )],
    );
    assert_eq!(result(&lost)["status"], "unknown");
    let found: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":current.handle}))).unwrap();
    assert_eq!(found.questions.len(), 1);
    let reference = serde_json::to_value(found.questions[0].reference.publication()).unwrap();
    for _ in 0..2 {
        let retry = run(
            &f,
            Some(&auth),
            vec![tool_call(
                "ovrcr_publish_question",
                &current.handle,
                body.clone(),
            )],
        );
        assert_eq!(result(&retry)["status"], "published");
        assert_eq!(result(&retry)["publication"], reference);
    }
    let unchanged: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":current.handle}))).unwrap();
    assert_eq!(unchanged.questions.len(), 1);
    assert_eq!(count(&f, "PublishQuestion"), 3);
}

#[test]
fn native_publisher_cli_replaced_conversation_rejects_previously_authorized_child() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_start", json!({
        "project":PROJECT,"workspace":f.workspace,"scope":"REBIND_AFTER_RESULT read README.",
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["README citation"]
    }))).unwrap();
    let current = complete(&f, &started);
    let auth = f
        .proxy
        .requests
        .lock()
        .unwrap()
        .iter()
        .find_map(|request| match request {
            DelegationRequest::Publish { auth, handle, .. } if handle == &current.handle => {
                Some(auth.clone())
            }
            _ => None,
        })
        .unwrap();
    let original = fs::read(&current.result_path).unwrap();
    fs::write(
        current.brief.with_extension("json.rebind"),
        "replace owned fixture conversation",
    )
    .unwrap();
    wait_file(&current.brief.with_extension("json.rebound"));
    let refused = run(
        &f,
        Some(&auth),
        vec![tool_call(
            "ovrcr_publish_question",
            &current.handle,
            question(&current),
        )],
    );
    assert_eq!(result(&refused)["status"], "refused");
    assert_eq!(fs::read(&current.result_path).unwrap(), original);
    assert!(f.reconcile(&current.handle).questions.is_empty());
}

#[test]
fn native_publisher_cli_internal_or_wrong_digest_reply_cannot_certify_real_server_publication() {
    for scenario in ["internal", "submission-digest", "question-digest"] {
        let (f, current, auth) = fixture();
        let socket = f.live.root.path().join("reply-boundary.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let upstream = f.live.socket.clone();
        let expected_obligation_revision = current.obligation_revision;
        let expected_archive_receipts = current.archive_receipts.clone();
        // Forward the real effect, then alter only its reply at the transport
        // boundary. This is an injected reply, not a synthetic publication.
        std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(20);
                let mut downstream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "adapter did not reach reply boundary"
                            );
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("reply boundary failed: {error}"),
                    }
                };
                // The listener is nonblocking; make the accepted stream's mode
                // explicit before blocking framed I/O on every platform.
                downstream.set_nonblocking(false).unwrap();
                downstream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                downstream
                    .set_write_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                exchange_preamble(&mut downstream).unwrap();
                let message: ClientMessage = read_frame(&mut downstream).unwrap();
                let mut server = connect_server(upstream).unwrap();
                server
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                server
                    .set_write_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut response =
                    client::request(&mut server, message.request_id, message.request).unwrap();
                assert!(
                    matches!(response, Response::Delegation(_)),
                    "real publication must commit first"
                );
                if let Response::Delegation(snapshot) = &response {
                    assert!(snapshot.submission.is_none());
                    assert!(snapshot.historical_submission.is_none());
                    assert!(snapshot.acceptance_ref.is_none());
                    assert_eq!(snapshot.acceptance, "pending");
                    // Publication receipts must preserve the accepted delivery /
                    // retention snapshot; they add no archive or obligation effect.
                    assert_eq!(snapshot.obligation_revision, expected_obligation_revision);
                    assert_eq!(snapshot.archive_receipts, expected_archive_receipts);
                    if scenario != "question-digest" {
                        // Regression: before the Publish response fix, this is None.
                        let publication = snapshot.publication.as_ref().unwrap();
                        assert!(!publication.handled);
                        assert_eq!(publication.identity.kind, PublicationKind::Submission);
                    }
                }
                if scenario == "internal" {
                    response = Response::Error {
                        code: ErrorCode::Internal,
                        message: "injected post-effect persistence uncertainty".into(),
                    };
                } else if let Response::Delegation(snapshot) = &mut response {
                    if scenario == "submission-digest" {
                        snapshot.publication.as_mut().unwrap().identity.digest = "0".repeat(64);
                    } else {
                        snapshot.questions.last_mut().unwrap().reference.digest = "0".repeat(64);
                    }
                }
                write_frame(
                    &mut downstream,
                    &ServerMessage::Response {
                        request_id: message.request_id,
                        response,
                    },
                )
                .unwrap();
            });
            let (name, body) = if scenario == "question-digest" {
                ("ovrcr_publish_question", question(&current))
            } else {
                ("ovrcr_publish", submission(&current))
            };
            let replies = run_on(
                &f,
                Some(&auth),
                vec![tool_call(name, &current.handle, body)],
                &socket,
            );
            worker.join().unwrap();
            assert_eq!(result(&replies)["status"], "unknown", "{scenario}");
            assert_eq!(replies.last().unwrap()["result"]["isError"], true);
            let actual: AssignmentSnapshot =
                serde_json::from_value(f.tool("ovrcr_collect", json!({"handle":current.handle})))
                    .unwrap();
            if scenario == "question-digest" {
                assert_eq!(actual.questions.len(), 1);
                assert_ne!(actual.questions[0].reference.digest, "0".repeat(64));
            } else {
                assert!(actual.submission.is_some());
                assert_ne!(actual.publication.unwrap().identity.digest, "0".repeat(64));
            }
        });
    }
}

#[test]
fn native_publisher_cli_corrupt_bytes_stay_uncertain_and_missing_bytes_republish_identically() {
    for scenario in ["corrupt", "missing"] {
        let (f, current, auth) = fixture();
        let body = submission(&current);
        let first = run(
            &f,
            Some(&auth),
            vec![tool_call("ovrcr_publish", &current.handle, body.clone())],
        );
        assert_eq!(result(&first)["status"], "published");
        let original = fs::read(&current.result_path).unwrap();
        if scenario == "corrupt" {
            fs::write(&current.result_path, b"{corrupt").unwrap();
        } else {
            fs::remove_file(&current.result_path).unwrap();
        }
        let retry = run(
            &f,
            Some(&auth),
            vec![tool_call("ovrcr_publish", &current.handle, body)],
        );
        if scenario == "corrupt" {
            assert!(matches!(
                result(&retry)["status"].as_str(),
                Some("unknown" | "refused")
            ));
            assert_eq!(retry.last().unwrap()["result"]["isError"], true);
            assert_eq!(fs::read(&current.result_path).unwrap(), b"{corrupt");
        } else {
            // Existing publication safely restores identical absent bytes.
            assert_eq!(result(&retry), result(&first));
            assert_eq!(fs::read(&current.result_path).unwrap(), original);
        }
    }
}
