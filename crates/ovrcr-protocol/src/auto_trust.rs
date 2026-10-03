//! Default auto-trust flags for managed Claude, Codex, Pi, and Oh My Pi launches.
//!
//! Callers append one flag only when argv has none of the permission, approve,
//! full-auto, or dangerously-* family. An existing flag in that family wins.
use std::ffi::OsString;

/// The flag a managed launch adds when the caller omitted the permission family.
pub fn default_auto_trust_flag(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("--dangerously-skip-permissions"),
        "codex" => Some("--full-auto"),
        "pi" => Some("--approve"),
        "omp" => Some("--auto-approve"),
        _ => None,
    }
}

/// True when `argv` already names a permission, approve, full-auto, or dangerously-* flag.
///
/// Codex's short `-a` is `--ask-for-approval`. The same token on another provider is not
/// that family. `--yolo` is Oh My Pi's alias of `--auto-approve`.
fn auto_trust_family_present(provider: &str, argv: &[OsString]) -> bool {
    argv.iter().any(|arg| {
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
                    | "--full-auto"
                    | "--ask-for-approval"
            )
            || (provider == "codex" && name == "-a")
    })
}

/// Append [`default_auto_trust_flag`] unless that family is already present.
///
/// The flag is inserted before a `--` prompt separator so admission still sees
/// one prompt. Existing flags are never replaced or duplicated.
pub fn apply_default_auto_trust(provider: &str, argv: &mut Vec<OsString>) {
    let Some(flag) = default_auto_trust_flag(provider) else {
        return;
    };
    if argv.is_empty()
        || argv.iter().any(|arg| arg.to_str().is_none())
        || auto_trust_family_present(provider, argv)
    {
        return;
    }
    let flag = OsString::from(flag);
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
            ("codex", "--full-auto"),
            ("pi", "--approve"),
            ("omp", "--auto-approve"),
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
            ("codex", &["codex", "--full-auto"]),
            ("codex", &["codex", "--ask-for-approval", "on-request"]),
            ("codex", &["codex", "-a", "never"]),
            ("codex", &["codex", "--dangerously-bypass-hook-trust"]),
            ("pi", &["pi", "--approve"]),
            ("pi", &["pi", "--no-approve"]),
            ("omp", &["omp", "--auto-approve"]),
            ("omp", &["omp", "--yolo"]),
            ("omp", &["omp", "--approval-mode", "always-ask"]),
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
                "--full-auto"
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
    fn non_utf8_argv_is_left_untouched() {
        use std::os::unix::ffi::OsStringExt;
        let mut argv = vec![OsString::from("claude"), OsString::from_vec(vec![0xff])];
        let original = argv.clone();
        apply_default_auto_trust("claude", &mut argv);
        assert_eq!(argv, original);
    }
}
