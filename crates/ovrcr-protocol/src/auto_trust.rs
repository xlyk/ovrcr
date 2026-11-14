//! Default auto-trust flags for managed agent launches.
//!
//! Callers append one flag only when argv has none of the permission, approve,
//! auto-trust, or dangerously-* family. An existing flag in that family wins.
//!
//! Codex defaults to `--approve-for-me` (Codex CLI >=0.155). Obsolete `--full-auto`
//! remains in the family presence check so a saved argv never gets a second flag.
use crate::is_exact_agy_conversation_id;
use std::ffi::OsString;

/// The flag a managed launch adds when the caller omitted the permission family.
pub fn default_auto_trust_flag(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("--dangerously-skip-permissions"),
        "codex" => Some("--approve-for-me"),
        "pi" => Some("--approve"),
        "omp" => Some("--auto-approve"),
        "agy" => Some("--dangerously-skip-permissions"),
        _ => None,
    }
}

/// True when `argv` already names a permission, approve, auto-trust, or dangerously-* flag.
///
/// Codex's short `-a` is `--ask-for-approval`. The same token on another provider is not
/// that family. `--yolo` is Oh My Pi's alias of `--auto-approve`. Codex keeps obsolete
/// `--full-auto` in the family so old saved argv does not also receive `--approve-for-me`.
fn auto_trust_family_present(provider: &str, argv: &[OsString]) -> bool {
    argv.iter().take_while(|arg| *arg != "--").any(|arg| {
        let Some(text) = arg.to_str() else {
            return false;
        };
        let name = text.split_once('=').map(|(name, _)| name).unwrap_or(text);
        name.starts_with("--dangerously-")
            || matches!(
                name,
                "--permission-mode"
                    | "--allow-dangerously-skip-permissions"
                    | "--approve"
                    | "--no-approve"
                    | "--auto-approve"
                    | "--yolo"
                    | "--approval-mode"
                    | "--approve-for-me"
                    | "--full-auto"
                    | "--ask-for-approval"
            )
            || (provider == "codex" && name == "-a")
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgyTrustDecision {
    Unsupported,
    ExplicitPermissionChoice,
    AddDefault,
}

fn agy_trust_decision(argv: &[OsString]) -> AgyTrustDecision {
    if argv.is_empty() || argv.iter().any(|arg| arg.to_str().is_none()) {
        return AgyTrustDecision::Unsupported;
    }

    let mut args = argv.iter().skip(1);
    let mut conversation_seen = false;
    let mut prompt_seen = false;
    let mut explicit_permission_choice = false;
    while let Some(arg) = args.next().and_then(|arg| arg.to_str()) {
        if arg == "--" {
            // A trailing terminator is harmless, but Agy has no positional-prompt
            // grammar. Never reinterpret or trust trailing positional arguments.
            if args.next().is_some() {
                return AgyTrustDecision::Unsupported;
            }
            return if explicit_permission_choice {
                AgyTrustDecision::ExplicitPermissionChoice
            } else {
                AgyTrustDecision::AddDefault
            };
        }
        let (name, inline_value) = arg
            .split_once('=')
            .map_or((arg, None), |(name, value)| (name, Some(value)));
        match name {
            // Consume values so names such as "models" are not mistaken for
            // subcommands or options.
            "--conversation" => {
                if conversation_seen {
                    return AgyTrustDecision::Unsupported;
                }
                conversation_seen = true;
                let Some(value) = agy_option_value(inline_value, &mut args, false) else {
                    return AgyTrustDecision::Unsupported;
                };
                if !is_exact_agy_conversation_id(value) {
                    return AgyTrustDecision::Unsupported;
                }
            }
            "--prompt-interactive" | "-i" => {
                if prompt_seen {
                    return AgyTrustDecision::Unsupported;
                }
                prompt_seen = true;
                if agy_option_value(inline_value, &mut args, true).is_none() {
                    return AgyTrustDecision::Unsupported;
                }
            }
            "--add-dir" | "--agent" | "--effort" | "--model" => {
                if agy_option_value(inline_value, &mut args, false).is_none() {
                    return AgyTrustDecision::Unsupported;
                }
            }
            "--continue" | "-c" | "--new-project" => {
                if inline_value.is_some() {
                    return AgyTrustDecision::Unsupported;
                }
            }
            // Explicit permission choices suppress the managed bypass default.
            "--mode" => {
                let Some(value) = agy_option_value(inline_value, &mut args, false) else {
                    return AgyTrustDecision::Unsupported;
                };
                if !matches!(value, "accept-edits" | "auto" | "plan") {
                    return AgyTrustDecision::Unsupported;
                }
                explicit_permission_choice = true;
            }
            "--sandbox" => {
                if !matches!(inline_value, None | Some("true" | "false")) {
                    return AgyTrustDecision::Unsupported;
                }
                explicit_permission_choice = true;
            }
            "--dangerously-skip-permissions" => {
                if inline_value.is_some() {
                    return AgyTrustDecision::Unsupported;
                }
                explicit_permission_choice = true;
            }
            // Settings, remote, print, and native management commands do not
            // receive managed interactive defaults.
            "--settings"
            | "--project"
            | "--remote-agent"
            | "--remote-control"
            | "--print"
            | "--prompt"
            | "-p"
            | "--input-format"
            | "--output-format"
            | "--print-timeout"
            | "--json-schema"
            | "--disable-slash-commands" => {
                return AgyTrustDecision::Unsupported;
            }
            // A positional token selects a subcommand or an unsupported mode.
            // Unknown flags also fail closed instead of gaining trust.
            _ => return AgyTrustDecision::Unsupported,
        }
    }
    if explicit_permission_choice {
        AgyTrustDecision::ExplicitPermissionChoice
    } else {
        AgyTrustDecision::AddDefault
    }
}

fn agy_option_value<'a>(
    inline_value: Option<&'a str>,
    args: &mut impl Iterator<Item = &'a OsString>,
    allow_leading_hyphen: bool,
) -> Option<&'a str> {
    let value = match inline_value {
        Some(value) => value,
        None => args.next()?.to_str()?,
    };
    (!value.is_empty() && (allow_leading_hyphen || !value.starts_with('-'))).then_some(value)
}

/// Append [`default_auto_trust_flag`] unless that family is already present.
///
/// The flag is inserted before a `--` prompt separator so admission still sees
/// one prompt. Existing flags are never replaced or duplicated.
pub fn apply_default_auto_trust(provider: &str, argv: &mut Vec<OsString>) {
    let Some(flag) = default_auto_trust_flag(provider) else {
        return;
    };
    if argv.is_empty() || argv.iter().any(|arg| arg.to_str().is_none()) {
        return;
    }
    if provider == "agy" {
        if agy_trust_decision(argv) != AgyTrustDecision::AddDefault {
            return;
        }
    } else if auto_trust_family_present(provider, argv) {
        return;
    }
    let flag = OsString::from(flag);
    if provider == "agy" {
        // Insert after the executable. Agy accepts options before or after native
        // selectors, and this avoids mistaking an opaque prompt value of `--` for
        // a top-level separator.
        argv.insert(1, flag);
        return;
    }
    if let Some(index) = argv.iter().position(|arg| arg == "--") {
        argv.insert(index, flag);
    } else {
        argv.push(flag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn bare_managed_launches_gain_one_provider_flag() {
        let cases = [
            ("claude", "--dangerously-skip-permissions"),
            ("codex", "--approve-for-me"),
            ("pi", "--approve"),
            ("omp", "--auto-approve"),
            ("agy", "--dangerously-skip-permissions"),
        ];
        for (provider, flag) in cases {
            let mut argv = args(&["/opt/bin/agent"]);
            apply_default_auto_trust(provider, &mut argv);
            assert_eq!(argv, args(&["/opt/bin/agent", flag]), "{provider}");
        }
    }

    #[test]
    fn providers_without_an_equivalent_are_left_alone() {
        for provider in ["grok", "hermes", "cursor-agent", "gemini", "shell"] {
            let mut argv = args(&["/opt/bin/agent"]);
            apply_default_auto_trust(provider, &mut argv);
            assert_eq!(argv, args(&["/opt/bin/agent"]), "{provider}");
        }
    }

    #[test]
    fn an_existing_permission_family_flag_is_not_appended_or_replaced() {
        let cases: &[(&str, &[&str])] = &[
            ("claude", &["claude", "--permission-mode", "dontAsk"]),
            ("claude", &["claude", "--permission-mode=plan"]),
            ("claude", &["claude", "--dangerously-skip-permissions"]),
            (
                "claude",
                &["claude", "--allow-dangerously-skip-permissions"],
            ),
            ("codex", &["codex", "--approve-for-me"]),
            ("codex", &["codex", "--full-auto"]),
            ("codex", &["codex", "--ask-for-approval", "on-request"]),
            ("codex", &["codex", "-a", "never"]),
            ("codex", &["codex", "--dangerously-bypass-hook-trust"]),
            ("pi", &["pi", "--approve"]),
            ("pi", &["pi", "--no-approve"]),
            ("omp", &["omp", "--auto-approve"]),
            ("omp", &["omp", "--yolo"]),
            ("omp", &["omp", "--approval-mode", "always-ask"]),
            ("agy", &["agy", "--mode", "plan"]),
            ("agy", &["agy", "--mode", "accept-edits"]),
            ("agy", &["agy", "--mode=auto"]),
            ("agy", &["agy", "--sandbox"]),
            ("agy", &["agy", "--sandbox=false"]),
            ("agy", &["agy", "--dangerously-skip-permissions"]),
        ];
        for (provider, original) in cases {
            let mut argv = args(original);
            apply_default_auto_trust(provider, &mut argv);
            assert_eq!(argv, args(original), "{provider} {original:?}");
        }
    }

    #[test]
    fn the_trust_flag_stays_before_a_prompt_separator() {
        let mut argv = args(&["claude", "--model", "sonnet", "--", "ship it"]);
        apply_default_auto_trust("claude", &mut argv);
        assert_eq!(
            argv,
            args(&[
                "claude",
                "--model",
                "sonnet",
                "--dangerously-skip-permissions",
                "--",
                "ship it"
            ])
        );
    }

    #[test]
    fn unrelated_flags_do_not_count_as_the_permission_family() {
        let mut argv = args(&["codex", "--model", "gpt-5", "--sandbox", "workspace-write"]);
        apply_default_auto_trust("codex", &mut argv);
        assert_eq!(
            argv,
            args(&[
                "codex",
                "--model",
                "gpt-5",
                "--sandbox",
                "workspace-write",
                "--approve-for-me"
            ])
        );
        let mut claude = args(&["claude", "-a"]);
        apply_default_auto_trust("claude", &mut claude);
        assert_eq!(
            claude,
            args(&["claude", "-a", "--dangerously-skip-permissions"])
        );
    }

    #[test]
    fn agy_trust_defaults_only_to_interactive_launches_and_prompt_text_is_not_an_option() {
        let mut interactive = args(&["agy", "--model", "gemini-3.8-flash-low"]);
        apply_default_auto_trust("agy", &mut interactive);
        assert_eq!(
            interactive,
            args(&[
                "agy",
                "--dangerously-skip-permissions",
                "--model",
                "gemini-3.8-flash-low"
            ])
        );

        for argv in [
            args(&["agy", "-p", "hello"]),
            args(&["agy", "--prompt=hello"]),
            args(&["agy", "models"]),
            args(&["agy", "acp"]),
            args(&["agy", "agent"]),
            args(&["agy", "agents"]),
            args(&["agy", "changelog"]),
            args(&["agy", "help"]),
            args(&["agy", "install"]),
            args(&["agy", "mcp"]),
            args(&["agy", "mic-serve"]),
            args(&["agy", "plugin"]),
            args(&["agy", "plugins"]),
            args(&["agy", "remote-control", "status"]),
            args(&["agy", "update"]),
            args(&["agy", "--help"]),
            args(&["agy", "--version"]),
            args(&["agy", "--remote-agent"]),
            args(&["agy", "--remote-control"]),
            args(&["agy", "--settings"]),
            args(&["agy", "--settings", "settings.json"]),
            args(&["agy", "--settings=settings.json"]),
            args(&["agy", "--project", "local"]),
            args(&[
                "agy",
                "--conversation",
                "01a08e5c-7480-7052-9964-9224aadebef0",
                "--conversation",
                "01a08e5c-7480-7052-9964-9224aadebef0",
            ]),
            args(&[
                "agy",
                "--prompt-interactive=one",
                "--prompt-interactive=two",
            ]),
            args(&["agy", "--prompt-interactive="]),
            args(&["agy", "--conversation", "not-a-uuid"]),
            args(&["agy", "--unknown-option"]),
        ] {
            let mut argv = argv.clone();
            let original = argv.clone();
            apply_default_auto_trust("agy", &mut argv);
            assert_eq!(argv, original);
        }

        let mut option_value_named_like_a_subcommand = args(&["agy", "--model", "models"]);
        apply_default_auto_trust("agy", &mut option_value_named_like_a_subcommand);
        assert_eq!(
            option_value_named_like_a_subcommand,
            args(&["agy", "--model", "models", "--dangerously-skip-permissions"])
        );

        let mut exact_resume =
            args(&["agy", "--conversation=01a08e5c-7480-7052-9964-9224aadebef0"]);
        apply_default_auto_trust("agy", &mut exact_resume);
        assert_eq!(
            exact_resume,
            args(&[
                "agy",
                "--conversation=01a08e5c-7480-7052-9964-9224aadebef0",
                "--dangerously-skip-permissions"
            ])
        );

        let mut standalone_separator = args(&["agy", "--"]);
        apply_default_auto_trust("agy", &mut standalone_separator);
        assert_eq!(
            standalone_separator,
            args(&["agy", "--dangerously-skip-permissions", "--"])
        );

        for prompt in [
            args(&["agy", "--prompt-interactive", "--"]),
            args(&["agy", "--prompt-interactive=--"]),
            args(&["agy", "--prompt-interactive=--mode=plan"]),
            args(&["agy", "-i", "--mode"]),
        ] {
            let expected = match prompt.as_slice() {
                [_, selector, value] if selector == "--prompt-interactive" || selector == "-i" => {
                    args(&[
                        "agy",
                        "--dangerously-skip-permissions",
                        selector.to_str().unwrap(),
                        value.to_str().unwrap(),
                    ])
                }
                [_, selector] if selector.to_str().unwrap().starts_with("--prompt-interactive=") => {
                    args(&[
                        "agy",
                        "--dangerously-skip-permissions",
                        selector.to_str().unwrap(),
                    ])
                }
                _ => unreachable!("prompt test has a native prompt selector"),
            };
            let mut argv = prompt;
            apply_default_auto_trust("agy", &mut argv);
            assert_eq!(argv, expected);
        }

        let mut unsupported_positionals = args(&["agy", "--", "--mode=plan"]);
        let original = unsupported_positionals.clone();
        apply_default_auto_trust("agy", &mut unsupported_positionals);
        assert_eq!(unsupported_positionals, original);
    }

    #[test]
    fn non_utf8_argv_is_left_untouched() {
        use std::os::unix::ffi::OsStringExt;
        let mut argv = vec![OsString::from("claude"), OsString::from_vec(vec![0xff])];
        let original = argv.clone();
        apply_default_auto_trust("claude", &mut argv);
        assert_eq!(argv, original);
    }
}
