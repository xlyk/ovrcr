//! Use the packaged installer; runtime never compiles the optional macOS Bridge.
use anyhow::Result;

#[cfg(not(target_os = "macos"))]
pub(super) fn run(_confirm: &mut impl FnMut(&str) -> Result<bool>) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn run(confirm: &mut impl FnMut(&str) -> Result<bool>) -> Result<()> {
    use anyhow::Context;
    use ovrcr::protocol::bridge_installation::LOCAL_DEVELOPMENT_ENV;
    let executable = std::env::current_exe()?;
    let home = std::env::var_os("HOME").context("locate Bridge destination")?;
    let opt_in = std::env::var_os(LOCAL_DEVELOPMENT_ENV);
    run_at(
        Inputs {
            executable: &executable,
            home: std::path::Path::new(&home),
            opt_in: opt_in.as_deref(),
        },
        confirm,
        &mut super::startup::command_output,
        &mut || std::env::var("OVRCR_BRIDGE_SIGNING_IDENTITY"),
        &mut super::startup::notice,
        &mut SystemProcesses,
    )
}

#[cfg(target_os = "macos")]
struct Inputs<'a> {
    executable: &'a std::path::Path,
    home: &'a std::path::Path,
    opt_in: Option<&'a std::ffi::OsStr>,
}

/// One LaunchServices-registered Bridge app process for the selected bundle ID.
#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct RunningBridge {
    pid: u32,
    executable: std::path::PathBuf,
    started: std::time::SystemTime,
}

/// Running-Bridge discovery and its only side effect, kept behind one seam.
#[cfg(target_os = "macos")]
trait BridgeProcesses {
    fn running(&mut self, bundle_id: &str) -> Result<Vec<RunningBridge>>;
    /// Send SIGTERM only while the PID still has the observed start time.
    fn terminate(&mut self, bridge: &RunningBridge) -> Result<()>;
    fn exited(&mut self, bridge: &RunningBridge) -> bool;
}

