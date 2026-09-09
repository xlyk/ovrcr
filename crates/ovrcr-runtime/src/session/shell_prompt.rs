use std::{ffi::OsString, fs, path::Path};

use anyhow::{Context, Result};
use portable_pty::CommandBuilder;

/// Only bare zsh launches are OVRCR-managed interactive shells. Never rewrite
/// explicit argv (including scripts, `-c`, or user-selected startup flags).
pub(super) fn configure(
    command: &mut CommandBuilder,
    argv: &[OsString],
) -> Result<Option<tempfile::TempDir>> {
    if argv.len() != 1 || Path::new(&argv[0]).file_name() != Some("zsh".as_ref()) {
        return Ok(None);
    }
    let startup = tempfile::Builder::new()
        .prefix("ovrcr-zsh-")
        .tempdir()
        .context("create zsh startup directory")?;
    fs::write(
        startup.path().join(".zshenv"),
        include_str!("shell_prompt.zshenv"),
    )?;
    fs::write(
        startup.path().join(".zshrc"),
        include_str!("shell_prompt.zshrc"),
    )?;
    command.env_remove("OVRCR_ORIGINAL_ZDOTDIR");
    if let Some(original) = std::env::var_os("ZDOTDIR") {
        command.env("OVRCR_ORIGINAL_ZDOTDIR", original);
    }
    command.env("ZDOTDIR", startup.path());
    Ok(Some(startup))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Read,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn compact_prompt_keeps_custom_zdotdir_and_leaves_explicit_commands_alone() {
        let home = tempfile::tempdir().unwrap();
        let custom = home.path().join("custom config");
        fs::create_dir(&custom).unwrap();
        fs::write(custom.join(".zshenv"), "export FROM_ENV=loaded\n").unwrap();
        fs::write(
            custom.join(".zshrc"),
            "alias fixture='print -r -- $FROM_ENV:$ZDOTDIR'\n",
        )
        .unwrap();
        let mut command = CommandBuilder::new("/bin/zsh");
        let startup = configure(&mut command, &["/bin/zsh".into()])
            .unwrap()
            .unwrap();
        command.env("OVRCR_ORIGINAL_ZDOTDIR", &custom);
        command.env("HOME", home.path());
        command.env("TERM", "xterm-256color");
        command.env(
            "LC_ALL",
            if cfg!(target_os = "macos") {
                "en_US.UTF-8"
            } else {
                "C.UTF-8"
            },
        );
        // -d skips host-wide configuration (such as interactive compinit checks).
        // User startup files still load; -ic runs the fixture and exits.
        command.args([
            "-dic",
            "fixture; print -r -- $PROMPT; print -r -- ${+OVRCR_ORIGINAL_ZDOTDIR}",
        ]);
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut bytes = [0; 4096];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => {
                        if tx.send(bytes[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(error) => panic!("read zsh fixture output: {error}"),
                }
            }
        });
        let mut output = String::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(bytes) => output.push_str(&String::from_utf8_lossy(&bytes)),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    reader.join().unwrap();
                    panic!("zsh startup did not exit; output: {output}");
                }
            }
        }
        reader.join().unwrap();
        assert!(child.wait().unwrap().success());
        assert!(
            output.contains(&format!("loaded:{}", custom.display())),
            "{output}"
        );
        assert!(output.contains("%1~"), "{output}");
        assert!(output.contains("\r\n0\r\n"), "{output}");
        drop(startup);
        for argv in [
            vec!["/bin/zsh", "-c", "echo untouched"],
            vec!["/bin/zsh", "-l"],
            vec!["/bin/bash"],
            vec!["claude"],
        ] {
            let argv: Vec<OsString> = argv.into_iter().map(Into::into).collect();
            let mut command = CommandBuilder::new(&argv[0]);
            assert!(configure(&mut command, &argv).unwrap().is_none());
        }
    }
}
