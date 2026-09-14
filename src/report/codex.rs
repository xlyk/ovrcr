//! Authenticated synchronous Codex hooks. No transcript or completion inference.
use super::InvocationLease;
use super::reporter::{self, Cycle, Reporter};
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
    let unavailable = reporter::preflight(&lease, argv, eligible_argv).or_else(|| {
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
        if reporter.closed() {
            return reporter::UNAVAILABLE.to_vec();
        }
        if !reporter::own_frame(input, native_root) {
            return reporter::IGNORED.to_vec();
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
            match reporter.admit_cycle(&identity) {
                Cycle::Known => return reporter::IGNORED.to_vec(),
                Cycle::Exhausted => return reporter::UNAVAILABLE.to_vec(),
                Cycle::Fresh => {}
            }
            if self.active.is_some() {
                reporter.disable();
                return reporter::UNAVAILABLE.to_vec();
            }
            if !reporter.bind(session, deadline, false) {
                reporter.disable();
                return reporter::UNAVAILABLE.to_vec();
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
