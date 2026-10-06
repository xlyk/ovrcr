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
    )
}

#[cfg(target_os = "macos")]
struct Inputs<'a> {
    executable: &'a std::path::Path,
    home: &'a std::path::Path,
    opt_in: Option<&'a std::ffi::OsStr>,
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
) -> Result<()> {
    use anyhow::{Context, ensure};
    use ovrcr::protocol::bridge_installation::BridgeProfile;
    use std::{process::Command, time::Duration};
    let profile = BridgeProfile::from_opt_in(inputs.opt_in)?;
    let payload = profile
        .installed_assets(inputs.executable)
        .filter(|p| p.join("scripts/install-bridge.sh").is_file())
        .or_else(|| profile.adjacent_assets(inputs.executable))
        .context("locate startup assets")?;
    if !payload.join("scripts/install-bridge.sh").is_file() {
        notice(
            if profile == BridgeProfile::LocalDevelopment {
                "OVRCR Local install assets missing; `just run` packages the selected local-development profile with the CLI."
            } else {
                "Bridge install assets missing; `just run` packages them with the CLI."
            },
            false,
        );
        return Ok(());
    }
    let destination = profile.destination(inputs.home);
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
    let repair = if destination.exists() {
        "Repair or update"
    } else {
        "Install"
    };
    let prompt = if profile == BridgeProfile::LocalDevelopment {
        format!(
            "Install OVRCR Local\nLocation: {}\n\nInstalls a separate local-development app with ad-hoc signing.\nNo Developer ID certificate or Keychain signing identity is used.\nNotification permission is requested separately.",
            destination.display()
        )
    } else {
        format!(
            "{repair} OVRCR Bridge\nLocation: {}\n\nInstalls the app from the packaged startup assets.\nNotification permission is requested separately.",
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
            "Installed {} at {}. An already running Bridge keeps its process until you quit that selected app.",
            if profile == BridgeProfile::LocalDevelopment {
                "OVRCR Local"
            } else {
                "OVRCR Bridge"
            },
            destination.display()
        ),
        true,
    );
    Ok(())
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
    use super::*;
    use ovrcr::protocol::bridge_installation::{BridgeProfile, LOCAL_DEVELOPMENT_ENV};
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
            let root = tempfile::tempdir().unwrap();
            let executable = root.path().join("bin/ovrcr");
            Self { root, executable }
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
            payload
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
        )
        .unwrap();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].0.contains("OVRCR Local install assets missing"));
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
}
