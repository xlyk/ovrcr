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

pub const PINNED_VERSION: &str = "0.153.0";

/// Fresh interactive grammar only. Provider arguments are never rewritten.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("codex")) {
        return false;
    }
    let mut args = argv[1..].iter();
    let mut prompt = false;
    while let Some(arg) = args.next() {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        match arg {
            "--no-alt-screen" | "--full-auto" => {}
            "--model" | "-m" | "--profile" | "-p" | "--sandbox" | "-s" | "--ask-for-approval"
            | "-a" | "--cd" | "-C" => {
                if args
                    .next()
                    .is_none_or(|v| v.is_empty() || v.to_string_lossy().starts_with('-'))
                {
                    return false;
                }
            }
            "--" => return !prompt && args.count() == 1,
            value
                if !value.starts_with('-')
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
                            | "resume"
                            | "fork"
                            | "cloud"
                            | "features"
                            | "help"
                    ) =>
            {
                prompt = true
            }
            _ => return false,
        }
    }
    true
}
pub fn supported_version(executable: &OsStr) -> bool {
    super::admission::probe_version(executable).as_deref() == Some(b"codex-cli 0.153.0\n")
}
pub fn receiver(lease: Option<InvocationLease>, argv: &[OsString]) -> HookHandler {
    let unavailable = reporter::preflight(
        lease.is_some(),
        argv,
        eligible_argv,
        reporter::interactive(),
    )
    .or_else(|| {
        (!supported_version(&argv[0])).then_some("version probe unsupported or unavailable")
    });
    let mut reporter = Reporter::new(AgentProvider::Codex, lease, None);
    if let Some(reason) = unavailable {
        reporter.unavailable("Codex", reason);
    }
    let recovery = (|| {
        let executable = std::path::PathBuf::from(argv.first()?);
        let executable = if executable.is_absolute() {
            executable
        } else {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|directory| directory.join(&executable))
                .find(|path| path.is_absolute() && path.is_file())?
        };
        Some(ovrcr_protocol::CodexConversation {
            conversation: String::new(),
            executable,
            history: None,
            config_dir: ovrcr_runtime::codex_recovery::config_dir().ok()?,
            options: ovrcr_runtime::codex_recovery::launch_options(argv).ok()?,
        })
    })();
    reporter.handler(Hooks {
        active: None,
        recovery,
    })
}

/// Codex's frames: one open response cycle at a time, identified by its session and turn.
struct Hooks {
    active: Option<String>,
    recovery: Option<ovrcr_protocol::CodexConversation>,
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
            "SessionStart" | "UserPromptSubmit" | "Stop" | "Interrupt" | "SessionEnd"
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
            if payload["source"] != "startup" || self.recovery.is_none() {
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
                reporter.disable();
            }
            return reporter::IGNORED.to_vec();
        }
        let state = if event == "UserPromptSubmit" {
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
            self.retain(reporter, payload, session, deadline);
            self.active = Some(identity);
            AgentActivity::Busy
        } else {
            if self.active.as_ref() != Some(&identity) {
                return reporter::IGNORED.to_vec();
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
                recovery: None,
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
}
