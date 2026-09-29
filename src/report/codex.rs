//! Authenticated synchronous Codex hooks. No transcript or completion inference.
use super::InvocationLease;
use super::reporter::{self, Reporter};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, SampleQuality,
};
use ovrcr_runtime::agent_runner::HookHandler;
use std::{
    ffi::{OsStr, OsString},
    path::Path,
    time::Instant,
};

pub fn version(bytes: &[u8]) -> Option<&str> {
    let value = std::str::from_utf8(bytes)
        .ok()?
        .strip_prefix("codex-cli ")?
        .strip_suffix('\n')?;
    super::versions::parse(value).map(|_| value)
}

/// Fresh interactive launches and exact `codex resume UUID`. Provider argv is never rewritten.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    eligible_launch(argv).is_some()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EligibleLaunch {
    Fresh,
    Resume(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InitialSource {
    Startup,
    Resume,
}

fn eligible_launch(argv: &[OsString]) -> Option<EligibleLaunch> {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("codex")) {
        return None;
    }
    let mut args = argv[1..].iter();
    let mut prompt = false;
    let mut resume = None;
    while let Some(arg) = args.next() {
        let arg = arg.to_str()?;
        match arg {
            "--no-alt-screen" | "--full-auto" => {}
            "--model" | "-m" | "--profile" | "-p" | "--sandbox" | "-s" | "--ask-for-approval"
            | "-a" | "--cd" | "-C" => {
                let value = args.next()?;
                if value.is_empty() || value.to_string_lossy().starts_with('-') {
                    return None;
                }
            }
            "--" => {
                return (resume.is_none() && !prompt && args.count() == 1)
                    .then_some(EligibleLaunch::Fresh);
            }
            "resume" if resume.is_none() && !prompt => {
                let value = args.next()?.to_str()?;
                if !ovrcr_runtime::codex_recovery::is_exact_identity(value) {
                    return None;
                }
                resume = Some(value.to_owned());
            }
            value
                if !value.starts_with('-')
                    && resume.is_none()
                    && !prompt
                    && !matches!(
                        value,
                        "exec"
                            | "e"
                            | "review"
                            | "login"
                            | "logout"
                            | "mcp"
                            | "mcp-server"
                            | "app-server"
                            | "app"
                            | "completion"
                            | "sandbox"
                            | "debug"
                            | "apply"
                            | "fork"
                            | "cloud"
                            | "features"
                            | "help"
                    ) =>
            {
                prompt = true;
            }
            _ => return None,
        }
    }
    match (resume, prompt) {
        (Some(conversation), false) => Some(EligibleLaunch::Resume(conversation)),
        (None, _) => Some(EligibleLaunch::Fresh),
        (Some(_), true) => None,
    }
}

pub fn supported_version(executable: &OsStr) -> bool {
    super::admission::probe_version(executable)
        .as_deref()
        .and_then(version)
        .is_some_and(|v| super::versions::CODEX.accepts(v))
}
pub fn receiver(lease: Option<InvocationLease>, argv: &[OsString]) -> HookHandler {
    let reserved = lease.is_some();
    let preflight = reporter::preflight(reserved, argv, eligible_argv, reporter::interactive());
    let launch = if preflight.is_none() && supported_version(&argv[0]) {
        eligible_launch(argv)
    } else {
        None
    };
    let unavailable = if launch.is_some() {
        None
    } else {
        preflight.or_else(|| {
            (!supported_version(&argv[0])).then_some("version probe unsupported or unavailable")
        })
    };
    // Admitted fresh or exact resume: refresh managed daemon so 0.158+ hooks
    // inherit this invocation's private channel.
    if unavailable.is_none() {
        ovrcr_runtime::codex_recovery::prepare_managed_launch();
    }
    let mut reporter = Reporter::new(AgentProvider::Codex, lease, None);
    if let Some(reason) = unavailable {
        reporter.unavailable("Codex", reason);
    } else if matches!(launch, Some(EligibleLaunch::Resume(_))) {
        eprintln!("agent awaiting certified resume");
    }
    let expected = match &launch {
        Some(EligibleLaunch::Resume(conversation)) => Some(conversation.clone()),
        Some(EligibleLaunch::Fresh) | None => None,
    };
    let initial_source = match &launch {
        Some(EligibleLaunch::Resume(_)) => Some(InitialSource::Resume),
        Some(EligibleLaunch::Fresh) => Some(InitialSource::Startup),
        None => None,
    };
    let recovery = launch.as_ref().and_then(|launch| {
        let executable = std::path::PathBuf::from(argv.first()?);
        let executable = if executable.is_absolute() {
            executable
        } else {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|directory| directory.join(&executable))
                .find(|path| path.is_absolute() && path.is_file())?
        };
        let conversation = match launch {
            EligibleLaunch::Resume(conversation) => conversation.clone(),
            EligibleLaunch::Fresh => String::new(),
        };
        Some(ovrcr_protocol::CodexConversation {
            conversation,
            executable,
            history: None,
            config_dir: ovrcr_runtime::codex_recovery::config_dir().ok()?,
            options: ovrcr_runtime::codex_recovery::launch_options(argv).ok()?,
        })
    });
    reporter.handler(Hooks {
        active: None,
        turn: None,
        open_requests: Vec::new(),
        recovery,
        expected,
        initial_source,
        initial_accepted: false,
    })
}

