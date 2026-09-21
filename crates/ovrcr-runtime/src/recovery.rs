//! Shared provider capability and launch boundary. Adapters only validate saved
//! references and construct argv; ServerState remains the sole process owner.
use anyhow::{Result, bail};
use ovrcr_protocol::{AgentProvider, ConversationReference};
use std::ffi::OsString;

pub fn supported(provider: AgentProvider) -> bool {
    matches!(
        provider,
        AgentProvider::Claude | AgentProvider::Pi | AgentProvider::Omp | AgentProvider::Codex
    )
}

pub fn unavailable(
    name: &str,
    reference: Option<&ConversationReference>,
    invalid: bool,
) -> Option<String> {
    let Some(provider) = AgentProvider::from_name(name).filter(|provider| supported(*provider))
    else {
        return Some(format!("Native resume is not available for {name}"));
    };
    if invalid {
        return Some(format!(
            "{name} conversation changed through an unsupported identity transition; start a new conversation in a separate session"
        ));
    }
    match reference {
        Some(ConversationReference::Pi(reference) | ConversationReference::Omp(reference))
            if reference.history.is_none() =>
        {
            Some("Conversation has no native history file; resume is unavailable".into())
        }
        Some(ConversationReference::Codex(reference))
            if !crate::codex_recovery::is_exact_identity(&reference.conversation) =>
        {
            Some("Codex recovery requires an exact canonical UUID".into())
        }
        Some(ConversationReference::Codex(reference)) if reference.history.is_none() => Some(
            "Codex conversation has no verified native history file; resume is unavailable".into(),
        ),
        Some(reference) if reference.provider() == provider => None,
        Some(_) => Some("Retained conversation provider does not match this session".into()),
        None => None,
    }
}

pub fn validate(reference: &ConversationReference) -> Result<()> {
    match reference {
        ConversationReference::Claude(reference) => crate::claude_recovery::validate(reference),
        ConversationReference::Codex(reference) => crate::codex_recovery::validate(reference),
        ConversationReference::Pi(reference) => {
            crate::extension_recovery::validate(AgentProvider::Pi, reference)
        }
        ConversationReference::Omp(reference) => {
            crate::extension_recovery::validate(AgentProvider::Omp, reference)
        }
    }
}

pub fn resume_argv(name: &str, reference: &ConversationReference) -> Result<Vec<OsString>> {
    if let Some(reason) = unavailable(name, Some(reference), false) {
        bail!("{reason}");
    }
    match reference {
        ConversationReference::Claude(reference) => crate::claude_recovery::resume_argv(reference),
        ConversationReference::Codex(reference) => crate::codex_recovery::resume_argv(reference),
        ConversationReference::Pi(reference) => {
            crate::extension_recovery::resume_argv(AgentProvider::Pi, reference)
        }
        ConversationReference::Omp(reference) => {
            crate::extension_recovery::resume_argv(AgentProvider::Omp, reference)
        }
    }
}

/// Without a retained identity, let the native agent offer its own history picker.
pub fn picker_argv(name: &str) -> Result<Vec<OsString>> {
    let provider = AgentProvider::from_name(name)
        .filter(|provider| supported(*provider))
        .ok_or_else(|| anyhow::anyhow!("Native resume is not available for {name}"))?;
    Ok(vec![
        provider.name().into(),
        match provider {
            AgentProvider::Codex => "resume",
            _ => "--resume",
        }
        .into(),
    ])
}

/// Submit one literal command line to the interactive terminal, then press Enter.
pub fn terminal_command(argv: &[OsString]) -> Result<String> {
    let words = argv
        .iter()
        .map(|arg| {
            let arg = arg
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("resume command is not UTF-8"))?;
            if arg.chars().any(char::is_control) {
                bail!("resume command contains terminal control characters");
            }
            Ok(format!("'{}'", arg.replace('\'', "'\\''")))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("{}\n", words.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_commands_are_explicit_and_terminal_arguments_are_literal() {
        for (provider, command) in [
            ("claude", "'claude' '--resume'\n"),
            ("codex", "'codex' 'resume'\n"),
            ("pi", "'pi' '--resume'\n"),
            ("omp", "'omp' '--resume'\n"),
        ] {
            assert_eq!(
                terminal_command(&picker_argv(provider).unwrap()).unwrap(),
                command
            );
        }
        assert!(picker_argv("unrecognized; exit").is_err());
        let command =
            terminal_command(&["printf".into(), "%s".into(), "a'b $(false); hi".into()]).unwrap();
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"a'b $(false); hi");
        assert!(terminal_command(&["bad\ncommand".into()]).is_err());
        assert!(terminal_command(&["bad\x1bcommand".into()]).is_err());
    }

    #[test]
    fn only_installed_adapter_is_eligible_and_provider_mismatch_fails_closed() {
        use ovrcr_protocol::ClaudeConversation;
        let reference = ConversationReference::Claude(ClaudeConversation {
            conversation: "5ebc5f9b-54b5-4928-9955-dc81c23743dd".into(),
            executable: "/bin/claude".into(),
            history: "/history".into(),
            config_dir: "/config".into(),
            options: vec![],
        });
        assert!(unavailable("claude", Some(&reference), false).is_none());
        assert!(unavailable("claude", None, false).is_none());
        assert!(
            unavailable("claude", Some(&reference), true)
                .unwrap()
                .contains("unsupported")
        );
        for provider in [AgentProvider::Grok, AgentProvider::Hermes] {
            assert!(!supported(provider));
            assert!(
                unavailable(provider.name(), Some(&reference), false)
                    .unwrap()
                    .contains("not available")
            );
            assert!(
                resume_argv(provider.name(), &reference)
                    .unwrap_err()
                    .to_string()
                    .contains("not available")
            );
        }
    }
}
