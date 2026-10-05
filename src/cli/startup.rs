//! Optional local repairs before the interactive Dashboard attaches.
use anyhow::Result;
use ovrcr::server::ServerPaths;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::os::unix::net::UnixStream;

pub(super) fn run(paths: &ServerPaths) -> Result<UnixStream> {
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let mut can_prompt = interactive;
    let mut confirm = |message: &str| -> Result<bool> {
        if !can_prompt {
            return Ok(false);
        }
        eprint!("{message} [y/n] ");
        std::io::stderr().flush()?;
        let mut answer = String::new();
        let count = std::io::stdin().lock().take(128).read_line(&mut answer)?;
        if count == 128 && !answer.ends_with('\n') {
            can_prompt = false;
            eprintln!("Answer too long; leaving remaining repairs unchanged.");
            return Ok(false);
        }
        Ok(matches!(
            answer.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        ))
    };
    if interactive {
        if super::startup_hooks::run(&mut confirm).is_err() {
            eprintln!(
                "Hook setup could not complete; inspect native configuration and run `ovrcr agent doctor`."
            );
        }
        if super::startup_bridge::run(&mut confirm).is_err() {
            eprintln!(
                "Bridge setup could not complete; Dashboard will continue. Check signing identity and ~/Applications/.ovrcr-bridge-install.* recovery folders before retrying."
            );
        }
    }
    ovrcr::client::connect_dashboard(paths, &mut confirm)
}

/// Bound the local packaging tools; their output never becomes a native-config diagnostic.
#[cfg(any(target_os = "macos", test))]
pub(super) fn command_output(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    use anyhow::{Context, bail};
    use std::os::unix::process::CommandExt;
    use std::process::{Output, Stdio};
    use std::time::{Duration, Instant};
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().context("start local setup tool")?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline
            || stdout.metadata()?.len() > 1_048_576
            || stderr.metadata()?.len() > 1_048_576
        {
            // This unreaped child established this process group; never signal a discovered PID.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!("local setup tool exceeded its time or output limit");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let read = |file: std::fs::File| -> Result<Vec<u8>> {
        use std::io::{Seek, SeekFrom};
        let mut file = file;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(1_048_577).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 1_048_576, "local setup output exceeds 1 MiB");
        Ok(bytes)
    };
    Ok(Output {
        status,
        stdout: read(stdout)?,
        stderr: read(stderr)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Command, time::Duration};
    #[test]
    fn local_setup_output_is_captured_and_timeout_reaps_the_owned_tool() {
        let output = command_output(
            Command::new("/bin/sh").args(["-c", "printf READY"]),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"READY");
        assert!(
            command_output(
                Command::new("/bin/sh").args(["-c", "exec sleep 30"]),
                Duration::from_millis(20)
            )
            .is_err()
        );
    }
}
