//! Pinned fresh Cursor CLI startup identity. No activity, metrics or recovery claims.
use super::InvocationLease;
use super::reporter::{self, Frames, Reporter};
use ovrcr_protocol::AgentProvider;
use ovrcr_runtime::agent_runner::HookHandler;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

pub const VERSION: &str = "2026.09.10-fd3934a";

pub fn supported_version(bytes: &[u8]) -> bool {
    bytes == format!("{VERSION}\n").as_bytes()
}

/// Only fresh default interactive launches and the inspected native model option.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if !matches!(
        argv.first()
            .and_then(|s| Path::new(s).file_name())
            .and_then(OsStr::to_str),
        Some("cursor-agent" | "agent")
    ) {
        return false;
    }
    match &argv[1..] {
        [] => true,
        [option, value] => {
            option == "--model"
                && value
                    .to_str()
                    .is_some_and(|s| !s.is_empty() && !s.starts_with('-'))
        }
        [option] => option
            .to_str()
            .and_then(|s| s.strip_prefix("--model="))
            .is_some_and(|s| !s.is_empty() && !s.starts_with('-')),
        _ => false,
    }
}

/// Whitelist the identity fields before forwarding a hook. Never send email, prompts,
/// transcript paths, model, workspace data or arbitrary provider fields to the server.
#[derive(Deserialize, Serialize)]
pub(super) struct Startup {
    hook_event_name: String,
    cursor_version: String,
    pub(super) conversation_id: String,
    generation_id: String,
    session_id: String,
    is_background_agent: bool,
}

impl Startup {
    fn valid(&self) -> bool {
        self.hook_event_name == "sessionStart"
            && self.cursor_version == VERSION
            && super::admission::canonical_uuid_v4(&self.conversation_id)
            && self.generation_id == self.conversation_id
            && self.session_id == self.conversation_id
            && !self.is_background_agent
    }
}

pub(super) fn startup(input: &[u8]) -> Option<Startup> {
    if input.len() > super::HOOK_INPUT_LIMIT {
        return None;
    }
    let startup: Startup = serde_json::from_slice(input).ok()?;
    startup.valid().then_some(startup)
}

