//! Immutable OVRCR-shipped procedures; no discovery or runtime authority.
use serde::{Deserialize, Serialize};

/// The complete owned bundle. Only these exact documents are advertised;
/// supporting files are relative to their skill directory, never discovered.
pub struct BundledSkill {
    pub name: &'static str,
    /// Bump whenever instructions, metadata or supporting assets change.
    pub revision: u32,
    pub document: &'static str,
    pub assets: &'static [(&'static str, &'static str)],
}

pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        revision: 1,
        name: "ovrcr-status",
        document: include_str!("../../../../pi/skills/ovrcr-status/SKILL.md"),
        assets: &[(
            "references/inventory.md",
            include_str!("../../../../pi/skills/ovrcr-status/references/inventory.md"),
        )],
    },
    BundledSkill {
        revision: 1,
        name: "ovrcr-coordinator",
        document: include_str!("../../../../pi/skills/ovrcr-coordinator/SKILL.md"),
        assets: &[],
    },
    BundledSkill {
        revision: 1,
        name: "ovrcr-workspaces",
        document: include_str!("../../../../pi/skills/ovrcr-workspaces/SKILL.md"),
        assets: &[],
    },
    BundledSkill {
        revision: 2,
        name: "ovrcr-session-care",
        document: include_str!("../../../../pi/skills/ovrcr-session-care/SKILL.md"),
        assets: &[],
    },
    BundledSkill {
        revision: 4,
        name: "ovrcr-cleanup",
        document: include_str!("../../../../pi/skills/ovrcr-cleanup/SKILL.md"),
        assets: &[],
    },
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSkillSelection {
    pub name: String,
    pub revision: u32,
}

impl BundledSkill {
    /// The owned single-line frontmatter is shared by delivery and discovery.
    pub fn description(&self) -> Option<&'static str> {
        let prefix = format!("---\nname: {}\ndescription: ", self.name);
        let (description, body) = self.document.strip_prefix(&prefix)?.split_once("\n---\n")?;
        (!description.trim().is_empty()
            && description.len() <= 1024
            && !description.contains('\n')
            && !body.trim().is_empty())
        .then_some(description)
    }

    pub fn selection(&self) -> OperatorSkillSelection {
        OperatorSkillSelection {
            name: self.name.into(),
            revision: self.revision,
        }
    }
}

impl OperatorSkillSelection {
    pub fn named(name: &str) -> Result<Self, String> {
        BUNDLED_SKILLS
            .iter()
            .find(|skill| skill.name == name)
            .map(BundledSkill::selection)
            .ok_or_else(|| "Unknown or unavailable Operator skill; request kept unsent.".into())
    }

    pub fn resolve(&self) -> Result<&'static BundledSkill, String> {
        BUNDLED_SKILLS
            .iter()
            .find(|skill| skill.name == self.name && skill.revision == self.revision)
            .ok_or_else(|| "Operator skill is unavailable or incompatible; select it again.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OperatorContext, OperatorConversationId, OperatorMessageContent, OperatorRequest,
        SessionRunId,
    };

    #[test]
    fn shipped_discovery_uses_the_validated_document_purpose() {
        let status = OperatorSkillSelection::named("ovrcr-status")
            .unwrap()
            .resolve()
            .unwrap();
        assert!(
            status
                .description()
                .unwrap()
                .starts_with("Summarize OVRCR inventory, attention items")
        );
        for skill in BUNDLED_SKILLS {
            assert!(
                skill.description().is_some(),
                "{} has no discoverable purpose",
                skill.name
            );
            assert_eq!(skill.selection().resolve().unwrap().name, skill.name);
        }
        for document in [
            "---\nname: other\ndescription: valid\n---\nbody",
            "---\nname: invalid\ndescription: \n---\nbody",
            "---\nname: invalid\ndescription: line\nextra\n---\nbody",
            "---\nname: invalid\ndescription: valid\n---\n",
        ] {
            let invalid = BundledSkill {
                name: "invalid",
                revision: 1,
                document,
                assets: &[],
            };
            assert!(invalid.description().is_none());
        }
    }

    #[test]
    fn explicit_selection_is_exact_and_legacy_messages_remain_ordinary() {
        let selection = OperatorSkillSelection::named("ovrcr-status").unwrap();
        let mut request = OperatorRequest::Submit {
            conversation: OperatorConversationId(1),
            expected_run: SessionRunId(1),
            text: String::new(),
            context: OperatorContext::default(),
            skill: Some(selection.clone()),
        };
        assert!(
            request.validate().is_ok(),
            "bare explicit invocation is valid"
        );
        for (name, revision) in [
            ("OVRCR-status", 1),
            ("ovrcr-status", 0),
            ("ovrcr-status", 999),
            ("../ovrcr-status", 1),
        ] {
            let OperatorRequest::Submit { skill, .. } = &mut request else {
                unreachable!()
            };
            *skill = Some(OperatorSkillSelection {
                name: name.into(),
                revision,
            });
            assert!(request.validate().is_err(), "no identity substitution");
        }
        let legacy = serde_json::json!({"User": {"text": "/skill:ovrcr-status remains legacy data", "context": OperatorContext::default()}});
        assert!(matches!(
            serde_json::from_value::<OperatorMessageContent>(legacy).unwrap(),
            OperatorMessageContent::User { skill: None, .. }
        ));
        let bytes = bincode::serde::encode_to_vec(&selection, bincode::config::standard()).unwrap();
        assert_eq!(bytes, b"\x0covrcr-status\x01");
        let (decoded, used): (OperatorSkillSelection, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(decoded, selection);
        assert_eq!(used, bytes.len());
    }
}