/// Codex's frames: one open response cycle at a time, identified by its session and turn.
/// Approvals use `approval:{turn_id}` from a verified root `PermissionRequest`.
struct Hooks {
    active: Option<String>,
    /// Current root turn for Input correlation; kept through Stop/Interrupt like Claude's prompt.
    turn: Option<String>,
    open_requests: Vec<(String, ovrcr_protocol::InputKind)>,
    recovery: Option<ovrcr_protocol::CodexConversation>,
    expected: Option<String>,
    initial_source: Option<InitialSource>,
    /// Certified SessionStart for this invocation has been accepted (resume attach).
    initial_accepted: bool,
}

impl Hooks {
    fn retain(
        &self,
        reporter: &mut Reporter,
        payload: &serde_json::Value,
        session: &str,
        deadline: Instant,
    ) {
        let Some(mut reference) = self.recovery.clone() else {
            return;
        };
        reference.conversation = session.into();
        reference.history = payload["transcript_path"]
            .as_str()
            .filter(|path| Path::new(path).is_absolute())
            .map(Into::into);
        if ovrcr_runtime::codex_recovery::validate(&reference).is_err() {
            reporter.invalidate_conversation(deadline);
            return;
        }
        if ovrcr_runtime::codex_recovery::validate_history(&reference).is_err() {
            reference.history = None;
        }
        // Recovery persistence is independent of activity publication. Reporter
        // retries a failed durable write with the same certified binding.
        reporter.retain_conversation(
            ovrcr_protocol::ConversationReference::Codex(reference),
            deadline,
        );
    }

    fn approval_id(turn: &str) -> String {
        format!("approval:{turn}")
    }

    fn current_turn(&self, session: &str, turn: &str, reporter: &Reporter) -> bool {
        self.turn.as_deref() == Some(turn)
            && reporter
                .binding()
                .is_some_and(|binding| binding.conversation == session)
    }

    fn open_approval(&mut self, reporter: &mut Reporter, turn: &str, deadline: Instant) -> Vec<u8> {
        let id = Self::approval_id(turn);
        if self.open_requests.iter().any(|(open, _)| open == &id)
            || self.open_requests.len() >= ovrcr_protocol::MAX_INPUT_REQUESTS
        {
            return reporter::IGNORED.to_vec();
        }
        self.open_requests
            .push((id, ovrcr_protocol::InputKind::Approval));
        self.publish_requests(reporter, deadline)
    }

    fn close_approvals(&mut self, reporter: &mut Reporter, deadline: Instant) -> Option<Vec<u8>> {
        if self.open_requests.is_empty() {
            return None;
        }
        self.open_requests.clear();
        let published = self.publish_requests(reporter, deadline);
        (published.as_slice() != reporter::ACCEPTED).then_some(published)
    }

