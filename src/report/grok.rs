//! Managed Grok launch: the supervisor picks the session UUID, retains the history file
//! Grok's documented session store keeps for it, and only then hands the UUID to Grok.
//! No hooks, no activity, no transcript watching, and no resume: the retained file is a
//! title source whose identity a reader checks with `grok_recovery::validate_history`.
use super::InvocationLease;
use super::reporter::{self, Reporter};
use ovrcr_protocol::{AgentProvider, ConversationReference, GrokConversation};
use ovrcr_runtime::agent_runner::HookHandler;
use std::{
    ffi::{OsStr, OsString},
    path::Path,
    time::{Duration, Instant},
};

/// `grok 1.0.40 (eb1a2256660d) [stable]` — the version is the token after the name.
pub fn version(bytes: &[u8]) -> Option<&str> {
    let value = std::str::from_utf8(bytes)
        .ok()?
        .strip_prefix("grok ")?
        .split_whitespace()
        .next()?;
    super::versions::parse(value).map(|_| value)
}

/// A fresh interactive session in the current directory: the only launch whose session
/// UUID the supervisor may choose and whose history location the store documents.
/// Resume, continue, fork, an explicit UUID, headless output, another working directory
/// or worktree, and every subcommand keep their native argv unchanged.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("grok")) {
        return false;
    }
    for arg in &argv[1..] {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        if arg == "--" {
            return true;
        }
        if arg.starts_with('-') {
            let option = arg.split_once('=').map_or(arg, |(name, _)| name);
            if matches!(
                option,
                "-r" | "--resume"
                    | "-c"
                    | "--continue"
                    | "-s"
                    | "--session-id"
                    | "--fork-session"
                    | "--restore-code"
                    | "-p"
                    | "--single"
                    | "--prompt-file"
                    | "--prompt-json"
                    | "--output-format"
                    | "--json-schema"
                    | "--include-partial-messages"
                    | "--cwd"
                    | "-w"
                    | "--worktree"
                    | "--worktree-ref"
                    | "--ref"
                    | "-h"
                    | "--help"
                    | "-v"
                    | "--version"
            ) {
                return false;
            }
        } else if matches!(
            arg,
            "agent"
                | "clone"
                | "completions"
                | "cursor-worker"
                | "dashboard"
                | "doctor"
                | "du"
                | "disk-usage"
                | "export"
                | "help"
                | "inspect"
                | "leader"
                | "login"
                | "logout"
                | "mcp"
                | "memory"
                | "models"
                | "plugin"
                | "sessions"
                | "setup"
                | "trace"
                | "update"
                | "usage"
                | "version"
                | "v"
                | "worktree"
                | "wrap"
        ) {
            // An option value that happens to spell a subcommand also lands here; that
            // only forgoes retention, it never rewrites Grok's own arguments.
            return false;
        }
    }
    true
}

pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler {
    // No version gate: argv eligibility alone decides retention.
    let unavailable = reporter::preflight(
        lease.is_some(),
        argv,
        eligible_argv,
        reporter::interactive(),
    );
    let mut reporter = Reporter::new(AgentProvider::Grok, lease, None);
    match unavailable {
        Some(reason) => reporter.unavailable("Grok", reason),
        None => {
            let store = (|| {
                Some((
                    ovrcr_runtime::grok_recovery::config_dir().ok()?,
                    std::env::current_dir().ok()?,
                ))
            })();
            match store {
                Some((home, cwd)) => {
                    admit(
                        &mut reporter,
                        argv,
                        &home,
                        &cwd,
                        Instant::now() + Duration::from_secs(1),
                    );
                }
                None => reporter.unavailable("Grok", "session store location unavailable"),
            }
        }
    }
    reporter.handler(Silent)
}

/// Bind the reservation to a fresh UUID and retain the `updates.jsonl` that UUID names
/// under `home`, then give Grok that UUID with `--session-id`. A refused bind leaves the
/// native argv untouched. A retention the server did not acknowledge is retried by the
/// reporter on its own schedule; the file's identity is checked by whoever reads it.
pub(crate) fn admit(
    reporter: &mut Reporter,
    argv: &mut Vec<OsString>,
    home: &Path,
    cwd: &Path,
    deadline: Instant,
) -> Option<GrokConversation> {
    let Ok(conversation) = super::admission::fresh_uuid() else {
        reporter.unavailable("Grok", "session identifier unavailable");
        return None;
    };
    let reference = GrokConversation {
        history: ovrcr_runtime::grok_recovery::history_path(home, cwd, &conversation),
        conversation,
    };
    if !reporter.bind(&reference.conversation, deadline, false) {
        reporter.unavailable("Grok", "session binding refused");
        return None;
    }
    reporter.retain_conversation(ConversationReference::Grok(reference.clone()), deadline);
    argv.splice(
        1..1,
        [
            OsString::from("--session-id"),
            OsString::from(&reference.conversation),
        ],
    );
    eprintln!("grok session retained for titles; resume unavailable");
    Some(reference)
}