#[cfg(target_os = "macos")]
fn run_at(
    inputs: Inputs<'_>,
    confirm: &mut impl FnMut(&str) -> Result<bool>,
    execute: &mut impl FnMut(
        &mut std::process::Command,
        std::time::Duration,
    ) -> Result<std::process::Output>,
    signing_identity: &mut impl FnMut() -> std::result::Result<String, std::env::VarError>,
    notice: &mut impl FnMut(&str, bool),
    processes: &mut impl BridgeProcesses,
) -> Result<()> {
    use anyhow::{Context, ensure};
    use ovrcr::protocol::bridge_installation::BridgeProfile;
    use std::{process::Command, time::Duration};
    let profile = BridgeProfile::from_opt_in(inputs.opt_in)?;
    let destination = profile.destination(inputs.home);
    let payload = profile
        .installed_assets(inputs.executable)
        .filter(|p| p.join("scripts/install-bridge.sh").is_file())
        .or_else(|| profile.adjacent_assets(inputs.executable))
        .context("locate startup assets")?;
    if !payload.join("scripts/install-bridge.sh").is_file() {
        notice(
            &missing_assets_notice(profile, &destination, inputs.executable),
            false,
        );
        return Ok(());
    }
    if let Some(reason) = stale_assets(&payload, inputs.executable) {
        notice(
            &format!(
                "{} startup assets at {} were packaged for a different ovrcr ({reason}), so no repair is offered from them. {}, then restart OVRCR.",
                app_label(profile),
                payload.display(),
                refresh_command(profile, inputs.executable)
            ),
            false,
        );
        return Ok(());
    }
    let validator = payload.join("native/bridge/validate-bundle.py");
    let local_existing = if profile == BridgeProfile::LocalDevelopment {
        match std::fs::symlink_metadata(&destination) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        }
    } else {
        None
    };
    let can_validate_existing = if profile == BridgeProfile::LocalDevelopment {
        local_existing
            .as_ref()
            .is_some_and(|metadata| metadata.is_dir())
    } else {
        destination.exists()
    };
    if can_validate_existing
        && validate_bundle(profile, &validator, &destination, execute)?
        && execute(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&destination),
            Duration::from_secs(5),
        )?
        .status
        .success()
    {
        let stale = stale_processes(profile, &destination, processes, notice);
        if stale.is_empty() {
            return Ok(());
        }
        let prompt = format!(
            "Restart {label}\nRunning: {}\nInstalled: {}\n\nThe running Bridge is older than the installed app, so the\nDashboard reports it as needing an update. Quits that process\nand starts the installed app.",
            describe(&stale),
            destination.display(),
            label = app_label(profile),
        );
        if confirm(&prompt)? {
            restart_processes(profile, &destination, &stale, processes, execute, notice);
        } else {
            notice(
                &format!(
                    "Running {} ({}) is older than the installed app; desktop notifications stay unavailable until it restarts. Restart OVRCR to review the offer again.",
                    app_label(profile),
                    describe(&stale)
                ),
                false,
            );
        }
        return Ok(());
    }
    if local_existing.is_some() {
        notice(
            "OVRCR Local installation is invalid or incompatible; local development is fresh-only and will not repair or replace that path. Inspect the existing local app before choosing a new installation. The production Bridge is untouched.",
            false,
        );
        return Ok(());
    }
    let source = payload.join(profile.app_name());
    ensure!(
        validate_bundle(profile, &validator, &source, execute)?,
        "packaged Bridge is invalid"
    );
    let stale = stale_processes(profile, &destination, processes, notice);
    let restart = if stale.is_empty() {
        String::new()
    } else {
        format!(
            "\nThe running Bridge ({}) is quit and the installed app started.",
            describe(&stale)
        )
    };
    let repair = if destination.exists() {
        "Repair or update"
    } else {
        "Install"
    };
    let prompt = if profile == BridgeProfile::LocalDevelopment {
        format!(
            "Install OVRCR Local\nLocation: {}\n\nInstalls a separate local-development app with ad-hoc signing.\nNo Developer ID certificate or Keychain signing identity is used.\nNotification permission is requested separately.{restart}",
            destination.display()
        )
    } else {
        format!(
            "{repair} OVRCR Bridge\nLocation: {}\n\nInstalls the app from the packaged startup assets.\nNotification permission is requested separately.{restart}",
            destination.display()
        )
    };
    if !confirm(&prompt)? {
        return Ok(());
    }
    let mut installer = Command::new("/bin/sh");
    installer.arg(payload.join("scripts/install-bridge.sh"));
    if profile == BridgeProfile::LocalDevelopment {
        installer
            .arg("--local-development")
            .env_remove("OVRCR_BRIDGE_SIGNING_IDENTITY");
    } else {
        let identity = match signing_identity() {
            Ok(value) if !value.is_empty() && value != "-" => value,
            Ok(_) => anyhow::bail!("Bridge needs a non-ad-hoc signing identity"),
            Err(_) => {
                let result = execute(
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
        installer.arg("--identity").arg(identity);
    }
    let result = execute(
        installer
            .arg("--destination")
            .arg(&destination)
            .arg(&source),
        Duration::from_secs(60),
    )?;
    ensure!(
        result.status.success(),
        "Bridge installer refused or failed; native installation needs inspection"
    );
    notice(
        &format!(
            "Installed {} at {}.",
            app_label(profile),
            destination.display()
        ),
        true,
    );
    // The accepted repair covers the process still running the replaced build.
    let stale = stale_processes(profile, &destination, processes, notice);
    if !stale.is_empty() {
        restart_processes(profile, &destination, &stale, processes, execute, notice);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn app_label(profile: ovrcr::protocol::bridge_installation::BridgeProfile) -> &'static str {
    use ovrcr::protocol::bridge_installation::BridgeProfile;
    match profile {
        BridgeProfile::Production => "OVRCR Bridge",
        BridgeProfile::LocalDevelopment => "OVRCR Local",
    }
}

/// The supported way to (re)package startup assets for this exact CLI.
#[cfg(target_os = "macos")]
fn refresh_command(
    profile: ovrcr::protocol::bridge_installation::BridgeProfile,
    executable: &std::path::Path,
) -> String {
    use ovrcr::protocol::bridge_installation::BridgeProfile;
    format!(
        "From an OVRCR checkout, run `{}just install-startup-assets {}`",
        if profile == BridgeProfile::LocalDevelopment {
            "OVRCR_BRIDGE_LOCAL_DEVELOPMENT=1 "
        } else {
            ""
        },
        shell_word(&executable.to_string_lossy())
    )
}

#[cfg(target_os = "macos")]
fn shell_word(text: &str) -> String {
    if !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+@%:,".contains(&b))
    {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

#[cfg(target_os = "macos")]
fn missing_assets_notice(
    profile: ovrcr::protocol::bridge_installation::BridgeProfile,
    destination: &std::path::Path,
    executable: &std::path::Path,
) -> String {
    let assets = profile
        .installed_assets(executable)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "the CLI's lib directory".into());
    let command = refresh_command(profile, executable);
    let label = app_label(profile);
    if destination.exists() {
        format!(
            "{label} is installed but cannot be checked or repaired: startup assets are missing at {assets}. {command}, then restart OVRCR."
        )
    } else {
        format!(
            "{label} is not installed, and its startup assets are missing at {assets}. {command}, then restart OVRCR to review the install offer."
        )
    }
}

/// Packaged assets must match this CLI before they can repair anything: a
/// different wire or schema installs an incompatible Bridge, and a different
/// callback CLI makes notification clicks fail closed.
#[cfg(target_os = "macos")]
fn stale_assets(payload: &std::path::Path, executable: &std::path::Path) -> Option<String> {
    use ovrcr::protocol::{BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION};
    let Some(contract) =
        std::fs::read_to_string(payload.join("native/bridge/expected-contract.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Some("no readable Bridge contract".into());
    };
    let wire = contract["wire"].as_u64();
    if wire != Some(u64::from(PROTOCOL_VERSION)) {
        return Some(format!(
            "protocol {}, this CLI {PROTOCOL_VERSION}",
            wire.map_or_else(|| "unknown".into(), |wire| wire.to_string())
        ));
    }
    let schema = contract["schema"].as_u64();
    if schema != Some(u64::from(BRIDGE_SCHEMA_VERSION)) {
        return Some(format!(
            "Bridge schema {}, this CLI {BRIDGE_SCHEMA_VERSION}",
            schema.map_or_else(|| "unknown".into(), |schema| schema.to_string())
        ));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    match ovrcr::server::bridge_executable_sha256(executable, deadline) {
        Ok(digest) if contract["callback_sha256"].as_str() != Some(digest.as_str()) => {
            Some("its callback CLI is a different build".into())
        }
        // An unreadable CLI leaves the bundle validator as the only check.
        _ => None,
    }
}

/// Running app processes for this profile that started before the installed
/// executable last changed. Processes from another copy are only reported.
#[cfg(target_os = "macos")]
fn stale_processes(
    profile: ovrcr::protocol::bridge_installation::BridgeProfile,
    destination: &std::path::Path,
    processes: &mut impl BridgeProcesses,
    notice: &mut impl FnMut(&str, bool),
) -> Vec<RunningBridge> {
    use std::os::unix::fs::MetadataExt;
    let client = destination.join("Contents/MacOS/OVRCRBridge");
    let running = match processes.running(profile.bundle_id()) {
        Ok(running) => running,
        Err(_) => {
            notice(
                &format!(
                    "Could not check for a running {}; if notifications report an update, quit that app and restart OVRCR.",
                    app_label(profile)
                ),
                false,
            );
            return Vec::new();
        }
    };
    let installed = std::fs::metadata(&client).ok().map(|metadata| {
        std::time::UNIX_EPOCH
            + std::time::Duration::new(metadata.ctime() as u64, metadata.ctime_nsec() as u32)
    });
    let canonical = |path: &std::path::Path| path.canonicalize().unwrap_or_else(|_| path.into());
    let mut stale = Vec::new();
    for bridge in running {
        if bridge.executable != client && canonical(&bridge.executable) != canonical(&client) {
            notice(
                &format!(
                    "Another {} copy is running from {} (pid {}); quit it so the installed app at {} handles notifications.",
                    app_label(profile),
                    bridge.executable.display(),
                    bridge.pid,
                    destination.display()
                ),
                false,
            );
            continue;
        }
        if installed.is_none_or(|changed| bridge.started < changed) {
            stale.push(bridge);
        }
    }
    stale
}

#[cfg(target_os = "macos")]
fn describe(bridges: &[RunningBridge]) -> String {
    bridges
        .iter()
        .map(|bridge| {
            format!(
                "pid {}, started {}",
                bridge.pid,
                chrono::DateTime::<chrono::Local>::from(bridge.started).format("%a %b %-d %H:%M")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Quit approved stale processes, then start the installed app. Failures stay
/// notices: the Dashboard still attaches and its next Bridge request launches
/// whatever app owns the endpoint.
#[cfg(target_os = "macos")]
fn restart_processes(
    profile: ovrcr::protocol::bridge_installation::BridgeProfile,
    destination: &std::path::Path,
    stale: &[RunningBridge],
    processes: &mut impl BridgeProcesses,
    execute: &mut impl FnMut(
        &mut std::process::Command,
        std::time::Duration,
    ) -> Result<std::process::Output>,
    notice: &mut impl FnMut(&str, bool),
) {
    use std::time::{Duration, Instant};
    let label = app_label(profile);
    for bridge in stale {
        if processes.terminate(bridge).is_err() && !processes.exited(bridge) {
            notice(
                &format!(
                    "Could not quit {label} (pid {}); quit it in Activity Monitor, then restart OVRCR.",
                    bridge.pid
                ),
                false,
            );
            return;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while !stale.iter().all(|bridge| processes.exited(bridge)) {
        if Instant::now() >= deadline {
            notice(
                &format!(
                    "{label} ({}) did not quit within 3 seconds; quit it in Activity Monitor, then restart OVRCR.",
                    describe(stale)
                ),
                false,
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let relaunched = execute(
        std::process::Command::new("/usr/bin/open")
            .arg("-g")
            .arg(destination),
        Duration::from_secs(5),
    )
    .is_ok_and(|output| output.status.success());
    if relaunched {
        notice(
            &format!("Restarted {label} from {}.", destination.display()),
            true,
        );
    } else {
        notice(
            &format!(
                "Quit the old {label}; the Dashboard starts the installed app with its next notification request."
            ),
            false,
        );
    }
}

#[cfg(target_os = "macos")]
struct SystemProcesses;

#[cfg(target_os = "macos")]
impl BridgeProcesses for SystemProcesses {
    /// LaunchServices identifies the app by bundle ID even after its bundle
    /// was replaced on disk, unlike an executable-path or name match.
    fn running(&mut self, bundle_id: &str) -> Result<Vec<RunningBridge>> {
        use std::{process::Command, time::Duration};
        let found = super::startup::command_output(
            Command::new("/usr/bin/lsappinfo")
                .arg("find")
                .arg(format!("bundleid={bundle_id}")),
            Duration::from_secs(5),
        )?;
        anyhow::ensure!(found.status.success(), "lsappinfo find failed");
        let uid = unsafe { libc::getuid() };
        let mut bridges = Vec::new();
        for serial in application_serials(&String::from_utf8_lossy(&found.stdout)) {
            let info = super::startup::command_output(
                Command::new("/usr/bin/lsappinfo")
                    .args(["info", "-only", "pid,executablepath"])
                    .arg(serial),
                Duration::from_secs(5),
            )?;
            let Some((pid, executable)) = info
                .status
                .success()
                .then(|| application_info(&String::from_utf8_lossy(&info.stdout)))
                .flatten()
            else {
                continue;
            };
            if let Some((owner, started)) = process_start(pid)
                && owner == uid
            {
                bridges.push(RunningBridge {
                    pid,
                    executable,
                    started,
                });
            }
        }
        Ok(bridges)
    }

    fn terminate(&mut self, bridge: &RunningBridge) -> Result<()> {
        anyhow::ensure!(
            process_start(bridge.pid).map(|(_, started)| started) == Some(bridge.started),
            "Bridge process changed before restart"
        );
        let pid = libc::pid_t::try_from(bridge.pid)?;
        // SAFETY: kill has no memory effects; the PID's start time was just rechecked.
        if unsafe { libc::kill(pid, libc::SIGTERM) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn exited(&mut self, bridge: &RunningBridge) -> bool {
        process_start(bridge.pid).map(|(_, started)| started) != Some(bridge.started)
    }
}

/// Owner UID and start time of a live process.
#[cfg(target_os = "macos")]
fn process_start(pid: u32) -> Option<(u32, std::time::SystemTime)> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as libc::c_int;
    // SAFETY: info is a writable proc_bsdinfo of the size passed.
    let read = unsafe {
        libc::proc_pidinfo(
            libc::c_int::try_from(pid).ok()?,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (read == size).then(|| {
        (
            info.pbi_uid,
            std::time::UNIX_EPOCH
                + std::time::Duration::new(
                    info.pbi_start_tvsec,
                    info.pbi_start_tvusec as u32 * 1000,
                ),
        )
    })
}

/// `lsappinfo find` prints serials such as `ASN:0x0-0x4a95a91-"OVRCR_Local":`.
#[cfg(target_os = "macos")]
fn application_serials(output: &str) -> Vec<String> {
    output
        .split_whitespace()
        .filter_map(|word| {
            let mut parts = word.strip_prefix("ASN:")?.splitn(3, '-');
            let (high, low) = (parts.next()?, parts.next()?);
            let hex = |part: &str| {
                part.strip_prefix("0x").is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit())
                })
            };
            (hex(high) && hex(low)).then(|| format!("ASN:{high}-{low}"))
        })
        .collect()
}

/// `lsappinfo info -only pid,executablepath` prints `"pid"=N` and
/// `"CFBundleExecutablePath"="PATH"` lines.
#[cfg(target_os = "macos")]
fn application_info(output: &str) -> Option<(u32, std::path::PathBuf)> {
    let mut pid = None;
    let mut executable = None;
    for line in output.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("\"pid\"=") {
            pid = value.parse::<u32>().ok().filter(|pid| *pid > 1);
        } else if let Some(value) = line.strip_prefix("\"CFBundleExecutablePath\"=") {
            executable = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .filter(|path| path.starts_with('/'))
                .map(std::path::PathBuf::from);
        }
    }
    Some((pid?, executable?))
}

#[cfg(target_os = "macos")]
fn validate_bundle(
    profile: ovrcr::protocol::bridge_installation::BridgeProfile,
    validator: &std::path::Path,
    bundle: &std::path::Path,
    execute: &mut impl FnMut(
        &mut std::process::Command,
        std::time::Duration,
    ) -> Result<std::process::Output>,
) -> Result<bool> {
    use ovrcr::protocol::bridge_installation::BridgeProfile;
    let mut command = std::process::Command::new("python3");
    command.arg("-I").arg(validator);
    if profile == BridgeProfile::LocalDevelopment {
        // This mode pins the local ID, display name and development marker.
        command
            .arg("--local-development")
            .env_remove("OVRCR_BRIDGE_SIGNING_IDENTITY");
    }
    Ok(
        execute(command.arg(bundle), std::time::Duration::from_secs(5))?
            .status
            .success(),
    )
}

/// Keychains can list one certificate more than once; the SHA-1 is the identity.
#[cfg(target_os = "macos")]
fn signing_identities(output: &str) -> Vec<String> {
    let mut identities: Vec<String> = Vec::new();
    for line in output.lines() {
        let mut words = line.split_whitespace();
        let Some(index) = words.next().and_then(|word| word.strip_suffix(')')) else {
            continue;
        };
        if index.parse::<u32>().is_err() {
            continue;
        }
        let Some(hash) = words.next() else {
            continue;
        };
        if hash.len() == 40
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
            && !identities
                .iter()
                .any(|known| known.eq_ignore_ascii_case(hash))
        {
            identities.push(hash.to_owned());
        }
    }
    identities
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use ovrcr::protocol::bridge_installation::{BridgeProfile, LOCAL_DEVELOPMENT_ENV};
    use ovrcr::protocol::{BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION};
    use std::{
        ffi::{OsStr, OsString},
        os::unix::process::ExitStatusExt,
        path::PathBuf,
        process::{Command, ExitStatus, Output},
    };

    struct Fixture {
        root: tempfile::TempDir,
        executable: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            let root = tempfile::tempdir().unwrap();
            let executable = root.path().join("bin/ovrcr");
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, "fixture CLI").unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { root, executable }
        }

        fn callback_sha256(&self) -> String {
            ovrcr::server::bridge_executable_sha256(
                &self.executable,
                std::time::Instant::now() + std::time::Duration::from_secs(2),
            )
            .unwrap()
        }

        fn inputs(&self, opt_in: Option<&'static OsStr>) -> Inputs<'_> {
            Inputs {
                executable: &self.executable,
                home: self.root.path(),
                opt_in,
            }
        }

        fn payload(&self, profile: BridgeProfile, installed: bool) -> PathBuf {
            let payload = if installed {
                profile.installed_assets(&self.executable).unwrap()
            } else {
                profile.adjacent_assets(&self.executable).unwrap()
            };
            std::fs::create_dir_all(payload.join("scripts")).unwrap();
            std::fs::write(payload.join("scripts/install-bridge.sh"), "fixture").unwrap();
            self.contract(&payload, PROTOCOL_VERSION, &self.callback_sha256());
            payload
        }

        fn contract(&self, payload: &std::path::Path, wire: u32, callback: &str) {
            std::fs::create_dir_all(payload.join("native/bridge")).unwrap();
            std::fs::write(
                payload.join("native/bridge/expected-contract.json"),
                serde_json::json!({
                    "wire": wire,
                    "schema": BRIDGE_SCHEMA_VERSION,
                    "callback_sha256": callback,
                })
                .to_string(),
            )
            .unwrap();
        }
    }

    #[derive(Debug)]
    struct Invocation {
        program: OsString,
        args: Vec<OsString>,
        identity_removed: bool,
    }

    impl Invocation {
        fn capture(command: &Command) -> Self {
            Self {
                program: command.get_program().to_owned(),
                args: command.get_args().map(OsStr::to_owned).collect(),
                identity_removed: command
                    .get_envs()
                    .any(|(key, value)| key == "OVRCR_BRIDGE_SIGNING_IDENTITY" && value.is_none()),
            }
        }
    }

    fn output(success: bool, stdout: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(if success { 0 } else { 256 }),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    struct NoBridges;

    impl BridgeProcesses for NoBridges {
        fn running(&mut self, _: &str) -> Result<Vec<RunningBridge>> {
            Ok(Vec::new())
        }
        fn terminate(&mut self, _: &RunningBridge) -> Result<()> {
            panic!("no Bridge process is running")
        }
        fn exited(&mut self, _: &RunningBridge) -> bool {
            true
        }
    }

    fn no_identity() -> std::result::Result<String, std::env::VarError> {
        panic!("local or declined setup must not read a signing identity")
    }

    #[test]
    fn local_startup_plans_only_fixed_local_validation_and_ad_hoc_installation() {
        let fixture = Fixture::new();
        let profile = BridgeProfile::LocalDevelopment;
        let payload = fixture.payload(profile, false);
        fixture.payload(BridgeProfile::Production, true);
        let mut calls = Vec::new();
        let mut prompts = Vec::new();
        run_at(
            fixture.inputs(Some(OsStr::new("1"))),
            &mut |prompt| {
                prompts.push(prompt.to_owned());
                Ok(true)
            },
            &mut |command, _| {
                calls.push(Invocation::capture(command));
                Ok(output(true, b""))
            },
            &mut no_identity,
            &mut |_, _| {},
            &mut NoBridges,
        )
        .unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("Install OVRCR Local"));
        assert!(prompts[0].contains("ad-hoc signing"));
        assert!(prompts[0].contains("No Developer ID certificate or Keychain signing identity"));
        assert!(prompts[0].contains("Notification permission is requested separately"));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].program, "python3");
        assert_eq!(
            calls[0].args,
            [
                OsString::from("-I"),
                payload
                    .join("native/bridge/validate-bundle.py")
                    .into_os_string(),
                OsString::from("--local-development"),
                payload.join(profile.app_name()).into_os_string(),
            ]
        );
        assert_eq!(calls[1].program, "/bin/sh");
        assert_eq!(
            calls[1].args,
            [
                payload.join("scripts/install-bridge.sh").into_os_string(),
                OsString::from("--local-development"),
                OsString::from("--destination"),
                profile.destination(fixture.root.path()).into_os_string(),
                payload.join(profile.app_name()).into_os_string(),
            ]
        );
        assert!(calls.iter().all(|call| call.identity_removed));
        assert!(calls.iter().all(|call| call.program != "/usr/bin/security"));
        assert!(
            !BridgeProfile::Production
                .destination(fixture.root.path())
                .exists()
        );
    }

    #[test]
    fn local_startup_missing_assets_never_falls_back_to_production() {
        let fixture = Fixture::new();
        fixture.payload(BridgeProfile::Production, true);
        fixture.payload(BridgeProfile::Production, false);
        let mut notices = Vec::new();
        run_at(
            fixture.inputs(Some(OsStr::new("1"))),
            &mut |_| panic!("missing local assets must not prompt"),
            &mut |_, _| panic!("missing local assets must not run a tool"),
            &mut no_identity,
            &mut |notice, success| notices.push((notice.to_owned(), success)),
            &mut NoBridges,
        )
        .unwrap();
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].0.contains("OVRCR Local is not installed")
                && notices[0].0.contains("ovrcr-local-development")
                && notices[0].0.contains(&format!(
                    "`OVRCR_BRIDGE_LOCAL_DEVELOPMENT=1 just install-startup-assets {}`",
                    fixture.executable.display()
                )),
            "{}",
            notices[0].0
        );
        assert!(!notices[0].1);
    }

    #[test]
    fn local_installed_asset_directory_is_preferred_without_production_assets() {
        let fixture = Fixture::new();
        let payload = fixture.payload(BridgeProfile::LocalDevelopment, true);
        fixture.payload(BridgeProfile::LocalDevelopment, false);
        let mut calls = Vec::new();
        run_at(
            fixture.inputs(Some(OsStr::new("1"))),
            &mut |_| Ok(false),
            &mut |command, _| {
                calls.push(Invocation::capture(command));
                Ok(output(true, b""))
            },
            &mut no_identity,
            &mut |_, _| {},
            &mut NoBridges,
        )
        .unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].args[1],
            payload
                .join("native/bridge/validate-bundle.py")
                .into_os_string()
        );
        assert!(
            calls[0]
                .args
                .contains(&OsString::from("--local-development"))
        );
    }

    #[test]
    fn valid_existing_local_installation_only_validates_metadata_and_seal() {
        let fixture = Fixture::new();
        fixture.payload(BridgeProfile::LocalDevelopment, false);
        let destination = BridgeProfile::LocalDevelopment.destination(fixture.root.path());
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(destination.join("keep"), "unchanged").unwrap();
        let mut calls = Vec::new();
        run_at(
            fixture.inputs(Some(OsStr::new("1"))),
            &mut |_| panic!("valid local installation must not prompt"),
            &mut |command, _| {
                calls.push(Invocation::capture(command));
                Ok(output(true, b""))
            },
            &mut no_identity,
            &mut |_, _| {},
            &mut NoBridges,
        )
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].args.last().unwrap().as_os_str(),
            destination.as_os_str()
        );
        assert_eq!(calls[1].program, "/usr/bin/codesign");
        assert_eq!(
            calls[1].args,
            [
                OsString::from("--verify"),
                OsString::from("--deep"),
                OsString::from("--strict"),
                destination.clone().into_os_string(),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("keep")).unwrap(),
            "unchanged"
        );
    }

    #[test]
    fn invalid_existing_local_installation_never_offers_repair() {
        for failed_program in ["python3", "/usr/bin/codesign"] {
            let fixture = Fixture::new();
            fixture.payload(BridgeProfile::LocalDevelopment, false);
            let destination = BridgeProfile::LocalDevelopment.destination(fixture.root.path());
            std::fs::create_dir_all(&destination).unwrap();
            std::fs::write(destination.join("keep"), "unchanged").unwrap();
            let mut calls = Vec::new();
            let mut notices = Vec::new();
            run_at(
                fixture.inputs(Some(OsStr::new("1"))),
                &mut |_| panic!("existing local destination must not offer repair"),
                &mut |command, _| {
                    calls.push(Invocation::capture(command));
                    Ok(output(command.get_program() != failed_program, b""))
                },
                &mut no_identity,
                &mut |notice, _| notices.push(notice.to_owned()),
                &mut NoBridges,
            )
            .unwrap();
            assert!(!calls.iter().any(|call| call.program == "/bin/sh"));
            assert_eq!(
                calls[0].args.last().unwrap().as_os_str(),
                destination.as_os_str()
            );
            assert!(notices[0].contains("fresh-only"));
            assert_eq!(
                std::fs::read_to_string(destination.join("keep")).unwrap(),
                "unchanged"
            );
        }
    }

    #[test]
    fn dangling_local_destination_is_refused_without_validation_or_replacement() {
        let fixture = Fixture::new();
        fixture.payload(BridgeProfile::LocalDevelopment, false);
        let destination = BridgeProfile::LocalDevelopment.destination(fixture.root.path());
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("absent", &destination).unwrap();
        let mut notices = Vec::new();
        run_at(
            fixture.inputs(Some(OsStr::new("1"))),
            &mut |_| panic!("symlink must not prompt"),
            &mut |_, _| panic!("symlink must not run a tool"),
            &mut no_identity,
            &mut |notice, _| notices.push(notice.to_owned()),
            &mut NoBridges,
        )
        .unwrap();
        assert!(notices[0].contains("fresh-only"));
        assert_eq!(
            std::fs::read_link(destination).unwrap(),
            PathBuf::from("absent")
        );
    }

    #[test]
    fn invalid_opt_in_refuses_every_startup_command_and_offer() {
        use std::os::unix::ffi::OsStrExt;
        let fixture = Fixture::new();
        fixture.payload(BridgeProfile::Production, false);
        for value in [
            OsStr::new("true"),
            OsStr::new("01"),
            OsStr::new(" 1"),
            OsStr::new("1 "),
            OsStr::from_bytes(b"\xff"),
        ] {
            let error = run_at(
                Inputs {
                    executable: &fixture.executable,
                    home: fixture.root.path(),
                    opt_in: Some(value),
                },
                &mut |_| panic!("invalid profile must not prompt"),
                &mut |_, _| panic!("invalid profile must not execute"),
                &mut no_identity,
                &mut |_, _| {},
                &mut NoBridges,
            )
            .unwrap_err();
            assert!(error.to_string().contains(LOCAL_DEVELOPMENT_ENV));
        }
    }

    #[test]
    fn production_default_empty_and_zero_keep_identity_discovery_and_installer_arguments() {
        for opt_in in [None, Some(OsStr::new("")), Some(OsStr::new("0"))] {
            let fixture = Fixture::new();
            let profile = BridgeProfile::Production;
            let payload = fixture.payload(profile, false);
            let mut calls = Vec::new();
            let mut identity_reads = 0;
            run_at(
                fixture.inputs(opt_in),
                &mut |prompt| {
                    assert!(prompt.contains("Install OVRCR Bridge"));
                    Ok(true)
                },
                &mut |command, _| {
                    let security = command.get_program() == "/usr/bin/security";
                    calls.push(Invocation::capture(command));
                    Ok(output(
                        true,
                        if security {
                            b"1) 0123456789ABCDEF0123456789ABCDEF01234567 \"Certificate\"\n"
                        } else {
                            b""
                        },
                    ))
                },
                &mut || {
                    identity_reads += 1;
                    Err(std::env::VarError::NotPresent)
                },
                &mut |_, _| {},
                &mut NoBridges,
            )
            .unwrap();
            assert_eq!(identity_reads, 1);
            assert_eq!(calls.len(), 3);
            assert_eq!(calls[1].program, "/usr/bin/security");
            assert_eq!(
                calls[1].args,
                ["find-identity", "-v", "-p", "codesigning"].map(OsString::from)
            );
            assert_eq!(
                calls[2].args,
                [
                    payload.join("scripts/install-bridge.sh").into_os_string(),
                    OsString::from("--identity"),
                    OsString::from("0123456789ABCDEF0123456789ABCDEF01234567"),
                    OsString::from("--destination"),
                    profile.destination(fixture.root.path()).into_os_string(),
                    payload.join(profile.app_name()).into_os_string(),
                ]
            );
            assert!(calls.iter().all(|call| !call.identity_removed));
            assert!(
                calls
                    .iter()
                    .all(|call| !call.args.contains(&OsString::from("--local-development")))
            );
        }
    }

    #[test]
    fn production_decline_and_invalid_package_do_not_query_an_identity() {
        for valid in [true, false] {
            let fixture = Fixture::new();
            fixture.payload(BridgeProfile::Production, false);
            let mut calls = Vec::new();
            let result = run_at(
                fixture.inputs(None),
                &mut |_| {
                    assert!(valid, "invalid payload must not prompt");
                    Ok(false)
                },
                &mut |command, _| {
                    calls.push(Invocation::capture(command));
                    Ok(output(valid, b""))
                },
                &mut no_identity,
                &mut |_, _| {},
                &mut NoBridges,
            );
            assert_eq!(result.is_ok(), valid);
            assert_eq!(calls.len(), 1);
        }
    }

    #[test]
    fn production_still_refuses_ad_hoc_or_ambiguous_signing_identities() {
        for explicit in [Some("-"), Some(""), None] {
            let fixture = Fixture::new();
            fixture.payload(BridgeProfile::Production, false);
            let mut calls = Vec::new();
            let error = run_at(
                fixture.inputs(None),
                &mut |_| Ok(true),
                &mut |command, _| {
                    calls.push(Invocation::capture(command));
                    Ok(output(true, b""))
                },
                &mut || {
                    explicit
                        .map(str::to_owned)
                        .ok_or(std::env::VarError::NotPresent)
                },
                &mut |_, _| {},
                &mut NoBridges,
            )
            .unwrap_err();
            assert!(error.to_string().contains("identity"));
            assert!(!calls.iter().any(|call| call.program == "/bin/sh"));
        }
    }

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

    #[test]
    fn signing_identity_listed_twice_is_one_identity() {
        let hash = "0123456789ABCDEF0123456789ABCDEF01234567";
        let output = format!(
            "  1) {hash} \"Developer ID Application: Fixture\"\n  2) {} \"Developer ID Application: Fixture\"\n     2 valid identities found\n",
            hash.to_ascii_lowercase()
        );
        assert_eq!(super::signing_identities(&output), [hash]);
        let other = "89ABCDEF0123456789ABCDEF0123456789ABCDEF";
        assert_eq!(
            super::signing_identities(&format!("{output}  3) {other} \"Other\"\n")),
            [hash, other]
        );
    }

    #[test]
    fn duplicate_keychain_listing_still_installs_with_that_identity() {
        let fixture = Fixture::new();
        let payload = fixture.payload(BridgeProfile::Production, false);
        let hash = "0123456789ABCDEF0123456789ABCDEF01234567";
        let mut calls = Vec::new();
        run_at(
            fixture.inputs(None),
            &mut |_| Ok(true),
            &mut |command, _| {
                let security = command.get_program() == "/usr/bin/security";
                calls.push(Invocation::capture(command));
                Ok(output(
                    true,
                    if security {
                        format!("  1) {hash} \"Dev\"\n  2) {hash} \"Dev\"\n     2 valid identities found\n").into_bytes()
                    } else {
                        Vec::new()
                    }
                    .as_slice(),
                ))
            },
            &mut || Err(std::env::VarError::NotPresent),
            &mut |_, _| {},
            &mut NoBridges,
        )
        .unwrap();
        let installer = calls.iter().find(|call| call.program == "/bin/sh").unwrap();
        assert_eq!(
            installer.args[..3],
            [
                payload.join("scripts/install-bridge.sh").into_os_string(),
                OsString::from("--identity"),
                OsString::from(hash),
            ]
        );
    }

    #[test]
    fn stale_assets_refuse_repair_and_name_the_refresh_command() {
        for (wire, callback, reason) in [
            (
                PROTOCOL_VERSION - 1,
                None,
                format!(
                    "protocol {}, this CLI {PROTOCOL_VERSION}",
                    PROTOCOL_VERSION - 1
                ),
            ),
            (
                PROTOCOL_VERSION,
                Some("0".repeat(64)),
                "callback CLI is a different build".to_owned(),
            ),
        ] {
            let fixture = Fixture::new();
            let payload = fixture.payload(BridgeProfile::Production, true);
            fixture.contract(
                &payload,
                wire,
                &callback.unwrap_or_else(|| fixture.callback_sha256()),
            );
            let mut notices = Vec::new();
            run_at(
                fixture.inputs(None),
                &mut |_| panic!("stale assets must not offer a repair"),
                &mut |_, _| panic!("stale assets must not run a tool"),
                &mut no_identity,
                &mut |notice, success| notices.push((notice.to_owned(), success)),
                &mut NoBridges,
            )
            .unwrap();
            assert_eq!(notices.len(), 1);
            let (notice, success) = &notices[0];
            assert!(!success);
            assert!(
                notice.contains(&reason)
                    && notice.contains(&payload.display().to_string())
                    && notice.contains(&format!(
                        "`just install-startup-assets {}`",
                        fixture.executable.display()
                    )),
                "{notice}"
            );
        }
    }

    #[test]
    fn missing_production_assets_distinguish_missing_and_installed_app() {
        for installed in [false, true] {
            let fixture = Fixture::new();
            let destination = BridgeProfile::Production.destination(fixture.root.path());
            if installed {
                std::fs::create_dir_all(&destination).unwrap();
            }
            let mut notices = Vec::new();
            run_at(
                fixture.inputs(None),
                &mut |_| panic!("missing assets must not prompt"),
                &mut |_, _| panic!("missing assets must not run a tool"),
                &mut no_identity,
                &mut |notice, _| notices.push(notice.to_owned()),
                &mut NoBridges,
            )
            .unwrap();
            assert_eq!(notices.len(), 1);
            let expected = if installed {
                "OVRCR Bridge is installed but cannot be checked or repaired"
            } else {
                "OVRCR Bridge is not installed"
            };
            assert!(
                notices[0].contains(expected)
                    && notices[0].contains(
                        &BridgeProfile::Production
                            .installed_assets(&fixture.executable)
                            .unwrap()
                            .display()
                            .to_string()
                    )
                    && notices[0].contains("just install-startup-assets"),
                "{}",
                notices[0]
            );
        }
    }

    struct FakeBridges {
        running: Vec<RunningBridge>,
        terminated: Vec<u32>,
        quits: bool,
    }

    impl FakeBridges {
        fn new(running: Vec<RunningBridge>) -> Self {
            Self {
                running,
                terminated: Vec::new(),
                quits: true,
            }
        }
    }

    impl BridgeProcesses for FakeBridges {
        fn running(&mut self, bundle_id: &str) -> Result<Vec<RunningBridge>> {
            assert_eq!(bundle_id, "com.ovrcr.bridge");
            Ok(self
                .running
                .iter()
                .filter(|bridge| !(self.quits && self.terminated.contains(&bridge.pid)))
                .cloned()
                .collect())
        }
        fn terminate(&mut self, bridge: &RunningBridge) -> Result<()> {
            self.terminated.push(bridge.pid);
            Ok(())
        }
        fn exited(&mut self, bridge: &RunningBridge) -> bool {
            self.quits && self.terminated.contains(&bridge.pid)
        }
    }

    fn installed_app(fixture: &Fixture) -> (PathBuf, PathBuf) {
        let destination = BridgeProfile::Production.destination(fixture.root.path());
        let client = BridgeProfile::Production.client_path(fixture.root.path());
        std::fs::create_dir_all(client.parent().unwrap()).unwrap();
        std::fs::write(&client, "installed Bridge").unwrap();
        (destination, client)
    }

    fn bridge(
        pid: u32,
        executable: &std::path::Path,
        started: std::time::SystemTime,
    ) -> RunningBridge {
        RunningBridge {
            pid,
            executable: executable.to_owned(),
            started,
        }
    }

    #[test]
    fn valid_installation_restarts_only_the_stale_running_bridge_after_approval() {
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        for approve in [true, false] {
            let fixture = Fixture::new();
            fixture.payload(BridgeProfile::Production, false);
            let (destination, client) = installed_app(&fixture);
            let foreign = fixture.root.path().join("elsewhere/OVRCRBridge");
            let mut processes = FakeBridges::new(vec![
                bridge(41, &client, UNIX_EPOCH + Duration::from_secs(1_000)),
                bridge(42, &client, SystemTime::now() + Duration::from_secs(3_600)),
                bridge(43, &foreign, UNIX_EPOCH + Duration::from_secs(1_000)),
            ]);
            let mut prompts = Vec::new();
            let mut calls = Vec::new();
            let mut notices = Vec::new();
            run_at(
                fixture.inputs(None),
                &mut |prompt| {
                    prompts.push(prompt.to_owned());
                    Ok(approve)
                },
                &mut |command, _| {
                    calls.push(Invocation::capture(command));
                    Ok(output(true, b""))
                },
                &mut no_identity,
                &mut |notice, success| notices.push((notice.to_owned(), success)),
                &mut processes,
            )
            .unwrap();
            assert_eq!(prompts.len(), 1);
            assert!(
                prompts[0].starts_with("Restart OVRCR Bridge\nRunning: pid 41, started ")
                    && !prompts[0].contains("pid 42")
                    && !prompts[0].contains("pid 43")
                    && prompts[0].contains(&destination.display().to_string()),
                "{}",
                prompts[0]
            );
            assert!(notices.iter().any(|(notice, success)| !success
                && notice.contains("pid 43")
                && notice.contains(&foreign.display().to_string())));
            assert!(!calls.iter().any(|call| call.program == "/bin/sh"));
            let open = calls.iter().find(|call| call.program == "/usr/bin/open");
            if approve {
                assert_eq!(processes.terminated, [41]);
                assert_eq!(
                    open.unwrap().args,
                    [OsString::from("-g"), destination.clone().into_os_string()]
                );
                assert!(notices.iter().any(|(notice, success)| *success
                    && notice
                        == &format!("Restarted OVRCR Bridge from {}.", destination.display())));
            } else {
                assert!(processes.terminated.is_empty());
                assert!(open.is_none());
                assert!(notices.iter().any(|(notice, success)| !success
                    && notice.contains("older than the installed app")
                    && notice.contains("pid 41")));
            }
        }
    }

    #[test]
    fn current_running_bridge_needs_no_offer() {
        use std::time::{Duration, SystemTime};
        let fixture = Fixture::new();
        fixture.payload(BridgeProfile::Production, false);
        let (_, client) = installed_app(&fixture);
        let mut processes = FakeBridges::new(vec![bridge(
            42,
            &client,
            SystemTime::now() + Duration::from_secs(3_600),
        )]);
        run_at(
            fixture.inputs(None),
            &mut |_| panic!("a current Bridge must not prompt"),
            &mut |_, _| Ok(output(true, b"")),
            &mut no_identity,
            &mut |notice, _| panic!("unexpected notice: {notice}"),
            &mut processes,
        )
        .unwrap();
        assert!(processes.terminated.is_empty());
    }

    #[test]
    fn accepted_repair_restarts_the_replaced_bridge_without_a_second_prompt() {
        use std::time::{Duration, UNIX_EPOCH};
        let fixture = Fixture::new();
        let payload = fixture.payload(BridgeProfile::Production, false);
        let (destination, client) = installed_app(&fixture);
        let mut processes = FakeBridges::new(vec![bridge(
            41,
            &client,
            UNIX_EPOCH + Duration::from_secs(1_000),
        )]);
        let mut prompts = Vec::new();
        let mut calls = Vec::new();
        let mut notices = Vec::new();
        run_at(
            fixture.inputs(None),
            &mut |prompt| {
                prompts.push(prompt.to_owned());
                Ok(true)
            },
            &mut |command, _| {
                let invocation = Invocation::capture(command);
                // The installed bundle is stale; the packaged one validates.
                let stale = invocation.program == "python3"
                    && invocation.args.last() == Some(&destination.clone().into_os_string());
                calls.push(invocation);
                Ok(output(!stale, b""))
            },
            &mut || Ok("0123456789ABCDEF0123456789ABCDEF01234567".into()),
            &mut |notice, success| notices.push((notice.to_owned(), success)),
            &mut processes,
        )
        .unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(
            prompts[0].starts_with("Repair or update OVRCR Bridge\n")
                && prompts[0].contains("The running Bridge (pid 41, started "),
            "{}",
            prompts[0]
        );
        let programs: Vec<_> = calls.iter().map(|call| call.program.clone()).collect();
        assert_eq!(
            programs,
            ["python3", "python3", "/bin/sh", "/usr/bin/open"].map(OsString::from)
        );
        assert_eq!(
            calls[1].args.last().unwrap(),
            &payload.join("OVRCR Bridge.app").into_os_string()
        );
        assert_eq!(processes.terminated, [41]);
        assert!(
            notices
                .iter()
                .any(|(notice, _)| notice.starts_with("Installed OVRCR Bridge at "))
        );
        assert!(
            notices
                .iter()
                .any(|(notice, success)| *success && notice.starts_with("Restarted OVRCR Bridge"))
        );
    }

    #[test]
    fn bridge_that_does_not_quit_is_reported_without_relaunch() {
        use std::time::{Duration, UNIX_EPOCH};
        let fixture = Fixture::new();
        fixture.payload(BridgeProfile::Production, false);
        let (_, client) = installed_app(&fixture);
        let mut processes = FakeBridges::new(vec![bridge(
            41,
            &client,
            UNIX_EPOCH + Duration::from_secs(1_000),
        )]);
        processes.quits = false;
        let mut calls = Vec::new();
        let mut notices = Vec::new();
        run_at(
            fixture.inputs(None),
            &mut |_| Ok(true),
            &mut |command, _| {
                calls.push(Invocation::capture(command));
                Ok(output(true, b""))
            },
            &mut no_identity,
            &mut |notice, success| notices.push((notice.to_owned(), success)),
            &mut processes,
        )
        .unwrap();
        assert_eq!(processes.terminated, [41]);
        assert!(!calls.iter().any(|call| call.program == "/usr/bin/open"));
        assert!(notices.iter().any(|(notice, success)| !success
            && notice.contains("did not quit")
            && notice.contains("Activity Monitor")));
    }

    #[test]
    fn launch_services_output_parses_serials_and_process_identity() {
        assert_eq!(
            super::application_serials(
                "ASN:0x0-0x4a95a91-\"OVRCR\":\nASN:0x0-0x47ca7c6-\"OVRCR_Local\":\nASN:zz-0x1:\n"
            ),
            ["ASN:0x0-0x4a95a91", "ASN:0x0-0x47ca7c6"]
        );
        assert!(super::application_serials("").is_empty());
        assert_eq!(
            super::application_info(
                "\"pid\"=26835\n\"CFBundleExecutablePath\"=\"/Users/me/Applications/OVRCR Bridge.app/Contents/MacOS/OVRCRBridge\"\n"
            ),
            Some((
                26835,
                PathBuf::from("/Users/me/Applications/OVRCR Bridge.app/Contents/MacOS/OVRCRBridge")
            ))
        );
        assert_eq!(
            super::application_info("\"pid\"=1\n\"CFBundleExecutablePath\"=\"/x\"\n"),
            None
        );
        assert_eq!(super::application_info("\"pid\"=7\n"), None);
    }
}
