//! Immutable, credential-free creation intent. Authentication is resolved at dispatch.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudCreationIntent {
    pub key: String,
    pub account: crate::CursorCloudAccount,
    pub credential_generation: String,
    pub repository: String,
    pub source_sha: String,
}

impl CloudCreationIntent {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.account.validate()?;
        if ![&self.key, &self.credential_generation].iter().all(|v| {
            v.len() == 64
                && v.bytes()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        }) || self.source_sha.len() != 40
            || !self
                .source_sha
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            || crate::canonical_cloud_repository(&self.repository).as_ref()
                != Some(&self.repository)
            || !self.account.permits(&self.repository)
        {
            anyhow::bail!("invalid immutable cloud creation intent");
        }
        Ok(())
    }
    /// Exactly the semantic SDK request; apiKey is injected only in memory.
    pub fn request(&self) -> serde_json::Value {
        serde_json::json!({"idempotencyKey":self.key,"options":{
            "model":{"id":self.account.model},
            "cloud":{"repos":[{"url":self.repository,"startingRef":self.source_sha}],"autoCreatePr":false,"workOnCurrentBranch":false}}})
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudCreationReceipt {
    pub agent_id: String,
    pub model: String,
}
impl CloudCreationReceipt {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.agent_id.is_empty()
            || self.agent_id.len() > 256
            || !self
                .agent_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || self.model.is_empty()
            || self.model.len() > 256
            || self.model.chars().any(char::is_control)
        {
            anyhow::bail!("invalid exact cloud creation receipt");
        }
        Ok(())
    }
}

/// The full immutable semantic request crosses the authenticated admission channel;
/// its prompt is published privately, never retained in SQLite metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSendIntent {
    pub key: String,
    pub creation_key: String,
    pub agent_id: String,
    pub credential_generation: String,
    pub model: String,
    pub prompt: String,
}
impl CloudSendIntent {
    pub fn validate(&self) -> anyhow::Result<()> {
        CloudCreationReceipt {
            agent_id: self.agent_id.clone(),
            model: self.model.clone(),
        }
        .validate()?;
        if ![&self.key, &self.creation_key, &self.credential_generation]
            .iter()
            .all(|v| {
                v.len() == 64
                    && v.bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            })
            || self.key == self.creation_key
            || self.prompt.trim().is_empty()
            || self.prompt.len() > 32768
            || self.prompt.contains('\0')
        {
            anyhow::bail!("invalid immutable cloud Send intent");
        }
        Ok(())
    }
    pub fn request(&self) -> serde_json::Value {
        serde_json::json!({"agentId":self.agent_id,"message":{"text":self.prompt},"options":{"model":{"id":self.model}},"idempotencyKey":self.key})
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudRunStatus {
    Finished,
    Error,
    Cancelled,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudCreationCommand {
    Prepare(CloudCreationIntent),
    Claim {
        key: String,
    },
    Acknowledge {
        key: String,
        receipt: CloudCreationReceipt,
    },
    PrepareSend(CloudSendIntent),
    ClaimSend {
        key: String,
        credential_generation: String,
    },
    AcknowledgeSend {
        key: String,
        agent_id: String,
        run_id: String,
    },
    ObserveSend {
        key: String,
        agent_id: String,
        run_id: String,
    },
    ObservationLost {
        key: String,
        agent_id: String,
        run_id: String,
    },
    CompleteSend {
        key: String,
        agent_id: String,
        run_id: String,
        status: CloudRunStatus,
    },
}

#[cfg(test)]
pub(crate) fn fixture_intent() -> CloudCreationIntent {
    CloudCreationIntent {
        key: "a".repeat(64),
        credential_generation: "b".repeat(64),
        source_sha: "c".repeat(40),
        repository: "https://github.com/acme/app".into(),
        account: crate::CursorCloudAccount {
            name: "work".into(),
            keychain_service: "cursor".into(),
            keychain_account: "work".into(),
            model: "composer-2.5".into(),
            repositories: vec!["https://github.com/acme/app".into()],
            owners: vec![],
            bridge_archive: "/sdk.tar.gz".into(),
        },
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creation_request_matches_pinned_cloud_semantics_without_credentials_or_prompt() {
        let intent = fixture_intent();
        intent.validate().unwrap();
        assert_eq!(
            intent.request(),
            serde_json::json!({"idempotencyKey":"a".repeat(64),"options":{"model":{"id":"composer-2.5"},"cloud":{"repos":[{"url":"https://github.com/acme/app","startingRef":"c".repeat(40)}],"autoCreatePr":false,"workOnCurrentBranch":false}}})
        );
        let mut bad = intent.clone();
        bad.repository = "https://github.com/other/app".into();
        assert!(bad.validate().is_err());
        bad = intent;
        bad.key = "new key".into();
        assert!(bad.validate().is_err());
        assert!(
            CloudCreationReceipt {
                agent_id: "bad\nidentity".into(),
                model: "composer-2.5".into()
            }
            .validate()
            .is_err()
        );
    }
}