/// Grok sends no frames to this receiver; anything that arrives is not its to read.
struct Silent;
impl reporter::Frames for Silent {
    fn frame(
        &mut self,
        _reporter: &mut Reporter,
        _input: &[u8],
        _native_root: bool,
        _deadline: Instant,
    ) -> Vec<u8> {
        reporter::IGNORED.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::reporter::scripted::{Answer, Observed, Supervisor};

    fn argv(args: &[&str]) -> Vec<OsString> {
        std::iter::once("/opt/grok/bin/grok")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn only_a_fresh_interactive_launch_in_this_directory_is_eligible() {
        for good in [
            &[][..],
            &["fix the bug"],
            &["--model", "grok-4.7", "--no-alt-screen"],
            &["--permission-mode=auto", "--", "sessions"],
        ] {
            assert!(eligible_argv(&argv(good)), "{good:?}");
        }
        for bad in [
            &["-r"][..],
            &["--resume", "title"],
            &["-c"],
            &["--continue"],
            &["-s", "01a0c579-40ff-7981-a9cb-0fe920aef561"],
            &["--session-id=01a0c579-40ff-7981-a9cb-0fe920aef561"],
            &["--fork-session"],
            &["-p", "hi"],
            &["--prompt-file", "p.txt"],
            &["--output-format", "json"],
            &["--cwd", "/elsewhere"],
            &["-w"],
            &["--worktree=feat"],
            &["--help"],
            &["sessions", "list"],
            &["--model", "grok-4.7", "export", "id"],
        ] {
            assert!(!eligible_argv(&argv(bad)), "{bad:?}");
        }
        assert!(!eligible_argv(&[OsString::from("/usr/bin/grokster")]));
        assert_eq!(
            version(b"grok 1.0.40 (eb1a2256660d) [stable]\n"),
            Some("1.0.40")
        );
        assert!(version(b"codex-cli 0.153.0\n").is_none());
        assert!(version(b"grok 1.0.40-beta (x)\n").is_none());
    }

    #[test]
    fn admission_binds_and_retains_the_documented_history_before_naming_the_session() {
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Grok);
        let mut launch = argv(&["--model", "grok-4.7"]);
        let deadline = Instant::now() + Duration::from_secs(10);
        let reference = admit(
            &mut reporter,
            &mut launch,
            Path::new("/home/u/.grok"),
            Path::new("/work/my repo"),
            deadline,
        )
        .unwrap();
        let id = reference.conversation.as_str();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4", "supervisor UUID must be canonical v4");
        assert_eq!(
            reference.history,
            std::path::PathBuf::from(format!(
                "/home/u/.grok/sessions/%2Fwork%2Fmy%20repo/{id}/updates.jsonl"
            ))
        );
        assert_eq!(
            launch,
            argv(&["--session-id", id, "--model", "grok-4.7"]),
            "the UUID Grok receives is the one retained"
        );
        assert!(!reporter.closed());
        let observed = supervisor.observed();
        let bound = observed
            .iter()
            .find_map(|event| match event {
                Observed::Bind { conversation, .. } => Some(conversation.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(bound, id);
        let retained = observed
            .iter()
            .find_map(|event| match event {
                Observed::Retain {
                    binding,
                    reference: ConversationReference::Grok(reference),
                } => Some((binding.clone(), reference.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(retained.0.provider, AgentProvider::Grok);
        assert_eq!(retained.0.conversation, id);
        assert_eq!(retained.1, reference);
    }

    #[test]
    fn a_refused_bind_leaves_the_native_arguments_alone() {
        let (mut reporter, supervisor) =
            Supervisor::scripted(AgentProvider::Grok, vec![Answer::Refuse]);
        let mut launch = argv(&["hello"]);
        assert!(
            admit(
                &mut reporter,
                &mut launch,
                Path::new("/home/u/.grok"),
                Path::new("/work"),
                Instant::now() + Duration::from_secs(10),
            )
            .is_none()
        );
        assert_eq!(launch, argv(&["hello"]));
        assert!(reporter.closed());
        assert!(
            !supervisor
                .observed()
                .iter()
                .any(|event| matches!(event, Observed::Retain { .. }))
        );
        let mut unmanaged = argv(&[]);
        let _handler = receiver(None, &mut unmanaged);
        assert_eq!(unmanaged, argv(&[]), "no reservation, no session id");
    }
}
