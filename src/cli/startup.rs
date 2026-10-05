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
        write_prompt(&mut std::io::stderr(), message, use_color())?;
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
            notice(
                "Hook setup could not complete; inspect native configuration and run `ovrcr agent doctor`.",
                false,
            );
        }
        if super::startup_bridge::run(&mut confirm).is_err() {
            notice(
                "Bridge setup could not complete; Dashboard will continue. Check signing identity and ~/Applications/.ovrcr-bridge-install.* recovery folders before retrying.",
                false,
            );
        }
    }
    ovrcr::client::connect_dashboard(paths, &mut confirm)
}

fn use_color() -> bool {
    std::io::stderr().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("TERM").as_deref() != Some(std::ffi::OsStr::new("dumb"))
}

fn terminal_color(color: ovrcr_tui::theme::Color) -> crossterm::style::Color {
    match color {
        ovrcr_tui::theme::Color::Rgb(r, g, b) => crossterm::style::Color::Rgb { r, g, b },
        _ => crossterm::style::Color::Reset,
    }
}

fn write_prompt(output: &mut impl Write, message: &str, color: bool) -> std::io::Result<()> {
    use crossterm::style::{Stylize, style};
    use ovrcr_tui::theme;
    let clean: String = message
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    let mut lines = clean.lines();
    let title = lines.next().unwrap_or("Local setup");
    let width = crossterm::terminal::size()
        .map(|(cols, _)| usize::from(cols).min(80))
        .unwrap_or(80);
    let heading = format!(
        " {:<width$}",
        "OVRCR · startup",
        width = width.saturating_sub(1)
    );
    writeln!(output)?;
    if color {
        writeln!(
            output,
            "{}",
            style(heading)
                .with(terminal_color(theme::BASE))
                .on(terminal_color(theme::MAUVE))
                .bold()
        )?;
        let accent = if title.contains("Restart") {
            theme::YELLOW
        } else {
            theme::MAUVE
        };
        writeln!(
            output,
            "\n  {}",
            style(title).with(terminal_color(accent)).bold()
        )?;
    } else {
        writeln!(output, "{heading}\n\n  {title}")?;
    }
    for line in lines {
        if color {
            if line == "All running sessions will stop." {
                writeln!(
                    output,
                    "  {}",
                    style(line).with(terminal_color(theme::YELLOW)).bold()
                )?;
            } else {
                writeln!(
                    output,
                    "  {}",
                    style(line).with(terminal_color(theme::SUBTEXT))
                )?;
            }
        } else {
            writeln!(output, "  {line}")?;
        }
    }
    if color {
        write!(
            output,
            "\n  {} {} ",
            style("[y/n]")
                .with(terminal_color(theme::BLUE))
                .on(terminal_color(theme::SURFACE0))
                .bold(),
            style("Enter skips · default n  ›").with(terminal_color(theme::SUBTEXT))
        )?;
    } else {
        write!(output, "\n  [y/n] Enter skips · default n  > ")?;
    }
    Ok(())
}

pub(super) fn notice(message: &str, success: bool) {
    use crossterm::style::{Stylize, style};
    use ovrcr_tui::theme;
    let clean: String = message
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    if use_color() {
        let accent = if success { theme::GREEN } else { theme::YELLOW };
        eprintln!("  {}", style(clean).with(terminal_color(accent)));
    } else {
        eprintln!("  {clean}");
    }
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
    fn plain_prompt_keeps_the_decision_and_cannot_execute_path_controls() {
        let mut output = Vec::new();
        write_prompt(
            &mut output,
            "Restart OVRCR server\nSocket: /tmp/\x1b[2Jprivate\nAll running sessions will stop.",
            false,
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(!text.contains('\x1b'));
        assert!(text.contains("All running sessions will stop."));
        assert!(text.contains("[y/n]"));
        assert!(text.contains("default n"));
    }

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