/// An invocation-owned plugin only; no installation, global/project settings or auth
/// changes. Remove partial materializations too, before the native fallback is run.
fn materialize_plugin() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let binary = std::env::current_exe()?;
    let binary = binary
        .to_str()
        .ok_or_else(|| std::io::Error::other("ovrcr path is not UTF-8"))?;
    let command = format!(
        "'{}' report cursor-agent --stdin",
        binary.replace('\'', "'\"'\"'")
    );
    let identifier = ovrcr_runtime::agent_runner::private_identifier()?;
    let root = std::env::temp_dir().join(format!("ovrcr-cursor-{identifier}"));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    let result = (|| {
        for directory in [".cursor-plugin", "hooks"] {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(directory))?;
        }
        for (name, value) in [
            (
                ".cursor-plugin/plugin.json",
                serde_json::json!({"name":format!("ovrcr-reporting-{identifier}"), "version":"1.0.0"}),
            ),
            (
                "hooks/hooks.json",
                serde_json::json!({"version":1,"hooks":{"sessionStart":[{"command":command}]}}),
            ),
        ] {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join(name))?;
            file.write_all(&serde_json::to_vec(&value)?)?;
        }
        Ok(root.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler {
    let mut scratch = None;
    let unavailable = reporter::preflight(
        lease.is_some(),
        argv,
        eligible_argv,
        reporter::interactive(),
    )
    .map(str::to_owned)
    .or_else(|| {
        (!super::admission::probe_version(&argv[0])
            .as_deref()
            .is_some_and(supported_version))
        .then(|| "version probe unsupported or unavailable".to_owned())
    })
    .or_else(|| match materialize_plugin() {
        Ok(root) => {
            argv.splice(
                1..1,
                [
                    OsString::from("--plugin-dir"),
                    root.clone().into_os_string(),
                ],
            );
            scratch = Some(root);
            None
        }
        Err(error) => Some(format!("local plugin unavailable: {error}")),
    });
    let mut reporter = Reporter::new(AgentProvider::Cursor, lease, scratch);
    if let Some(reason) = unavailable {
        reporter.unavailable("Cursor", &reason);
    }
    reporter.handler(Events)
}

struct Events;
impl Frames for Events {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        #[derive(Deserialize)]
        struct Envelope {
            provider: String,
            origin: String,
            payload: Startup,
        }
        if reporter.closed() || !native_root || input.len() > super::HOOK_INPUT_LIMIT {
            return reporter::IGNORED.to_vec();
        }
        let Ok(envelope) = serde_json::from_slice::<Envelope>(input) else {
            return reporter::IGNORED.to_vec();
        };
        if envelope.provider != "cursor-agent"
            || envelope.origin != "cursor-hook"
            || !envelope.payload.valid()
        {
            return reporter::IGNORED.to_vec();
        }
        let conversation = &envelope.payload.conversation_id;
        if let Some(binding) = reporter.binding() {
            return if binding.conversation == *conversation {
                reporter::ACCEPTED
            } else {
                reporter::IGNORED
            }
            .to_vec();
        }
        reporter
            .bind_or_disable(conversation, deadline, false)
            .unwrap_or_else(|| reporter::ACCEPTED.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::reporter::scripted::{Answer, Observed, Supervisor};
    use serde_json::{Value, json};
    use std::time::Duration;

    fn payload() -> Value {
        json!({"hook_event_name":"sessionStart","cursor_version":VERSION,
            "conversation_id":"00000000-0000-4000-8000-000000000001",
            "generation_id":"00000000-0000-4000-8000-000000000001",
            "session_id":"00000000-0000-4000-8000-000000000001","is_background_agent":false})
    }

    fn frame(payload: Value) -> Vec<u8> {
        serde_json::to_vec(
            &json!({"provider":"cursor-agent","origin":"cursor-hook","payload":payload}),
        )
        .unwrap()
    }

    #[test]
    fn temporary_plugin_names_are_isolated_per_invocation() {
        let first = materialize_plugin().unwrap();
        let second = materialize_plugin().unwrap();
        let name = |root: &Path| {
            let manifest: Value = serde_json::from_slice(
                &std::fs::read(root.join(".cursor-plugin/plugin.json")).unwrap(),
            )
            .unwrap();
            manifest["name"].as_str().unwrap().to_owned()
        };
        let names = [name(&first), name(&second)];
        std::fs::remove_dir_all(first).unwrap();
        std::fs::remove_dir_all(second).unwrap();
        assert_ne!(
            names[0], names[1],
            "a local plugin must not shadow another invocation or user plugin with a fixed name"
        );
    }

    #[test]
    fn version_is_exact_not_a_guessed_compatible_range() {
        assert!(supported_version(format!("{VERSION}\n").as_bytes()));
        for invalid in [
            VERSION,
            "2026.09.11-fd3934a\n",
            "2026.09.10-fd3934a\nextra",
            "2026.09.10-fd3934a\r\n",
        ] {
            assert!(!supported_version(invalid.as_bytes()), "{invalid}");
        }
    }

    #[test]
    fn only_source_inspected_fresh_arguments_are_eligible() {
        let args = |native: &str, args: &[&str]| {
            std::iter::once(native)
                .chain(args.iter().copied())
                .map(OsString::from)
                .collect::<Vec<_>>()
        };
        for native in ["/opt/cursor-agent", "/opt/agent"] {
            for good in [&[][..], &["--model", "auto"], &["--model=auto"]] {
                assert!(eligible_argv(&args(native, good)), "{good:?}");
            }
            for bad in [
                &["--model"][..],
                &["--model="],
                &["--model", "--print"],
                &["--resume", "id"],
                &["--continue"],
                &["-p", "work"],
                &["login"],
                &["acp"],
                &["--workspace", "/tmp"],
                &["--plugin-dir", "/tmp"],
                &["--help"],
                &["--version"],
                &["work"],
                &["--force"],
                &["--mode", "ask"],
            ] {
                assert!(!eligible_argv(&args(native, bad)), "{bad:?}");
            }
        }
        assert!(!eligible_argv(&[]));
        assert!(!eligible_argv(&args("/opt/unrelated", &[])));
    }

    #[test]
    fn hook_whitelist_validates_identity_and_discards_private_fields() {
        let mut input = payload();
        input["user_email"] = "DO_NOT_FORWARD".into();
        input["transcript_path"] = "DO_NOT_FORWARD".into();
        input["model"] = "DO_NOT_FORWARD".into();
        let parsed = startup(&serde_json::to_vec(&input).unwrap()).unwrap();
        assert!(
            !serde_json::to_string(&parsed)
                .unwrap()
                .contains("DO_NOT_FORWARD")
        );
        for (key, value) in [
            ("cursor_version", json!("unknown")),
            ("hook_event_name", json!("stop")),
            ("conversation_id", json!("not-a-uuid")),
            ("generation_id", json!("other")),
            ("session_id", json!("other")),
            ("is_background_agent", json!(true)),
        ] {
            let mut bad = payload();
            bad[key] = value;
            assert!(
                startup(&serde_json::to_vec(&bad).unwrap()).is_none(),
                "{key}"
            );
        }
        assert!(startup(b"{}").is_none());
        assert!(startup(&vec![b' '; super::super::HOOK_INPUT_LIMIT + 1]).is_none());
    }

    #[test]
    fn only_native_owned_startup_binds_once_without_observations() {
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Cursor);
        let mut events = Events;
        let deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            events.frame(&mut reporter, &frame(payload()), false, deadline),
            reporter::IGNORED
        );
        assert!(reporter.binding().is_none());
        for (key, value) in [
            ("cursor_version", json!("unknown")),
            ("generation_id", json!("foreign")),
            ("is_background_agent", json!(true)),
        ] {
            let mut bad = payload();
            bad[key] = value;
            assert_eq!(
                events.frame(&mut reporter, &frame(bad), true, deadline),
                reporter::IGNORED
            );
            assert!(reporter.binding().is_none());
        }
        for (key, value) in [("provider", json!("claude")), ("origin", json!("foreign"))] {
            let mut envelope: Value = serde_json::from_slice(&frame(payload())).unwrap();
            envelope[key] = value;
            assert_eq!(
                events.frame(
                    &mut reporter,
                    &serde_json::to_vec(&envelope).unwrap(),
                    true,
                    deadline
                ),
                reporter::IGNORED
            );
        }
        assert_eq!(
            events.frame(&mut reporter, &frame(payload()), true, deadline),
            reporter::ACCEPTED
        );
        let binding = reporter.binding().unwrap().clone();
        assert_eq!(
            events.frame(&mut reporter, &frame(payload()), true, deadline),
            reporter::ACCEPTED
        );
        let mut other = payload();
        for key in ["conversation_id", "generation_id", "session_id"] {
            other[key] = "00000000-0000-4000-8000-000000000002".into();
        }
        assert_eq!(
            events.frame(&mut reporter, &frame(other), true, deadline),
            reporter::IGNORED
        );
        assert_eq!(reporter.binding(), Some(&binding));
        reporter.disable();
        let observed = supervisor.observed();
        assert_eq!(
            observed
                .iter()
                .filter(|entry| matches!(entry, Observed::Bind { .. }))
                .count(),
            1
        );
        assert!(!observed.iter().any(|entry| matches!(
            entry,
            Observed::Report(_) | Observed::Health(_) | Observed::Retain { .. }
        )));
    }

    #[test]
    fn unresolved_bind_receipt_disables_without_another_bind() {
        let (mut reporter, supervisor) = Supervisor::scripted(
            AgentProvider::Cursor,
            vec![Answer::Withhold, Answer::Withhold],
        );
        let mut events = Events;
        assert_eq!(
            events.frame(
                &mut reporter,
                &frame(payload()),
                true,
                Instant::now() + Duration::from_millis(200)
            ),
            reporter::UNAVAILABLE
        );
        assert!(reporter.closed() && reporter.binding().is_none());
        assert_eq!(
            events.frame(
                &mut reporter,
                &frame(payload()),
                true,
                Instant::now() + Duration::from_secs(1)
            ),
            reporter::IGNORED
        );
        let observed = supervisor.observed();
        assert_eq!(
            observed
                .iter()
                .filter(|entry| matches!(entry, Observed::Bind { .. }))
                .count(),
            1
        );
        assert!(!observed.iter().any(|entry| matches!(
            entry,
            Observed::Report(_) | Observed::Health(_) | Observed::Retain { .. }
        )));
    }

    #[test]
    fn refused_bind_disables_instead_of_retrying_or_fabricating_identity() {
        let (mut reporter, supervisor) =
            Supervisor::scripted(AgentProvider::Cursor, vec![Answer::Refuse]);
        let mut events = Events;
        let deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            events.frame(&mut reporter, &frame(payload()), true, deadline),
            reporter::UNAVAILABLE
        );
        assert!(reporter.closed() && reporter.binding().is_none());
        assert_eq!(
            events.frame(&mut reporter, &frame(payload()), true, deadline),
            reporter::IGNORED
        );
        // The fixture records attempted requests before refusing them. Exactly one
        // attempt, no accepted binding and no observation/retention may escape it.
        let observed = supervisor.observed();
        assert_eq!(
            observed
                .iter()
                .filter(|entry| matches!(entry, Observed::Bind { .. }))
                .count(),
            1
        );
        assert!(!observed.iter().any(|entry| matches!(
            entry,
            Observed::Report(_) | Observed::Health(_) | Observed::Retain { .. }
        )));
    }
}
