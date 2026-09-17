use super::{
    AppResult, RuntimeError,
    agent_setup::{binary, quote, with_version_probe},
};
use serde_json::json;
use std::{ffi::OsStr, io::Read, path::Path};
use toml::Value;

const HOOKS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "Interrupt",
    "SessionEnd",
];
const REQUIREMENTS: &str = "Requires exact Codex CLI 0.153.0 and synchronous direct-exec command hooks. Review and trust these hooks in native Codex before the first tracked prompt; an initial prompt supplied during hook review may run untracked. Configuration presence does not prove hook trust or delivery. Hooks exit successfully with empty stdout (native no-op), never an approval decision. Inside an OVRCR terminal run: ovrcr agent run codex -- codex. Exact Codex CLI 0.153.0 hooks-only support passed acceptance. Managed root startup/prompt hooks also retain exact conversation identity for terminal reopen. Recovery uses codex resume UUID without a prompt; reporting stays unavailable for that resumed invocation. Native recovery acceptance is tracked separately in issue #128.";
const FORMS: &str = "Fresh interactive codex only (executable basename codex): optional --no-alt-screen, --full-auto; separate-token --model/-m, --profile/-p, --sandbox/-s, --ask-for-approval/-a, --cd/-C followed by a nonempty value not starting with '-'; at most one prompt (use -- before a prompt matching a subcommand). Resume, fork, picker, exec, remote, unknown options and other versions run natively with reporting unavailable.";

