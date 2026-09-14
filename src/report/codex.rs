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
    reporter.handler(Hooks { active: None })
}

/// Codex's frames: one open response cycle at a time, identified by its session and turn.
struct Hooks {
    active: Option<String>,
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
            "UserPromptSubmit" | "Stop" | "Interrupt" | "SessionEnd"
        ) {
            return reporter::IGNORED.to_vec();
        }
        let Some(session) = payload["session_id"]
            .as_str()
            .filter(|s| ovrcr_protocol::validate_agent_id(s).is_ok())
        else {
            return reporter::IGNORED.to_vec();
        };
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
        (Hooks { active: None }, reporter, supervisor)
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
