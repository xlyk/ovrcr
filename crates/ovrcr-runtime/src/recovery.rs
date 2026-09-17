//! Shared provider capability and launch boundary. Adapters only validate saved
//! references and construct argv; ServerState remains the sole process owner.
use anyhow::{Result, bail};
use ovrcr_protocol::{AgentProvider, ConversationReference};
use std::ffi::OsString;

pub fn supported(provider: AgentProvider) -> bool {
    matches!(provider, AgentProvider::Claude)
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
        Some(reference) if reference.provider() == provider => None,
        Some(_) => Some("Retained conversation provider does not match this session".into()),
        None => Some(format!(
            "No certified {name} conversation and recoverable configuration; use managed launch and configured reporting"
        )),
    }
}

pub fn validate(reference: &ConversationReference) -> Result<()> {
    match reference {
        ConversationReference::Claude(reference) => crate::claude_recovery::validate(reference),
    }
}

pub fn resume_argv(name: &str, reference: &ConversationReference) -> Result<Vec<OsString>> {
    if let Some(reason) = unavailable(name, Some(reference), false) {
        bail!("{reason}");
    }
    match reference {
        ConversationReference::Claude(reference) => crate::claude_recovery::resume_argv(reference),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(
            unavailable("claude", None, false)
                .unwrap()
                .contains("No certified")
        );
        assert!(
            unavailable("claude", Some(&reference), true)
                .unwrap()
                .contains("unsupported")
        );
        for provider in [
            AgentProvider::Codex,
            AgentProvider::Pi,
            AgentProvider::Omp,
            AgentProvider::Grok,
            AgentProvider::Hermes,
        ] {
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
