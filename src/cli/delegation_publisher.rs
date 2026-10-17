//! Two native-child MCP tools. The Server owns authorization and immutable files.
//! Native clients use `ovrcr delegation-publisher` over bounded stdio JSON-RPC.
use super::{AppResult, RuntimeError};
use ovrcr::protocol::{
    AssignmentIdentity, AssignmentQuestion, DelegationAuth, DelegationRequest, PublicationIdentity,
    PublicationKind, Request, ResearchSubmission, Response, SessionId, client, exchange_preamble,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufRead, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

const VERSION: &str = "2025-11-25";
const MAX_LINE: usize = 524_288;
const MAX_OUTPUT: usize = 65_536;
const SUBMISSION: &str = "ovrcr_publish";
const QUESTION: &str = "ovrcr_publish_question";

struct Authority {
    auth: DelegationAuth,
    socket: PathBuf,
}

impl Authority {
    fn load(mut get: impl FnMut(&str) -> Option<String>) -> Result<Self, &'static str> {
        let session = get("OVRCR_DELEGATION_SESSION").ok_or("managed child authority missing")?;
        let id = session
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0 && id.to_string() == session)
            .ok_or("invalid managed child session")?;
        let capability = get("OVRCR_DELEGATION_TOKEN")
            .filter(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            })
            .ok_or("invalid managed child capability")?;
        let socket = get("OVRCR_DELEGATION_SOCKET")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("managed socket must be absolute")?;
        get("OVRCR_DELEGATION_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("managed instance directory must be absolute")?;
        Ok(Self {
            auth: DelegationAuth {
                session: SessionId(id),
                capability,
                source: None,
                controller: None,
            },
            socket,
        })
    }
}

