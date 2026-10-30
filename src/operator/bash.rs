//! Bash stays in the managed worker's process group; the Server owns timeout stop.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    time::Instant,
};

pub const OUTPUT_LIMIT: usize = 64 * 1024;
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BashArgs {
    pub command: String,
    pub cwd: Option<PathBuf>,
    pub timeout_seconds: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct BashOutput {
    pub output: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub timed_out: bool,
}
pub struct BashExecution {
    pub result: BashOutput,
    // Keep the direct child owned while the Server stops the complete worker group.
    pub(crate) child: Option<Child>,
}
#[derive(Clone)]
pub struct Bash {
    cwd: PathBuf,
    executable: PathBuf,
}
impl Bash {
    pub fn new(cwd: PathBuf, executable: PathBuf) -> Self {
        Self { cwd, executable }
    }
    pub async fn execute(&self, args: BashArgs) -> Result<BashExecution> {
        let seconds = args.timeout_seconds.unwrap_or(120);
        if !(1..=600).contains(&seconds) {
            bail!("Bash timeout must be between 1 and 600 seconds");
        }
        self.execute_until(args, Instant::now() + Duration::from_secs(seconds))
            .await
    }
    async fn execute_until(&self, args: BashArgs, deadline: Instant) -> Result<BashExecution> {
        if args.command.is_empty()
            || args.command.len() > OUTPUT_LIMIT
            || args.command.contains('\0')
        {
            bail!("invalid Bash command");
        }
        let cwd = args.cwd.as_ref().unwrap_or(&self.cwd);
        if !cwd.is_absolute() {
            bail!("Bash cwd must be absolute");
        }
        let mut command = Command::new("/bin/bash");
        command
            .args(["--noprofile", "--norc", "-c", &args.command])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("OVRCR_EXECUTABLE", &self.executable);
        // Preserve ordinary tool credentials and public instance routing, never model or lease credentials.
        for (key, _) in std::env::vars_os() {
            if private_shell_env(&key.to_string_lossy()) {
                command.env_remove(key);
            }
        }
        command.env_remove("BASH_ENV").env_remove("ENV");
        let mut child = command
            .spawn()
            .map_err(|_| anyhow::anyhow!("Bash launch failed"))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let mut out_open = true;
        let mut err_open = true;
        let mut bytes = Vec::with_capacity(OUTPUT_LIMIT);
        let mut truncated = false;
        let mut out = [0u8; 8192];
        let mut err = [0u8; 8192];
        while out_open || err_open {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => return Ok(timed_out(bytes, truncated, child)),
                read = stdout.read(&mut out), if out_open => {
                    match read { Ok(0) => out_open = false, Ok(n) => retain(&mut bytes, &out[..n], &mut truncated), Err(_) => return Ok(uncertain(bytes, truncated, child)) }
                }
                read = stderr.read(&mut err), if err_open => {
                    match read { Ok(0) => err_open = false, Ok(n) => retain(&mut bytes, &err[..n], &mut truncated), Err(_) => return Ok(uncertain(bytes, truncated, child)) }
                }
            }
        }
        let status = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => return Ok(timed_out(bytes, truncated, child)),
            status = child.wait() => match status { Ok(status) => status, Err(_) => return Ok(uncertain(bytes, truncated, child)) },
        };
        Ok(BashExecution {
            result: output(bytes, truncated, status.code(), false),
            child: None,
        })
    }
}
fn retain(bytes: &mut Vec<u8>, incoming: &[u8], truncated: &mut bool) {
    let count = incoming.len().min(OUTPUT_LIMIT - bytes.len());
    bytes.extend_from_slice(&incoming[..count]);
    *truncated |= count != incoming.len();
}
fn output(
    bytes: Vec<u8>,
    mut truncated: bool,
    exit_code: Option<i32>,
    timed_out: bool,
) -> BashOutput {
    let mut output = String::from_utf8_lossy(&bytes).replace('\0', "�");
    if output.len() > OUTPUT_LIMIT {
        let mut end = OUTPUT_LIMIT;
        while !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
        truncated = true;
    }
    BashOutput {
        output,
        exit_code,
        truncated,
        timed_out,
    }
}
fn timed_out(bytes: Vec<u8>, truncated: bool, child: Child) -> BashExecution {
    BashExecution {
        result: output(bytes, truncated, None, true),
        child: Some(child),
    }
}
fn uncertain(bytes: Vec<u8>, truncated: bool, child: Child) -> BashExecution {
    BashExecution {
        result: output(bytes, truncated, None, false),
        child: Some(child),
    }
}
fn private_shell_env(key: &str) -> bool {
    matches!(
        key,
        "OPENAI_API_KEY"
            | "OPENAI_ACCESS_TOKEN"
            | "OPENAI_REFRESH_TOKEN"
            | "CHATGPT_ACCESS_TOKEN"
            | "CHATGPT_REFRESH_TOKEN"
            | "XAI_API_KEY"
            | "XAI_ACCESS_TOKEN"
            | "XAI_REFRESH_TOKEN"
            | "GROK_API_KEY"
            | "GROK_ACCESS_TOKEN"
            | "GROK_REFRESH_TOKEN"
            | "OVRCR_AGENT_SOCKET"
            | "OVRCR_AGENT_TOKEN"
            | "OVRCR_HOOK_SOCKET"
            | "OVRCR_HOOK_TOKEN"
            | "OVRCR_SESSION_ID"
            | "OVRCR_DELEGATION_SESSION"
            | "OVRCR_DELEGATION_TOKEN"
            | "OVRCR_DELEGATION_SOCKET"
            | "OVRCR_DELEGATION_HOME"
            | "OVRCR_DELEGATION_INSTANCE"
            | "OVRCR_DELEGATION_EPOCH"
            | "OPENAI_AUTH_TOKEN"
            | "CHATGPT_TOKEN"
            | "GROK_TOKEN"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn real_bash_captures_both_streams_exit_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        let tool = Bash::new(dir.path().into(), std::env::current_exe().unwrap());
        let outcome = tool
            .execute(BashArgs {
                command: "printf out; printf err >&2; exit 7".into(),
                cwd: None,
                timeout_seconds: None,
            })
            .await
            .unwrap();
        assert!(outcome.result.output.contains("out"));
        assert!(outcome.result.output.contains("err"));
        assert_eq!(outcome.result.exit_code, Some(7));
        assert!(!outcome.result.truncated);
        let large = tool
            .execute(BashArgs {
                command: "head -c 100000 /dev/zero | tr '\\0' x".into(),
                cwd: None,
                timeout_seconds: None,
            })
            .await
            .unwrap();
        assert_eq!(large.result.output.len(), 65536);
        assert!(large.result.truncated);
    }
    #[tokio::test]
    async fn bash_uses_captured_cwd_no_startup_files_and_matching_cli() {
        let dir = tempfile::tempdir().unwrap();
        let tool = Bash::new(dir.path().into(), std::env::current_exe().unwrap());
        let outcome = tool
            .execute(BashArgs {
                command: "printf '%s\\n' \"$PWD\" \"$OVRCR_EXECUTABLE\"".into(),
                cwd: None,
                timeout_seconds: None,
            })
            .await
            .unwrap();
        assert!(outcome.result.output.contains(dir.path().to_str().unwrap()));
        assert!(
            outcome
                .result
                .output
                .contains(std::env::current_exe().unwrap().to_str().unwrap())
        );
        assert!(
            tool.execute(BashArgs {
                command: "true".into(),
                cwd: None,
                timeout_seconds: Some(601)
            })
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn timeout_keeps_the_child_in_supervised_group_until_exact_turn_stop() {
        let dir = tempfile::tempdir().unwrap();
        let tool = Bash::new(dir.path().into(), std::env::current_exe().unwrap());
        let mut outcome = tool
            .execute_until(
                BashArgs {
                    command: "sleep 0.08".into(),
                    cwd: None,
                    timeout_seconds: None,
                },
                Instant::now() + Duration::from_millis(5),
            )
            .await
            .unwrap();
        assert!(outcome.result.timed_out);
        assert_eq!(outcome.result.exit_code, None);
        let mut child = outcome.child.take().expect("the timeout keeps ownership");
        let pid = child.id().unwrap() as libc::pid_t;
        assert_eq!(unsafe { libc::getpgid(pid) }, unsafe { libc::getpgrp() });
        assert!(
            child.try_wait().unwrap().is_none(),
            "timeout did not independently terminate the child"
        );
        assert!(
            child.wait().await.unwrap().success(),
            "fixture drains its task-owned child"
        );
    }
}