fn settings(path: Option<&Path>) -> anyhow::Result<Value> {
    let Some(path) = path else {
        return Ok(Value::Table(Default::default()));
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1_048_576, "settings file exceeds 1 MiB");
    Ok(toml::from_str(std::str::from_utf8(&bytes)?)?)
}
fn command() -> anyhow::Result<String> {
    Ok(format!("exec {} report codex --stdin", quote(&binary()?)))
}
fn reporter(group: &Value, event: &str, command: &str) -> bool {
    let matcher = group.get("matcher").and_then(Value::as_str);
    let matches = group.get("matcher").is_none()
        || matcher == Some("")
        || (event == "SessionStart" && matcher == Some("startup|resume"));
    matches
        && group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|handlers| {
                handlers.iter().any(|h| {
                    h.get("type").and_then(Value::as_str) == Some("command")
                        && h.get("command").and_then(Value::as_str) == Some(command)
                        && h.get("async").is_none_or(|v| v.as_bool() == Some(false))
                })
            })
}
pub(super) fn setup(path: Option<&Path>) -> AppResult<()> {
    let result = (|| -> anyhow::Result<String> {
        let mut value = settings(path)?;
        let command = command()?;
        let hooks = value
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("settings must be a TOML table"))?
            .entry("hooks")
            .or_insert_with(|| Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("hooks must be a table"))?;
        for event in HOOKS {
            let groups = hooks
                .entry(*event)
                .or_insert_with(|| Value::Array(vec![]))
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("hook event must be an array"))?;
            if !groups.iter().any(|group| reporter(group, event, &command)) {
                let mut group = toml::map::Map::new();
                if *event == "SessionStart" {
                    group.insert("matcher".into(), Value::String("startup|resume".into()));
                }
                let mut handler = toml::map::Map::new();
                handler.insert("type".into(), Value::String("command".into()));
                handler.insert("command".into(), Value::String(command.clone()));
                group.insert("hooks".into(), Value::Array(vec![Value::Table(handler)]));
                groups.push(Value::Table(group));
            }
        }
        Ok(toml::to_string_pretty(&value)?)
    })();
    // Do not echo parser diagnostics: malformed supplied configuration may contain secrets.
    let value = result.map_err(|_| RuntimeError::internal(anyhow::anyhow!("Cannot compose Codex TOML: settings unreadable, invalid, oversized, or incompatible hook structure")))?;
    print!("{value}");
    eprintln!(
        "{REQUIREMENTS}\n{FORMS}\nNo file was written. Review the printed TOML and explicitly merge it into the intended Codex config.toml. Existing values and handler order are preserved; comments and formatting are not. Do not redirect onto the input file. Preserve permission, approval and trust settings; never copy trust hashes from an example. Removal: remove only the exact added direct-exec reporter handlers; preserve other handlers and their order."
    );
    Ok(())
}
pub(super) fn doctor(
    path: Option<&Path>,
    session: Option<u64>,
    executable: &OsStr,
) -> AppResult<()> {
    let probe = with_version_probe(|| ovrcr::report::admission::probe_version(executable))?;
    // Report only the native version grammar, never arbitrary executable output.
    let version = probe
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|s| s.strip_prefix("codex-cli ")?.strip_suffix('\n'))
        .filter(|s| {
            s.split('.').count() == 3
                && s.split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        });
    let supported = version == Some(ovrcr::report::codex::PINNED_VERSION);
    let status = if supported {
        "supported"
    } else if probe.is_some() {
        "unsupported"
    } else {
        "unavailable"
    };
    let mut issues = vec![];
    let configuration = match (path, settings(path)) {
        (None, _) => "unverified",
        (Some(_), Err(_)) => {
            issues.push("settings_unreadable_or_invalid".to_owned());
            "unverified"
        }
        (Some(_), Ok(value)) => {
            let command = command().map_err(RuntimeError::internal)?;
            for event in HOOKS {
                if !value
                    .get("hooks")
                    .and_then(|h| h.get(*event))
                    .and_then(Value::as_array)
                    .is_some_and(|groups| groups.iter().any(|g| reporter(g, event, &command)))
                {
                    issues.push(format!("{event}:synchronous_reporter_missing"));
                }
            }
            if issues.is_empty() {
                "supplied_file_supported"
            } else {
                "supplied_file_unsupported_or_unverified"
            }
        }
    };
    println!("{}", serde_json::to_string_pretty(&json!({
        "provider":"codex", "executable":executable.to_string_lossy(), "version":version,
        "supported_versions":[ovrcr::report::codex::PINNED_VERSION], "probe_status":status,
        "release_status":"accepted_exact_0.153.0_hooks_only",
        "configuration":{"status":configuration,"effective_configuration":"unverified","hook_trust":"unverified","delivery":"unverified","issues":issues},
        "session_status":if session.is_some() { "not_inspected_use_session_usage" } else { "not_requested" },
        "capabilities":{"initial_invocation":{"fresh":supported,"resume":false,"fork":false,"picker":false},"activity":"last_observed_root_turn","completion_quality":"observed","metrics":"unavailable","task_success":false},
        "requirements":REQUIREMENTS, "launch_forms":FORMS,
        "remediation":"Run agent setup codex --print --settings PATH, review the composition and trust hooks through native Codex. Select exact codex-cli 0.153.0. Doctor only invokes --version; no server or provider conversation is required."
    })).map_err(RuntimeError::internal)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_presence_requires_unfiltered_synchronous_exact_command() {
        let base: Value = toml::from_str(
            "[[hooks]]\ntype = 'command'\ncommand = 'exec exact report codex --stdin'\n",
        )
        .unwrap();
        let command = "exec exact report codex --stdin";
        assert!(reporter(&base, "Stop", command));
        assert!(!reporter(&base, "Stop", "exec other report codex --stdin"));
        for matcher in [Value::String("never".into()), Value::Boolean(false)] {
            let mut group = base.clone();
            group
                .as_table_mut()
                .unwrap()
                .insert("matcher".into(), matcher);
            assert!(!reporter(&group, "Stop", command));
        }
        for asynchronous in [Value::Boolean(true), Value::String("false".into())] {
            let mut group = base.clone();
            group["hooks"][0]
                .as_table_mut()
                .unwrap()
                .insert("async".into(), asynchronous);
            assert!(!reporter(&group, "Stop", command));
        }
        let mut startup = base.clone();
        startup
            .as_table_mut()
            .unwrap()
            .insert("matcher".into(), "startup|resume".into());
        assert!(reporter(&startup, "SessionStart", command));
        assert!(!reporter(&startup, "Stop", command));
    }
}