#[derive(Default)]
enum Phase {
    #[default]
    New,
    Initializing,
    Ready,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    name: String,
    arguments: Value,
    #[serde(default, rename = "_meta")]
    _meta: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmissionArgs {
    handle: String,
    submission: ResearchSubmission,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionArgs {
    handle: String,
    question: AssignmentQuestion,
}

enum Outcome {
    Published(PublicationIdentity),
    Refused(&'static str),
    ServerRefused(ovrcr::protocol::ErrorCode),
    Unknown,
}

fn tool_result(outcome: Outcome) -> Value {
    let (failed, value) = match outcome {
        Outcome::Published(publication) => (
            false,
            json!({"status":"published","publication":publication,
            "meaning":"Immutable publication confirmed; this is not acceptance or provider settlement."}),
        ),
        Outcome::Refused(reason) => (
            true,
            json!({"status":"refused","reason":reason,"sent":false}),
        ),
        Outcome::ServerRefused(code) => (
            true,
            json!({"status":"refused","code":code,
            "reason":"The Server returned a rejection; this is not rollback proof. Reconcile before replacement; any replay must retain the identical identity, version and payload."}),
        ),
        Outcome::Unknown => (
            true,
            json!({"status":"unknown",
            "reason":"Publication outcome is uncertain. Reconcile the same assignment/version; any retry must retain the identical identity, version and payload."}),
        ),
    };
    json!({"isError":failed,"content":[{"type":"text","text":value.to_string()}],"structuredContent":value})
}

fn forward(authority: &Authority, request: DelegationRequest) -> Outcome {
    let Some(expected) = expected_publication(&request) else {
        return Outcome::Refused("only serializable child publication is supported");
    };
    // Bounds precede the preamble; no request is sent until that handshake succeeds.
    let Ok(mut stream) = UnixStream::connect(&authority.socket) else {
        return Outcome::Refused("managed Server is unavailable; no publication sent");
    };
    let timeout = Some(Duration::from_secs(10));
    if stream.set_read_timeout(timeout).is_err()
        || stream.set_write_timeout(timeout).is_err()
        || exchange_preamble(&mut stream).is_err()
    {
        return Outcome::Refused("managed Server handshake failed; no publication sent");
    }
    match client::request(&mut stream, 1, Request::Delegation(Box::new(request))) {
        Ok(response) => response_outcome(&expected, response),
        Err(_) => Outcome::Unknown,
    }
}

fn expected_publication(request: &DelegationRequest) -> Option<PublicationIdentity> {
    // Match the Server's authenticated canonical bytes, not an untyped JSON map.
    let (assignment, kind, version, bytes) = match request {
        DelegationRequest::Publish { submission, .. } => (
            submission.identity.clone(),
            PublicationKind::Submission,
            submission.version,
            submission.publication_bytes().ok()?,
        ),
        DelegationRequest::PublishQuestion { question, .. } => (
            question.identity.clone(),
            PublicationKind::Question {
                result_version: question.result_version,
                question_id: question.question_id.clone(),
            },
            question.question_version,
            serde_json::to_vec(question).ok()?,
        ),
        _ => return None,
    };
    Some(PublicationIdentity {
        assignment,
        kind,
        version,
        digest: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn confirmed(expected: &PublicationIdentity, actual: Option<PublicationIdentity>) -> Outcome {
    match actual {
        Some(actual) if &actual == expected => Outcome::Published(actual),
        _ => Outcome::Unknown,
    }
}

fn response_outcome(expected: &PublicationIdentity, response: Response) -> Outcome {
    match response {
        Response::Error {
            code: ovrcr::protocol::ErrorCode::Internal | ovrcr::protocol::ErrorCode::PartialFailure,
            ..
        } => Outcome::Unknown,
        Response::Error { code, .. } => Outcome::ServerRefused(code),
        Response::Delegation(snapshot) => {
            let reference = if let PublicationKind::Question {
                result_version,
                question_id,
            } = &expected.kind
            {
                snapshot
                    .questions
                    .iter()
                    .find(|state| {
                        state.issue.is_none()
                            && state.reference.assignment == expected.assignment
                            && state.reference.result_version == *result_version
                            && &state.reference.question_id == question_id
                            && state.reference.question_version == expected.version
                    })
                    .map(|state| state.reference.publication())
            } else {
                snapshot
                    .publication
                    .as_ref()
                    .map(|state| state.identity.clone())
            };
            confirmed(expected, reference)
        }
        _ => Outcome::Unknown,
    }
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn exact(value: &Value, serialized: impl serde::Serialize) -> bool {
    serde_json::to_value(serialized).is_ok_and(|typed| typed == *value)
}

fn exact_submission(value: &Value, submission: &ResearchSubmission) -> bool {
    submission.coding.is_none()
        && submission.review.is_none()
        && submission
            .publication_bytes()
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|typed| typed == *value)
}

fn valid_handle(handle: &str) -> bool {
    !handle.is_empty()
        && handle.len() <= 128
        && handle
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn valid_identity(identity: &AssignmentIdentity) -> bool {
    [
        &identity.instance,
        &identity.workflow,
        &identity.controller,
        &identity.task,
        &identity.request,
    ]
    .into_iter()
    .all(|value| valid_handle(value))
        && identity.controller_epoch > 0
        && identity.attempt > 0
        && identity.contract > 0
        && identity.session.is_some_and(|id| id.0 > 0)
        && identity.run.is_some_and(|run| run.0 > 0)
}

fn publication_request(
    call: Call,
    authority: &Authority,
) -> Result<DelegationRequest, &'static str> {
    match call.name.as_str() {
        SUBMISSION => {
            let args: SubmissionArgs = serde_json::from_value(call.arguments.clone())
                .map_err(|_| "invalid submission arguments")?;
            if !valid_handle(&args.handle)
                || !exact_submission(&call.arguments["submission"], &args.submission)
                || !valid_identity(&args.submission.identity)
                || args.submission.identity.session != Some(authority.auth.session)
                || args.submission.validate(&args.submission.identity).is_err()
            {
                return Err("invalid or foreign submission arguments");
            }
            Ok(DelegationRequest::Publish {
                auth: authority.auth.clone(),
                handle: args.handle,
                submission: args.submission,
            })
        }
        QUESTION => {
            let args: QuestionArgs = serde_json::from_value(call.arguments.clone())
                .map_err(|_| "invalid question arguments")?;
            if !valid_handle(&args.handle)
                || !exact(&call.arguments["question"], &args.question)
                || !valid_identity(&args.question.identity)
                || args.question.identity.session != Some(authority.auth.session)
                || args
                    .question
                    .validate(
                        &args.question.identity,
                        args.question.result_version,
                        args.question.supersedes.as_ref(),
                    )
                    .is_err()
            {
                return Err("invalid or foreign question arguments");
            }
            Ok(DelegationRequest::PublishQuestion {
                auth: authority.auth.clone(),
                handle: args.handle,
                question: args.question,
            })
        }
        _ => Err("unknown publication tool"),
    }
}

fn object(properties: Value) -> Value {
    let required: Vec<_> = properties.as_object().unwrap().keys().cloned().collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn catalog() -> Value {
    let id = json!({"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_-]+$"});
    let integer = json!({"type":"integer","minimum":1});
    let identity = object(
        json!({"instance":id,"workflow":id,"controller":id,"controller_epoch":integer,
        "task":id,"attempt":integer,"contract":integer,"request":id,
        "session":integer,"run":integer}),
    );
    let reference = object(
        json!({"assignment":identity,"result_version":integer,"question_id":id,
        "question_version":integer,"digest":{"type":"string","pattern":"^[a-f0-9]{64}$"}}),
    );
    let strings = json!({"type":"array","maxItems":64,"items":{"type":"string","maxLength":4096}});
    let submission = object(
        json!({"schema":{"const":1},"identity":identity,"version":integer,
        "outcome":{"enum":["success","failed","blocked"]},
        "summary":{"type":"string","minLength":1,"maxLength":65536},"evidence":strings,"unresolved":strings}),
    );
    let question = object(
        json!({"schema":{"const":1},"identity":identity,"result_version":integer,
        "question_id":id,"question_version":integer,"supersedes":{"anyOf":[{"type":"null"},reference]},
        "kind":{"enum":["Clarification","HumanDecision"]},"question":{"type":"string","minLength":1,"maxLength":32768},
        "context":{"type":"string","maxLength":32768},"choices":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":4096}}}),
    );
    json!({"tools":[
        {"name":SUBMISSION,"description":"Publish this child's immutable versioned result using the exact current brief identity. A publication does not accept work. Retry only the identical payload after uncertain delivery.",
         "inputSchema":object(json!({"handle":id,"submission":submission})),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
        {"name":QUESTION,"description":"Publish this child's exact model-originated question before waiting. Native login/approval prompts are separate; never include credentials. Publication grants no approval.",
         "inputSchema":object(json!({"handle":id,"question":question})),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
    ]})
}

fn message(
    phase: &mut Phase,
    authority: &Result<Authority, &'static str>,
    value: Value,
    send: &mut impl FnMut(&Authority, DelegationRequest) -> Outcome,
) -> Option<Value> {
    let Some(envelope) = value.as_object() else {
        return Some(rpc_error(Value::Null, -32600, "invalid request"));
    };
    let id = envelope.get("id").cloned();
    if value["jsonrpc"] != "2.0"
        || !value["method"].is_string()
        || envelope
            .keys()
            .any(|key| !["jsonrpc", "id", "method", "params"].contains(&key.as_str()))
        || id.as_ref().is_some_and(|id| {
            !(id.as_str().is_some_and(|id| id.len() <= 128) || id.is_i64() || id.is_u64())
        })
    {
        return Some(rpc_error(Value::Null, -32600, "invalid request"));
    }
    let method = value["method"].as_str().unwrap();
    let params = value.get("params");
    if id.is_none() {
        // Notifications never receive a reply and can never execute a tool.
        if method == "notifications/initialized"
            && matches!(phase, Phase::Initializing)
            && params.is_none_or(|value| value.as_object().is_some_and(|value| value.is_empty()))
        {
            *phase = Phase::Ready;
        }
        return None;
    }
    let id = id.unwrap();
    let error = |code, text| Some(rpc_error(id.clone(), code, text));
    let result = match method {
        "initialize" if matches!(phase, Phase::New) => {
            let Some(_) = params.filter(|p| {
                p["protocolVersion"].is_string()
                    && p["capabilities"].is_object()
                    && p["clientInfo"]["name"].is_string()
                    && p["clientInfo"]["version"].is_string()
            }) else {
                return error(-32602, "invalid initialize parameters");
            };
            *phase = Phase::Initializing;
            json!({"protocolVersion":VERSION,"capabilities":{"tools":{}},
                "serverInfo":{"name":"ovrcr-child-publication","version":env!("CARGO_PKG_VERSION")}})
        }
        "initialize" => return error(-32600, "already initialized"),
        "ping" => json!({}),
        _ if !matches!(phase, Phase::Ready) => {
            return error(-32600, "initialize and notifications/initialized required");
        }
        "tools/list" => {
            if params.is_some_and(|params| {
                !params
                    .as_object()
                    .is_some_and(|params| params.keys().all(|key| key == "_meta"))
            }) {
                return error(-32602, "catalog has no pagination cursor");
            }
            catalog()
        }
        "tools/call" => {
            let Ok(call) = serde_json::from_value::<Call>(params.cloned().unwrap_or(Value::Null))
            else {
                return error(-32602, "invalid tool call");
            };
            if ![SUBMISSION, QUESTION].contains(&call.name.as_str()) {
                return error(-32602, "unknown tool");
            }
            match authority {
                Err(reason) => tool_result(Outcome::Refused(reason)),
                Ok(authority) => match publication_request(call, authority) {
                    Ok(request) => tool_result(send(authority, request)),
                    Err(reason) => tool_result(Outcome::Refused(reason)),
                },
            }
        }
        _ => return error(-32601, "method not found"),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

fn serve(
    input: &mut impl BufRead,
    output: &mut impl Write,
    authority: Result<Authority, &'static str>,
    mut send: impl FnMut(&Authority, DelegationRequest) -> Outcome,
) -> io::Result<()> {
    let mut phase = Phase::New;
    loop {
        let mut line = Vec::new();
        let size = input
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if size == 0 {
            return Ok(());
        }
        let fatal = size > MAX_LINE || line.last() != Some(&b'\n');
        let response = if fatal {
            Some(rpc_error(
                Value::Null,
                -32700,
                "oversized or incomplete MCP line",
            ))
        } else {
            match serde_json::from_slice(&line) {
                Ok(value) => message(&mut phase, &authority, value, &mut send),
                Err(_) => Some(rpc_error(Value::Null, -32700, "invalid JSON")),
            }
        };
        if let Some(response) = response {
            let mut bytes = serde_json::to_vec(&response)?;
            if bytes.len() > MAX_OUTPUT {
                // The effect might have committed; never call a response-size failure refusal.
                bytes = serde_json::to_vec(
                    &json!({"jsonrpc":"2.0","id":response["id"],"result":tool_result(Outcome::Unknown)}),
                )?;
            }
            output.write_all(&bytes)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
        if fatal {
            return Ok(());
        }
    }
}

pub(super) fn run() -> AppResult<()> {
    let authority = Authority::load(|key| std::env::var(key).ok());
    serve(
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        authority,
        forward,
    )
    .map_err(RuntimeError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> Authority {
        Authority::load(|key| {
            Some(match key {
                "OVRCR_DELEGATION_SESSION" => "7".into(),
                "OVRCR_DELEGATION_TOKEN" => "a".repeat(64),
                "OVRCR_DELEGATION_SOCKET" => "/private/test.sock".into(),
                "OVRCR_DELEGATION_HOME" => "/private/instance".into(),
                _ => unreachable!(),
            })
        })
        .unwrap()
    }
    fn initialize() -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}})
    }
    fn identity() -> Value {
        json!({"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"handle","session":7,"run":1})
    }
    fn submission() -> Value {
        json!({"handle":"handle","submission":{"schema":1,"identity":identity(),"version":1,"outcome":"success","summary":"done","evidence":["README:1"],"unresolved":[]}})
    }
    fn call(name: &str, arguments: Value) -> Value {
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":arguments}})
    }
    fn ready() -> Phase {
        Phase::Ready
    }

    #[test]
    fn handshake_negotiates_current_version_and_notifications_never_publish() {
        let mut phase = Phase::New;
        let auth = Ok(authority());
        let mut no_effect =
            |_: &Authority, _: DelegationRequest| panic!("no publication before handshake");
        let early = message(
            &mut phase,
            &auth,
            call(SUBMISSION, submission()),
            &mut no_effect,
        )
        .unwrap();
        assert_eq!(early["error"]["code"], -32600);
        let mut request = initialize();
        request["params"]["protocolVersion"] = "future-version".into();
        assert_eq!(
            message(&mut phase, &auth, request, &mut no_effect).unwrap()["result"]["protocolVersion"],
            VERSION
        );
        assert!(!matches!(phase, Phase::Ready));
        assert!(
            message(
                &mut phase,
                &auth,
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                &mut no_effect
            )
            .is_none()
        );
        assert!(matches!(phase, Phase::Ready));
        let mut notification = call(SUBMISSION, submission());
        notification.as_object_mut().unwrap().remove("id");
        assert!(message(&mut phase, &auth, notification, &mut no_effect).is_none());
        assert!(message(&mut phase, &auth, json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}), &mut no_effect).is_none());
    }

    #[test]
    fn strict_catalog_has_only_two_tools_and_no_parent_or_filesystem_surface() {
        let tools = catalog()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], SUBMISSION);
        assert_eq!(tools[1]["name"], QUESTION);
        for tool in tools {
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            let properties = tool["inputSchema"]["properties"].as_object().unwrap();
            assert_eq!(properties.len(), 2);
            for forbidden in [
                "auth",
                "capability",
                "request",
                "path",
                "command",
                "acceptance",
            ] {
                assert!(!properties.contains_key(forbidden));
            }
        }
        let mut no_effect =
            |_: &Authority, _: DelegationRequest| panic!("unknown requests cannot publish");
        for value in [
            call("ovrcr_accept", submission()),
            json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"file:///private"}}),
        ] {
            assert!(
                message(&mut ready(), &Ok(authority()), value, &mut no_effect).unwrap()["error"]
                    .is_object()
            );
        }
    }

    #[test]
    fn argument_validation_rejects_forged_authority_unknown_nested_fields_and_incomplete_success() {
        let mut no_effect =
            |_: &Authority, _: DelegationRequest| panic!("invalid payload reached control client");
        for variant in 0..7 {
            let mut args = submission();
            match variant {
                0 => args["capability"] = "do-not-echo".into(),
                1 => args["submission"]["identity"]["auth"] = "do-not-echo".into(),
                2 => args["submission"]["identity"]["session"] = 8.into(),
                3 => args["submission"]["evidence"] = json!([]),
                4 => {
                    args["submission"]
                        .as_object_mut()
                        .unwrap()
                        .remove("version");
                }
                5 => args["submission"]["identity"]["run"] = Value::Null,
                _ => args["submission"]["identity"]["controller_epoch"] = 0.into(),
            }
            let response = message(
                &mut ready(),
                &Ok(authority()),
                call(SUBMISSION, args),
                &mut no_effect,
            )
            .unwrap();
            assert_eq!(response["result"]["structuredContent"]["status"], "refused");
            assert!(!response.to_string().contains("do-not-echo"));
        }
        let missing = message(
            &mut ready(),
            &Err("managed child authority missing"),
            call(SUBMISSION, submission()),
            &mut no_effect,
        )
        .unwrap();
        assert_eq!(missing["result"]["structuredContent"]["sent"], false);
    }

    #[test]
    fn both_tools_forward_only_typed_environment_authority_and_keep_unknown_distinct() {
        let mut observed = Vec::new();
        let mut send = |auth: &Authority, request: DelegationRequest| {
            assert_eq!(auth.auth.session, SessionId(7));
            observed.push(request);
            Outcome::Unknown
        };
        let question = json!({"handle":"handle","question":{"schema":1,"identity":identity(),"result_version":1,"question_id":"question","question_version":1,"supersedes":null,"kind":"Clarification","question":"Which section?","context":"README","choices":[]}});
        for (name, args) in [(SUBMISSION, submission()), (QUESTION, question)] {
            let response =
                message(&mut ready(), &Ok(authority()), call(name, args), &mut send).unwrap();
            assert_eq!(response["result"]["structuredContent"]["status"], "unknown");
            assert_eq!(response["result"]["isError"], true);
        }
        assert!(
            matches!(&observed[0], DelegationRequest::Publish { auth, .. } if auth.controller.is_none() && auth.source.is_none() && auth.capability == "a".repeat(64))
        );
        assert!(matches!(
            &observed[1],
            DelegationRequest::PublishQuestion { .. }
        ));
        let rejected = tool_result(Outcome::ServerRefused(ovrcr::protocol::ErrorCode::Conflict));
        assert_eq!(rejected["structuredContent"]["status"], "refused");
        assert!(rejected["structuredContent"].get("sent").is_none());
    }

    #[test]
    fn stdio_has_only_framed_json_and_oversized_or_truncated_lines_have_no_effect() {
        let mut frames = vec![
            initialize(),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        ];
        let mut bytes = Vec::new();
        for frame in frames.drain(..) {
            bytes.extend(serde_json::to_vec(&frame).unwrap());
            bytes.push(b'\n');
        }
        let mut output = Vec::new();
        serve(
            &mut io::Cursor::new(bytes),
            &mut output,
            Ok(authority()),
            |_, _| panic!("list has no effects"),
        )
        .unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["result"]["tools"].as_array().unwrap().len(), 2);
        for invalid in [
            vec![b'x'; MAX_LINE + 1],
            serde_json::to_vec(&initialize()).unwrap(),
            b"{broken\n".to_vec(),
        ] {
            let mut output = Vec::new();
            serve(
                &mut io::Cursor::new(invalid),
                &mut output,
                Ok(authority()),
                |_, _| panic!("invalid line published"),
            )
            .unwrap();
            let value: Value = serde_json::from_slice(&output).unwrap();
            assert_eq!(value["error"]["code"], -32700);
        }
    }

    #[test]
    fn invalid_environment_never_falls_back_to_parent_or_hook_credentials() {
        assert!(
            Authority::load(|key| (key == "OVRCR_HOOK_TOKEN").then(|| "a".repeat(64))).is_err()
        );
        for bad in ["0", "-1", "+7", " 7"] {
            assert!(
                Authority::load(|key| Some(match key {
                    "OVRCR_DELEGATION_SESSION" => bad.into(),
                    "OVRCR_DELEGATION_TOKEN" => "a".repeat(64),
                    _ => "/absolute".into(),
                }))
                .is_err()
            );
        }
    }

    #[test]
    fn internal_errors_and_mismatched_complete_publication_identity_remain_unknown() {
        let request = publication_request(
            Call {
                name: SUBMISSION.into(),
                arguments: submission(),
                _meta: None,
            },
            &authority(),
        )
        .unwrap();
        let expected = expected_publication(&request).unwrap();
        for code in [
            ovrcr::protocol::ErrorCode::Internal,
            ovrcr::protocol::ErrorCode::PartialFailure,
        ] {
            assert!(matches!(
                response_outcome(
                    &expected,
                    Response::Error {
                        code,
                        message: "must not be echoed".into()
                    }
                ),
                Outcome::Unknown
            ));
        }
        assert!(matches!(
            confirmed(&expected, Some(expected.clone())),
            Outcome::Published(_)
        ));
        for variant in 0..4 {
            let mut wrong = expected.clone();
            match variant {
                0 => wrong.digest = "0".repeat(64),
                1 => wrong.version += 1,
                2 => wrong.assignment.contract += 1,
                _ => {
                    wrong.kind = PublicationKind::Question {
                        result_version: 1,
                        question_id: "question".into(),
                    }
                }
            }
            assert!(matches!(
                confirmed(&expected, Some(wrong)),
                Outcome::Unknown
            ));
        }
        assert!(matches!(
            response_outcome(
                &expected,
                Response::Error {
                    code: ovrcr::protocol::ErrorCode::Conflict,
                    message: "explicit rejection".into()
                }
            ),
            Outcome::ServerRefused(_)
        ));
    }

    #[test]
    fn research_canonical_admission_and_digest_do_not_adopt_coding_defaults() {
        let arguments = submission();
        let request = publication_request(
            Call {
                name: SUBMISSION.into(),
                arguments: arguments.clone(),
                _meta: None,
            },
            &authority(),
        )
        .unwrap();
        let expected = expected_publication(&request).unwrap();
        let legacy = br#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"handle","session":7,"run":1},"version":1,"outcome":"success","summary":"done","evidence":["README:1"],"unresolved":[]}"#;
        assert_eq!(expected.digest, format!("{:x}", Sha256::digest(legacy)));
        let mut sent = 0;
        let reply = message(
            &mut ready(),
            &Ok(authority()),
            call(SUBMISSION, arguments.clone()),
            &mut |_, request| {
                sent += 1;
                assert_eq!(expected_publication(&request).unwrap(), expected);
                Outcome::Published(expected.clone())
            },
        )
        .unwrap();
        assert_eq!(sent, 1);
        assert_eq!(reply["result"]["structuredContent"]["status"], "published");
        for (name, value) in [
            ("review", Value::Null),
            ("coding", Value::Null),
            ("coding", json!({"checks":[],"candidate":null})),
            ("extra", Value::Bool(true)),
        ] {
            let mut invalid = arguments.clone();
            invalid["submission"][name] = value;
            let reply = message(
                &mut ready(),
                &Ok(authority()),
                call(SUBMISSION, invalid),
                &mut |_, _| panic!("closed research payload must refuse before transport"),
            )
            .unwrap();
            assert_eq!(reply["result"]["structuredContent"]["status"], "refused");
        }
        let mut reviewed = arguments.clone();
        reviewed["submission"]["coding"] = Value::Null;
        reviewed["submission"]["review"] = json!({
            "axis":"Spec","outcome":"Passed","findings":[],
            "target":{"coding":{"assignment":identity(),
                "publication":{"assignment":identity(),"kind":"Submission","version":1,"digest":"a".repeat(64)},
                "candidate":{"path":"/assignment/candidate-v1.json","digest":"b".repeat(64),"workspace":"coding","base":"c".repeat(40),"git_identity":"generation","changed_artifacts":["change.txt"]},
                "source_contract_digest":"d".repeat(64)},
                "sources":[{"kind":"Spec","reference":"SPEC.md","digest":"e".repeat(64)}]}
        });
        let typed: ResearchSubmission =
            serde_json::from_value(reviewed["submission"].clone()).unwrap();
        assert!(typed.validate(&typed.identity).is_ok());
        let reply = message(
            &mut ready(),
            &Ok(authority()),
            call(SUBMISSION, reviewed),
            &mut |_, _| panic!("review payload must refuse before transport"),
        )
        .unwrap();
        assert_eq!(reply["result"]["structuredContent"]["status"], "refused");
    }
}
