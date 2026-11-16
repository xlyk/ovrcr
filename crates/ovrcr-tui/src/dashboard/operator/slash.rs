//! Slash command disambiguation and built-in Operator command registry.

use ovrcr_protocol::{OperatorReasoning, OperatorThinkingLevel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SlashCommandKind {
    Copy,
    Thinking,
    Model,
    Skills,
    New,
    Session,
    Compact,
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SlashCommandSpec {
    pub kind: SlashCommandKind,
    pub name: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
    pub takes_args: bool,
}

pub(super) const COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        kind: SlashCommandKind::Copy,
        name: "copy",
        usage: "/copy",
        description: "Copy latest assistant reply Markdown",
        takes_args: false,
    },
    SlashCommandSpec {
        kind: SlashCommandKind::Thinking,
        name: "thinking",
        usage: "/thinking [level]",
        description: "Choose or set reasoning effort level",
        takes_args: true,
    },
    SlashCommandSpec {
        kind: SlashCommandKind::Model,
        name: "model",
        usage: "/model [provider/id | default]",
        description: "Choose or set Operator model",
        takes_args: true,
    },
    SlashCommandSpec {
        kind: SlashCommandKind::New,
        name: "new",
        usage: "/new",
        description: "Start a fresh Operator session epoch",
        takes_args: false,
    },
    SlashCommandSpec {
        kind: SlashCommandKind::Session,
        name: "session",
        usage: "/session",
        description: "Inspect Operator session stats and usage",
        takes_args: false,
    },
    SlashCommandSpec {
        kind: SlashCommandKind::Compact,
        name: "compact",
        usage: "/compact [instructions]",
        description: "Compact Operator conversation context",
        takes_args: true,
    },
    SlashCommandSpec {
        kind: SlashCommandKind::Tree,
        name: "tree",
        usage: "/tree",
        description: "Inspect or switch Operator conversation branch",
        takes_args: false,
    },
    SlashCommandSpec {
        kind: SlashCommandKind::Skills,
        name: "skills",
        usage: "/skills [request]",
        description: "Browse shipped skills; prepare without sending",
        takes_args: true,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ParsedDraft<'a> {
    Prompt,
    Skill { name: &'a str, request: &'a str },
    Slash { name: &'a str, args: &'a str },
}

/// Inspects index `0` of `draft`: a single-segment `/token` (no leading
/// whitespace and no second `/` before whitespace or end-of-input) is parsed as
/// a slash command; multi-segment paths (`/etc/hosts`, `/Users/...`) and
/// leading-whitespace drafts (` /copy`) pass through as ordinary prompts.
/// `/skill:` is reserved even when its identity is malformed, never a path.
pub(super) fn parse_draft(draft: &str) -> ParsedDraft<'_> {
    let Some(rest) = draft.strip_prefix('/') else {
        return ParsedDraft::Prompt;
    };
    let split_at = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..split_at];
    let args = rest[split_at..].trim();
    if let Some(name) = name.strip_prefix("skill:") {
        return ParsedDraft::Skill {
            name,
            request: args,
        };
    }
    if name.contains('/') {
        return ParsedDraft::Prompt;
    }
    ParsedDraft::Slash { name, args }
}

pub(super) fn find_command(name: &str) -> Option<&'static SlashCommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
}

/// Matching commands for the live `/` popup in the full Operator composer.
/// Active only when editing a leading single-segment `/prefix` with no space.
pub(super) fn menu_candidates(draft: &str) -> Vec<&'static SlashCommandSpec> {
    let Some(prefix) = draft.strip_prefix('/') else {
        return Vec::new();
    };
    if prefix.contains('/') || prefix.chars().any(char::is_whitespace) {
        return Vec::new();
    }
    let lower = prefix.to_ascii_lowercase();
    COMMANDS
        .iter()
        .filter(|spec| spec.name.starts_with(&lower))
        .collect()
}

pub(super) fn completion_text(spec: &SlashCommandSpec) -> String {
    if spec.takes_args {
        format!("/{} ", spec.name)
    } else {
        format!("/{}", spec.name)
    }
}

pub(super) fn available_names() -> String {
    COMMANDS
        .iter()
        .map(|spec| format!("/{}", spec.name))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn unknown_command_error(name: &str) -> String {
    let token = if name.is_empty() {
        "/".to_owned()
    } else {
        format!("/{name}")
    };
    format!(
        "Unknown slash command \"{token}\". Available: {}.",
        format_args!("{}, /skill:<name> [request]", available_names())
    )
}

pub(super) fn parse_thinking_level(arg: &str) -> Option<OperatorThinkingLevel> {
    OperatorThinkingLevel::ALL
        .into_iter()
        .find(|level| level.as_str().eq_ignore_ascii_case(arg))
}

pub(super) fn allowed_thinking_levels(
    reasoning: Option<&OperatorReasoning>,
) -> Vec<OperatorThinkingLevel> {
    match reasoning {
        Some(reasoning) => {
            let mut choices = vec![OperatorThinkingLevel::Default];
            choices.extend(
                OperatorThinkingLevel::ALL
                    .into_iter()
                    .skip(1)
                    .filter(|level| reasoning.available.contains(level)),
            );
            choices
        }
        None => OperatorThinkingLevel::ALL.to_vec(),
    }
}

pub(super) fn thinking_level_error(
    arg: &str,
    reasoning: Option<&OperatorReasoning>,
    recognized: bool,
) -> String {
    let valid = allowed_thinking_levels(reasoning)
        .into_iter()
        .map(OperatorThinkingLevel::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if recognized {
        format!("Unsupported thinking level \"{arg}\" (valid: {valid}).")
    } else {
        format!("Invalid thinking level \"{arg}\" (valid: {valid}).")
    }
}
