//! Use the packaged installer; runtime never compiles the optional macOS Bridge.
use anyhow::Result;

#[cfg(not(target_os = "macos"))]
pub(super) fn run(_confirm: &mut impl FnMut(&str) -> Result<bool>) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn run(confirm: &mut impl FnMut(&str) -> Result<bool>) -> Result<()> {
    use super::startup::command_output;
    use anyhow::{Context, ensure};
    use std::{
        path::{Path, PathBuf},
        process::Command,
        time::Duration,
    };
    let executable = std::env::current_exe()?;
    let directory = executable.parent().context("locate startup assets")?;
    let installed = directory.parent().map(|p| p.join("lib/ovrcr"));
    let payload = installed
        .filter(|p| p.join("scripts/install-bridge.sh").is_file())
        .unwrap_or_else(|| directory.join("ovrcr-startup"));
    if !payload.join("scripts/install-bridge.sh").is_file() {
        eprintln!("Bridge install assets missing; `just run` packages them with the CLI.");
        return Ok(());
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("locate Bridge destination")?);
    let destination = home.join("Applications/OVRCR Bridge.app");
    let validator = payload.join("native/bridge/validate-bundle.py");
    let validate = |bundle: &Path| -> Result<bool> {
        Ok(command_output(
            Command::new("python3")
                .arg("-I")
                .arg(&validator)
                .arg(bundle),
            Duration::from_secs(5),
        )?
        .status
        .success())
    };
    if destination.exists()
        && validate(&destination)?
        && command_output(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&destination),
            Duration::from_secs(5),
        )?
        .status
        .success()
    {
        return Ok(());
    }
    let source = payload.join("OVRCR Bridge.app");
    ensure!(validate(&source)?, "packaged Bridge is invalid");
    let repair = if destination.exists() {
        "Repair or update"
    } else {
        "Install"
    };
    if !confirm(&format!(
        "{repair} OVRCR Bridge at {}? This installs the app; notification permission is requested separately.",
        destination.display()
    ))? {
        return Ok(());
    }
    let identity = match std::env::var("OVRCR_BRIDGE_SIGNING_IDENTITY") {
        Ok(value) if !value.is_empty() && value != "-" => value,
        Ok(_) => anyhow::bail!("Bridge needs a non-ad-hoc signing identity"),
        Err(_) => {
            let result = command_output(
                Command::new("/usr/bin/security").args([
                    "find-identity",
                    "-v",
                    "-p",
                    "codesigning",
                ]),
                Duration::from_secs(5),
            )?;
            ensure!(
                result.status.success(),
                "cannot find Bridge signing identity"
            );
            let identities = signing_identities(&String::from_utf8_lossy(&result.stdout));
            ensure!(
                identities.len() == 1,
                "set OVRCR_BRIDGE_SIGNING_IDENTITY to one valid non-ad-hoc identity"
            );
            identities[0].clone()
        }
    };
    let result = command_output(
        Command::new("/bin/sh")
            .arg(payload.join("scripts/install-bridge.sh"))
            .arg("--identity")
            .arg(identity)
            .arg("--destination")
            .arg(&destination)
            .arg(&source),
        Duration::from_secs(60),
    )?;
    ensure!(
        result.status.success(),
        "Bridge installer refused or failed; native installation needs inspection"
    );
    eprintln!(
        "Installed OVRCR Bridge at {}. An already running Bridge keeps its process until you quit that selected app.",
        destination.display()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn signing_identities(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            words.next()?.strip_suffix(')')?.parse::<u32>().ok()?;
            let hash = words.next()?;
            (hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| hash.to_owned())
        })
        .collect()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn signing_identity_requires_a_numbered_certificate_hash() {
        assert_eq!(
            super::signing_identities(
                "  1) 0123456789ABCDEF0123456789ABCDEF01234567 \"Certificate\"\n  1 valid identities found"
            ),
            ["0123456789ABCDEF0123456789ABCDEF01234567"]
        );
        assert!(super::signing_identities("1) - \"Ad hoc\"\n arbitrary output").is_empty());
    }
}