    fn publish_requests(&mut self, reporter: &mut Reporter, deadline: Instant) -> Vec<u8> {
        let requests = self
            .open_requests
            .iter()
            .map(|(id, kind)| ovrcr_protocol::InputRequest {
                id: id.clone(),
                kind: *kind,
            })
            .collect();
        reporter.publish(AgentObservation::Input(requests), deadline)
    }
}

impl reporter::Frames for Hooks {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        if let Some(answer) = reporter.own_frame(input, native_root) {
            return answer;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(input) else {
            return reporter::IGNORED.to_vec();
        };
        if value["provider"] != "codex" || value["origin"] != "codex-hook" {
            return reporter::IGNORED.to_vec();
        }
        let payload = &value["payload"];
        let Some(event) = payload["hook_event_name"].as_str() else {
            return reporter::IGNORED.to_vec();
        };
        if event.starts_with("Subagent")
            || payload
                .get("agent_id")
                .is_some_and(|id| id.as_str() != Some(""))
        {
            return reporter::IGNORED.to_vec();
        }
        if !matches!(
            event,
            "SessionStart"
                | "UserPromptSubmit"
                | "Stop"
                | "Interrupt"
                | "SessionEnd"
                | "PermissionRequest"
                | "PreToolUse"
                | "PostToolUse"
        ) {
            return reporter::IGNORED.to_vec();
        }
        let Some(session) = payload["session_id"]
            .as_str()
            .filter(|s| ovrcr_protocol::validate_agent_id(s).is_ok())
        else {
            return reporter::IGNORED.to_vec();
        };
        if event == "SessionStart" {
            let Some(source) = payload["source"].as_str() else {
                return reporter::IGNORED.to_vec();
            };
            // Exact resume must certify Root identity before any reporting is admitted.
            if self.initial_source == Some(InitialSource::Resume) {
                if self.initial_accepted {
                    // In-process switches stay conservative until conversation-switch tickets.
                    if self.active.is_some() {
                        reporter.invalidate_conversation(deadline);
                    }
                    reporter.disable();
                    return reporter::UNAVAILABLE.to_vec();
                }
                let Some(expected) = self.expected.as_deref() else {
                    return reporter::IGNORED.to_vec();
                };
                if source != "resume" {
                    if session == expected {
                        reporter.disable();
                        return reporter::UNAVAILABLE.to_vec();
                    }
                    return reporter::IGNORED.to_vec();
                }
                if session != expected
                    || self.recovery.is_none()
                    || payload["transcript_path"].as_str().is_none()
                {
                    return reporter::IGNORED.to_vec();
                }
                if let Some(unavailable) = reporter.bind_or_disable(session, deadline, false) {
                    return unavailable;
                }
                self.retain(reporter, payload, session, deadline);
                self.initial_accepted = true;
                return reporter::ACCEPTED.to_vec();
            }
            if source != "startup" || self.recovery.is_none() {
                return reporter::IGNORED.to_vec();
            }
            if self.active.is_some() {
                reporter.invalidate_conversation(deadline);
                return reporter::UNAVAILABLE.to_vec();
            }
            if let Some(unavailable) = reporter.bind_or_disable(session, deadline, false) {
                return unavailable;
            }
            self.retain(reporter, payload, session, deadline);
            self.initial_accepted = true;
            return reporter::ACCEPTED.to_vec();
        }
        let Some(turn) = payload["turn_id"]
            .as_str()
            .filter(|s| ovrcr_protocol::validate_agent_id(s).is_ok())
        else {
            return reporter::IGNORED.to_vec();
        };
        // A newline cannot appear in either validated identity, so one string names the
        // pair without two of them ever colliding.
        let identity = format!("{session}\n{turn}");
        if event == "SessionEnd" {
            if self.active.as_ref() == Some(&identity)
                || (self.active.is_none()
                    && reporter.published(&identity)
                    && reporter
                        .binding()
                        .is_some_and(|binding| binding.conversation == session))
            {
                let _ = self.close_approvals(reporter, deadline);
                self.turn = None;
                reporter.disable();
            }
            return reporter::IGNORED.to_vec();
        }
        // Verified approval open: root PermissionRequest for the current turn. PreToolUse,
        // PostToolUse and auto-approved tools never open a request. Publish Input only —
        // do not rewrite the underlying activity sample (Busy or Ready).
        if event == "PermissionRequest" {
            if !self.current_turn(session, turn, reporter) {
                return reporter::IGNORED.to_vec();
            }
            return self.open_approval(reporter, turn, deadline);
        }
        // Allow / deny / cancel / next tool close every open approval for this turn: Codex
        // PermissionRequest has no resolve id, so the closing boundary is the next
        // attributable root activity after a genuine open (mirrors Claude).
        if matches!(event, "PreToolUse" | "PostToolUse") {
            if !self.current_turn(session, turn, reporter) || self.open_requests.is_empty() {
                return reporter::IGNORED.to_vec();
            }
            self.open_requests.clear();
            return self.publish_requests(reporter, deadline);
        }
        let state = if event == "UserPromptSubmit" {
            // Exact resume cannot bind from a prompt alone — SessionStart(source=resume)
            // must establish the Root identity first. Historical Stop/Submit before that
            // stay ignored and never invent Ready.
            if self.initial_source == Some(InitialSource::Resume) && !self.initial_accepted {
                return reporter::IGNORED.to_vec();
            }
            if self.initial_source == Some(InitialSource::Resume)
                && self
                    .expected
                    .as_deref()
                    .is_some_and(|expected| expected != session)
            {
                return reporter::IGNORED.to_vec();
            }
            // Deduplicate *before* any revisions, binding changes, or freshness updates.
            if let Some(answer) = reporter.admit_cycle(&identity) {
                return answer;
            }
            if self.active.is_some() {
                reporter.disable();
                return reporter::UNAVAILABLE.to_vec();
            }
            if let Some(unavailable) = reporter.bind_or_disable(session, deadline, false) {
                return unavailable;
            }
            // A new root turn retires any approval still open for a prior turn_id.
            if let Some(failed) = self.close_approvals(reporter, deadline) {
                return failed;
            }
            self.retain(reporter, payload, session, deadline);
            self.active = Some(identity);
            self.turn = Some(turn.to_owned());
            AgentActivity::Busy
        } else {
            if self.active.as_ref() != Some(&identity) {
                return reporter::IGNORED.to_vec();
            }
            if let Some(failed) = self.close_approvals(reporter, deadline) {
                return failed;
            }
            self.active = None;
            if event == "Stop" {
                AgentActivity::ResponseReady
            } else {
                AgentActivity::Idle
            }
        };
        reporter.publish(
            AgentObservation::Activity(ActivitySample {
                state,
                quality: SampleQuality::Observed,
                turn: Some(turn.to_owned()),
            }),
            deadline,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::reporter::Frames;
    use crate::report::reporter::scripted::Supervisor;

    fn start() -> (Hooks, Reporter, Supervisor) {
        let (reporter, supervisor) = Supervisor::reporter(AgentProvider::Codex);
        (
            Hooks {
                active: None,
                turn: None,
                open_requests: Vec::new(),
                recovery: None,
                expected: None,
                initial_source: Some(InitialSource::Startup),
                initial_accepted: false,
            },
            reporter,
            supervisor,
        )
    }

    fn resume_hooks(expected: &str) -> (Hooks, Reporter, Supervisor) {
        let (reporter, supervisor) = Supervisor::reporter(AgentProvider::Codex);
        (
            Hooks {
                active: None,
                turn: None,
                open_requests: Vec::new(),
                recovery: Some(ovrcr_protocol::CodexConversation {
                    conversation: expected.into(),
                    executable: "/bin/codex".into(),
                    history: None,
                    config_dir: "/config".into(),
                    options: vec![],
                }),
                expected: Some(expected.into()),
                initial_source: Some(InitialSource::Resume),
                initial_accepted: false,
            },
            reporter,
            supervisor,
        )
    }
    fn deadline() -> Instant {
        Instant::now() + std::time::Duration::from_secs(10)
    }
    fn hook(event: &str, session: &str, turn: &str) -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"provider":"codex","origin":"codex-hook","payload":{
                "hook_event_name":event,"session_id":session,"turn_id":turn
            }}),
        )
        .unwrap()
    }
    fn drive(hooks: &mut Hooks, reporter: &mut Reporter, input: &[u8]) -> Vec<u8> {
        hooks.frame(reporter, input, true, deadline())
    }
    fn states(supervisor: Supervisor) -> Vec<(u64, AgentActivity, Option<String>, SampleQuality)> {
        Supervisor::activity(&supervisor.observed())
            .into_iter()
            .map(|(generation, _, state, turn, quality)| (generation, state, turn, quality))
            .collect()
    }

    #[test]
    fn recovery_uses_only_certified_root_identity_and_missing_replacement_history_cannot_reopen_old_identity()
     {
        use crate::report::reporter::scripted::Observed;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("exact.jsonl");
        let a = "01a08e5c-7480-7052-9964-9224aadebef0";
        let b = "01a08e94-0deb-76c3-a2b5-540c4874a53f";
        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{a}\",\"source\":\"cli\"}}}}\n"
            ),
        )
        .unwrap();
        let (mut hooks, mut reporter, supervisor) = start();
        hooks.recovery = Some(ovrcr_protocol::CodexConversation {
            conversation: String::new(),
            executable: "/bin/codex".into(),
            history: None,
            config_dir: root.path().into(),
            options: vec![],
        });
        let frame = |id: &str, child: bool| {
            serde_json::to_vec(&serde_json::json!({
                "provider": "codex", "origin": "codex-hook", "payload": {
                    "hook_event_name": "SessionStart", "source": "startup", "session_id": id,
                    "transcript_path": path, "agent_id": if child { "child" } else { "" },
                }
            }))
            .unwrap()
        };
        assert_eq!(
            hooks.frame(&mut reporter, &frame(a, false), false, deadline()),
            reporter::IGNORED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &frame(a, true)),
            reporter::IGNORED
        );
        assert!(reporter.binding().is_none());
        assert_eq!(
            drive(&mut hooks, &mut reporter, &frame(a, false)),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &frame(b, false)),
            reporter::ACCEPTED
        );
        let observed = supervisor.observed();
        assert!(
            Supervisor::activity(&observed).is_empty(),
            "startup must not invent Ready"
        );
        let retained: Vec<_> = observed
            .into_iter()
            .filter_map(|event| match event {
                Observed::Retain {
                    reference: ovrcr_protocol::ConversationReference::Codex(reference),
                    ..
                } => Some(reference),
                _ => None,
            })
            .collect();
        assert_eq!(retained.len(), 2);
        assert_eq!(retained[0].conversation, a);
        assert_eq!(retained[0].history, Some(path));
        assert_eq!(retained[1].conversation, b);
        assert_eq!(
            retained[1].history, None,
            "mismatched file retained an old recoverable identity"
        );
    }

    #[test]
    fn exact_resume_admits_matching_root_then_reports_new_turn_without_historical_ready() {
        let id = "01a08e5c-7480-7052-9964-9224aadebef0";
        let other = "01a08e94-0deb-76c3-a2b5-540c4874a53f";
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("exact.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"source\":\"cli\"}}}}\n"
            ),
        )
        .unwrap();
        let (mut hooks, mut reporter, _supervisor) = resume_hooks(id);
        hooks.recovery.as_mut().unwrap().config_dir = root.path().into();
        let frame = |source: &str, session: &str, child: bool| {
            serde_json::to_vec(&serde_json::json!({
                "provider": "codex", "origin": "codex-hook", "payload": {
                    "hook_event_name": "SessionStart", "source": source, "session_id": session,
                    "transcript_path": path, "agent_id": if child { "child" } else { "" },
                }
            }))
            .unwrap()
        };
        // Historical / mismatched / child identity cannot attach.
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", id, "historical")),
            reporter::IGNORED
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", id, "historical")
            ),
            reporter::IGNORED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &frame("startup", id, false)),
            reporter::UNAVAILABLE
        );
        let (mut hooks, mut reporter, supervisor) = resume_hooks(id);
        hooks.recovery.as_mut().unwrap().config_dir = root.path().into();
        assert_eq!(
            drive(&mut hooks, &mut reporter, &frame("resume", other, false)),
            reporter::IGNORED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &frame("resume", id, true)),
            reporter::IGNORED
        );
        assert!(reporter.binding().is_none());
        assert_eq!(
            drive(&mut hooks, &mut reporter, &frame("resume", id, false)),
            reporter::ACCEPTED
        );
        assert!(hooks.initial_accepted);
        assert_eq!(
            reporter.binding().map(|b| b.conversation.as_str()),
            Some(id)
        );
        // New resumed turn: Busy then Observed Ready once; repeats do not republish.
        // SessionStart itself must not have invented Ready — only the new turn pair.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", id, "new-turn")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", id, "new-turn")),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", id, "new-turn")),
            reporter::IGNORED
        );
        assert_eq!(
            states(supervisor),
            vec![
                (
                    1,
                    AgentActivity::Busy,
                    Some("new-turn".into()),
                    SampleQuality::Observed
                ),
                (
                    1,
                    AgentActivity::ResponseReady,
                    Some("new-turn".into()),
                    SampleQuality::Observed
                ),
            ],
            "historical resume attach must not invent an extra Ready"
        );
    }

    #[test]
    fn eligible_launch_accepts_exact_resume_and_rejects_picker_or_prompted_resume() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        let id = "01a08e5c-7480-7052-9964-9224aadebef0";
        assert_eq!(
            eligible_launch(&args(&["codex", "resume", id])),
            Some(EligibleLaunch::Resume(id.into()))
        );
        assert_eq!(
            eligible_launch(&args(&[
                "codex",
                "--model",
                "gpt",
                "resume",
                id,
                "--no-alt-screen"
            ])),
            Some(EligibleLaunch::Resume(id.into()))
        );
        assert_eq!(
            eligible_launch(&args(&["codex", "resume", id, "--model", "gpt"])),
            Some(EligibleLaunch::Resume(id.into()))
        );
        assert_eq!(
            eligible_launch(&args(&["codex"])),
            Some(EligibleLaunch::Fresh)
        );
        for values in [
            vec!["codex", "resume"],
            vec!["codex", "resume", "not-a-uuid"],
            vec!["codex", "resume", id, "prompt"],
            vec!["codex", "fork", id],
            vec!["codex", "exec"],
        ] {
            assert_eq!(eligible_launch(&args(&values)), None, "{values:?}");
        }
    }

    #[test]
    fn non_resumable_reporting_identity_still_produces_ready() {
        let (mut hooks, mut reporter, supervisor) = start();
        hooks.recovery = Some(ovrcr_protocol::CodexConversation {
            conversation: String::new(),
            executable: "/bin/codex".into(),
            history: None,
            config_dir: "/config".into(),
            options: vec![],
        });
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t1")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", "root", "t1")),
            reporter::ACCEPTED
        );
        assert_eq!(
            states(supervisor).last().unwrap().1,
            AgentActivity::ResponseReady
        );
    }

    #[test]
    fn a_closed_turn_that_repeats_is_a_duplicate_and_publishes_nothing() {
        let (mut hooks, mut reporter, supervisor) = start();
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t1")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", "root", "t1")),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t1")
            ),
            reporter::IGNORED,
            "the turn identity is already published; a repeat is not a new response"
        );
        assert!(hooks.active.is_none() && !reporter.closed());
        // The pair is one identity: neither half alone matches another turn's.
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", "root", "t1x")),
            reporter::IGNORED
        );
        assert_eq!(
            states(supervisor),
            vec![
                (
                    1,
                    AgentActivity::Busy,
                    Some("t1".into()),
                    SampleQuality::Observed
                ),
                (
                    1,
                    AgentActivity::ResponseReady,
                    Some("t1".into()),
                    SampleQuality::Observed
                ),
            ]
        );
    }

    #[test]
    fn a_session_end_for_a_turn_this_receiver_published_ends_reporting() {
        let (mut hooks, mut reporter, supervisor) = start();
        drive(
            &mut hooks,
            &mut reporter,
            &hook("UserPromptSubmit", "root", "t1"),
        );
        drive(&mut hooks, &mut reporter, &hook("Stop", "root", "t1"));
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("SessionEnd", "root", "unseen")
            ),
            reporter::IGNORED
        );
        assert!(
            !reporter.closed(),
            "an end naming a turn this receiver never published is not this conversation's"
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("SessionEnd", "root", "t1")),
            reporter::IGNORED
        );
        assert!(reporter.closed());
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t2")
            ),
            reporter::UNAVAILABLE,
            "a receiver that has ended reports nothing more"
        );
        assert_eq!(states(supervisor).len(), 2);
    }

    #[test]
    fn a_frame_that_is_not_the_native_roots_is_never_this_receivers() {
        let (mut hooks, mut reporter, supervisor) = start();
        assert_eq!(
            hooks.frame(
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t1"),
                false,
                deadline()
            ),
            reporter::IGNORED
        );
        assert!(hooks.active.is_none() && !reporter.closed());
        // The same frame from the native root is this receiver's.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t1")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(states(supervisor).len(), 1);
    }

    #[test]
    fn a_second_prompt_while_a_cycle_is_open_ends_reporting() {
        // Codex reports one response cycle at a time; an overlap is ordering ambiguity.
        let (mut hooks, mut reporter, supervisor) = start();
        drive(
            &mut hooks,
            &mut reporter,
            &hook("UserPromptSubmit", "root", "t1"),
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t2")
            ),
            reporter::UNAVAILABLE
        );
        assert!(reporter.closed());
        assert_eq!(states(supervisor).len(), 1);
    }

    fn tool_hook(event: &str, session: &str, turn: &str, tool_use_id: Option<&str>) -> Vec<u8> {
        let mut payload = serde_json::json!({
            "hook_event_name": event,
            "session_id": session,
            "turn_id": turn,
            "tool_name": "Bash",
        });
        if let Some(id) = tool_use_id {
            payload["tool_use_id"] = id.into();
        }
        serde_json::to_vec(&serde_json::json!({
            "provider": "codex",
            "origin": "codex-hook",
            "payload": payload,
        }))
        .unwrap()
    }

    #[test]
    fn permission_request_opens_approval_identity_and_closes_on_next_root_activity() {
        use ovrcr_protocol::InputKind;
        let (mut hooks, mut reporter, supervisor) = start();
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t1")
            ),
            reporter::ACCEPTED
        );
        // PreToolUse alone never opens Input.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PreToolUse", "root", "t1", Some("exec-1"))
            ),
            reporter::IGNORED
        );
        assert!(hooks.open_requests.is_empty());
        // Verified PermissionRequest opens approval:{turn_id}.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PermissionRequest", "root", "t1", None)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            hooks.open_requests,
            vec![("approval:t1".into(), InputKind::Approval)]
        );
        // Duplicate PermissionRequest for the same turn is not a second request.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PermissionRequest", "root", "t1", None)
            ),
            reporter::IGNORED
        );
        assert_eq!(hooks.open_requests.len(), 1);
        // Child / wrong turn cannot open or clear.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &serde_json::to_vec(&serde_json::json!({
                    "provider":"codex","origin":"codex-hook","payload":{
                        "hook_event_name":"PermissionRequest",
                        "session_id":"root","turn_id":"t1","tool_name":"Bash",
                        "agent_id":"child"
                    }
                }))
                .unwrap()
            ),
            reporter::IGNORED
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PermissionRequest", "root", "other", None)
            ),
            reporter::IGNORED
        );
        assert_eq!(
            hooks.open_requests.len(),
            1,
            "stale/child PermissionRequest must not clear"
        );
        // Allow path: PostToolUse closes; underlying Busy sample is untouched.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PostToolUse", "root", "t1", Some("exec-1"))
            ),
            reporter::ACCEPTED
        );
        assert!(hooks.open_requests.is_empty());
        // Finish t1 before opening a distinct later turn (Codex is one cycle at a time).
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", "root", "t1")),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &hook("UserPromptSubmit", "root", "t2")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PermissionRequest", "root", "t2", None)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            hooks.open_requests,
            vec![("approval:t2".into(), InputKind::Approval)]
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Stop", "root", "t2")),
            reporter::ACCEPTED
        );
        assert!(hooks.open_requests.is_empty());
        let observed = supervisor.observed();
        assert_eq!(
            Supervisor::inputs(&observed),
            vec![
                vec![("approval:t1".into(), InputKind::Approval)],
                vec![],
                vec![("approval:t2".into(), InputKind::Approval)],
                vec![],
            ]
        );
        assert_eq!(
            Supervisor::activity(&observed).last().unwrap().2,
            AgentActivity::ResponseReady
        );
    }

    #[test]
    fn permission_request_can_cover_ready_and_native_fixture_correlates_unanswered_selector() {
        use ovrcr_protocol::InputKind;
        let (mut hooks, mut reporter, supervisor) = start();
        drive(
            &mut hooks,
            &mut reporter,
            &hook("UserPromptSubmit", "root", "t1"),
        );
        drive(&mut hooks, &mut reporter, &hook("Stop", "root", "t1"));
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PermissionRequest", "root", "t1", None)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            hooks.open_requests,
            vec![("approval:t1".into(), InputKind::Approval)]
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PreToolUse", "root", "t1", Some("exec-2"))
            ),
            reporter::ACCEPTED
        );
        assert!(hooks.open_requests.is_empty());
        let observed = supervisor.observed();
        assert_eq!(
            Supervisor::activity(&observed).last().unwrap().2,
            AgentActivity::ResponseReady,
            "closing Input must not rewrite the underlying Ready sample"
        );
        assert_eq!(
            Supervisor::inputs(&observed),
            vec![vec![("approval:t1".into(), InputKind::Approval)], vec![],]
        );

        // Native matrix: PermissionRequest fires while the unanswered selector is present.
        let unanswered = include_str!(
            "../../tests/fixtures/agent-reporting/codex/0.153.0/native-matrix/permission-unanswered.json"
        );
        let value: serde_json::Value = serde_json::from_str(unanswered).unwrap();
        assert_eq!(
            value["callback"]["event"]["hook_event_name"],
            "PermissionRequest"
        );
        assert!(value["parent_cua_selector_unanswered"].as_bool().unwrap());
        assert!(value["marker_absent"].as_bool().unwrap());
        assert!(value["callback"]["event"].get("tool_use_id").is_none());
        let allowed = include_str!(
            "../../tests/fixtures/agent-reporting/codex/0.153.0/native-matrix/permission-allowed.json"
        );
        let allowed: serde_json::Value = serde_json::from_str(allowed).unwrap();
        let names: Vec<_> = allowed["callbacks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["event"]["hook_event_name"].as_str().unwrap())
            .take(3)
            .collect();
        assert_eq!(
            names,
            ["PreToolUse", "PermissionRequest", "PostToolUse"],
            "allow path is PreToolUse → PermissionRequest → PostToolUse"
        );
    }

    #[test]
    fn auto_approved_tools_and_interrupt_do_not_invent_or_leak_requests() {
        use ovrcr_protocol::InputKind;
        let (mut hooks, mut reporter, supervisor) = start();
        drive(
            &mut hooks,
            &mut reporter,
            &hook("UserPromptSubmit", "root", "t1"),
        );
        // Auto-approved: PreToolUse + PostToolUse without PermissionRequest.
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PreToolUse", "root", "t1", Some("exec-a"))
            ),
            reporter::IGNORED
        );
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PostToolUse", "root", "t1", Some("exec-a"))
            ),
            reporter::IGNORED
        );
        assert!(hooks.open_requests.is_empty());
        assert_eq!(
            drive(
                &mut hooks,
                &mut reporter,
                &tool_hook("PermissionRequest", "root", "t1", None)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            hooks.open_requests,
            vec![("approval:t1".into(), InputKind::Approval)]
        );
        assert_eq!(
            drive(&mut hooks, &mut reporter, &hook("Interrupt", "root", "t1")),
            reporter::ACCEPTED
        );
        assert!(hooks.open_requests.is_empty());
        let observed = supervisor.observed();
        assert_eq!(
            Supervisor::activity(&observed).last().unwrap().2,
            AgentActivity::Idle
        );
        assert_eq!(
            Supervisor::inputs(&observed),
            vec![vec![("approval:t1".into(), InputKind::Approval)], vec![],]
        );
    }
}
