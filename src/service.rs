use crate::config::RegistryPath;
use crate::protocol::ErrorCode;
use crate::protocol::client;
use crate::server::{ServerPaths, connect_if_running, connect_raw_if_running, handshake};
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use directories::BaseDirs;
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const SERVICE_LABEL: &str = "com.ovrcr.server";
pub const SERVICE_NAME: &str = "ovrcr.service";
const ENVIRONMENT_FILE_VARIABLE: &str = "OVRCR_ENV_FILE";

#[derive(Clone, Debug, Args)]
pub struct ServiceArgs {
    #[command(subcommand)]
    pub command: ServiceCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum ServiceCommand {
    Install {
        #[arg(long)]
        environment_file: Option<PathBuf>,
        /// Terminate live sessions and task runs on a managed server before replacing it.
        #[arg(long)]
        kill_sessions: bool,
    },
    Start,
    Stop,
    Status,
    Uninstall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServicePlatform {
    Launchd,
    Systemd,
}

#[derive(Clone, Debug)]
pub struct ServiceConfig {
    pub platform: ServicePlatform,
    pub service_name: String,
    pub definition_path: PathBuf,
    pub manager_executable: PathBuf,
    pub executable: PathBuf,
    pub registry_path: PathBuf,
    pub server_paths: ServerPaths,
    pub environment: Vec<(String, String)>,
}

impl ServiceConfig {
    pub fn resolve() -> Result<Self> {
        let registry_override = env::var_os(crate::config::HOME_ENV).is_some()
            || env::var_os(crate::config::CONFIG_ENV).is_some();
        let registry_path = absolute(RegistryPath::resolve()?.0)?;
        let mut server_paths = ServerPaths::resolve()?;
        server_paths.socket = absolute(server_paths.socket)?;
        let executable = absolute(env::current_exe().context("resolve OVRCR executable")?)?;
        let base = BaseDirs::new().context("resolve user directories")?;

        let identity = registry_override.then(|| stable_path_id(&registry_path));
        #[cfg(target_os = "macos")]
        let (platform, service_name, definition_path, manager_executable) = {
            let service_name = identity
                .map(|id| format!("{SERVICE_LABEL}-{id:016x}"))
                .unwrap_or_else(|| SERVICE_LABEL.into());
            let definition_path = base
                .home_dir()
                .join("Library/LaunchAgents")
                .join(format!("{service_name}.plist"));
            (
                ServicePlatform::Launchd,
                service_name,
                definition_path,
                PathBuf::from("/bin/launchctl"),
            )
        };
        #[cfg(target_os = "linux")]
        let (platform, service_name, definition_path, manager_executable) = {
            let service_name = identity
                .map(|id| format!("ovrcr-{id:016x}.service"))
                .unwrap_or_else(|| SERVICE_NAME.into());
            let definition_path = base.config_dir().join("systemd/user").join(&service_name);
            (
                ServicePlatform::Systemd,
                service_name,
                definition_path,
                PathBuf::from("/usr/bin/systemctl"),
            )
        };
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        bail!("background service is supported only on macOS and Linux");

        let environment = ["SHELL", "PATH", "HOME", "PI_CODING_AGENT_DIR"]
            .into_iter()
            .filter_map(|key| env::var(key).ok().map(|value| (key.into(), value)))
            .collect();
        Ok(Self {
            platform,
            service_name,
            definition_path,
            manager_executable,
            executable,
            registry_path,
            server_paths,
            environment,
        })
    }

    fn validate(&self) -> Result<()> {
        for (name, path) in [
            ("service definition", &self.definition_path),
            ("service manager", &self.manager_executable),
            ("executable", &self.executable),
            ("registry", &self.registry_path),
            ("socket", &self.server_paths.socket),
        ] {
            if !path.is_absolute() {
                bail!("{name} must be an absolute path: {}", path.display());
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct StatusOutput<'a> {
    installed: bool,
    loaded: bool,
    running: bool,
    unmanaged: bool,
    definition: &'a Path,
}

pub fn run(command: ServiceCommand, json: bool) -> Result<()> {
    run_with(&ServiceConfig::resolve()?, command, json)
}

pub fn run_with(config: &ServiceConfig, command: ServiceCommand, json: bool) -> Result<()> {
    config.validate()?;
    validate_installed_socket(config)?;
    match command {
        ServiceCommand::Install {
            environment_file,
            kill_sessions,
        } => {
            let environment_file = environment_file
                .map(absolute)
                .transpose()?
                .map(|path| {
                    if !path.is_file() {
                        bail!("environment file does not exist: {}", path.display());
                    }
                    Ok(path)
                })
                .transpose()?;
            install(config, environment_file.as_deref(), kill_sessions)?;
            print_action("installed", json)
        }
        ServiceCommand::Start => {
            if !config.definition_path.is_file() {
                bail!("OVRCR service is not installed");
            }
            start_installed(config)?;
            print_action("started", json)
        }
        ServiceCommand::Stop => {
            stop(config)?;
            print_action("stopped", json)
        }
        ServiceCommand::Status => status(config, json),
        ServiceCommand::Uninstall => {
            uninstall(config)?;
            print_action("uninstalled", json)
        }
    }
}

pub fn start_if_installed() -> Result<bool> {
    start_if_installed_with(&ServiceConfig::resolve()?)
}

pub fn start_if_installed_with(config: &ServiceConfig) -> Result<bool> {
    config.validate()?;
    if !config.definition_path.is_file() {
        return Ok(false);
    }
    validate_installed_socket(config)?;
    start_installed(config)?;
    Ok(true)
}

pub fn load_environment_file() -> Result<()> {
    let Some(path) = env::var_os(ENVIRONMENT_FILE_VARIABLE) else {
        return Ok(());
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        bail!(
            "environment file must be an absolute path: {}",
            path.display()
        );
    }
    for (key, value) in read_environment_file(&path)? {
        // SAFETY: this is called during single-threaded process startup, before the server spawns threads.
        unsafe { env::set_var(key, value) };
    }
    Ok(())
}

pub fn read_environment_file(path: &Path) -> Result<Vec<(String, String)>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("read environment file {}", path.display()))?;
    let mut values = Vec::new();
    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, raw_value) = line.split_once('=').with_context(|| {
            format!(
                "invalid environment assignment at {}:{}",
                path.display(),
                index + 1
            )
        })?;
        let key = key.trim();
        if !valid_environment_key(key) {
            bail!(
                "invalid environment key at {}:{}",
                path.display(),
                index + 1
            );
        }
        if matches!(
            key,
            crate::config::HOME_ENV
            | crate::config::CONFIG_ENV
            | "OVRCR_SOCKET"
            | ENVIRONMENT_FILE_VARIABLE
        ) {
            bail!("environment file may not set {key}");
        }
        let raw_value = raw_value.trim();
        let value = if raw_value.len() >= 2
            && ((raw_value.starts_with('"') && raw_value.ends_with('"'))
                || (raw_value.starts_with('\'') && raw_value.ends_with('\'')))
        {
            raw_value[1..raw_value.len() - 1].to_owned()
        } else {
            raw_value.to_owned()
        };
        values.push((key.to_owned(), value));
    }
    Ok(values)
}

fn valid_environment_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn install(
    config: &ServiceConfig,
    environment_file: Option<&Path>,
    kill_sessions: bool,
) -> Result<()> {
    let loaded = manager_loaded(config)?;
    let connection = managed_connection(config, loaded)?;
    fs::create_dir_all(&config.registry_path).with_context(|| {
        format!(
            "create OVRCR instance directory {}",
            config.registry_path.display()
        )
    })?;
    if let Some(mut stream) = connection
        && let Err(error) = client::shutdown(&mut stream, 1, kill_sessions)
    {
        match error.downcast_ref::<client::ServerError>() {
            Some(refused) if refused.code == ErrorCode::SessionsRemain => {
                let sessions = count_sessions(config)?;
                bail!(
                    "OVRCR server refused shutdown ({}): {sessions} session(s) open; \
                     close them or rerun install with --kill-sessions",
                    refused.message
                );
            }
            Some(refused) => bail!("OVRCR refused shutdown: {}", refused.message),
            None => return Err(error.context("request graceful OVRCR shutdown")),
        }
    }
    let definition = match config.platform {
        ServicePlatform::Launchd => launchd_definition(config, environment_file)?,
        ServicePlatform::Systemd => systemd_definition(config, environment_file)?,
    };
    write_atomic(&config.definition_path, definition.as_bytes())?;
    match config.platform {
        ServicePlatform::Launchd => {
            if loaded {
                run_manager(config, &["bootout".into(), launchd_target(config)])?;
            }
            run_manager(
                config,
                &[
                    "bootstrap".into(),
                    launchd_domain(),
                    config.definition_path.as_os_str().to_owned(),
                ],
            )?;
        }
        ServicePlatform::Systemd => {
            run_manager(config, &["--user".into(), "daemon-reload".into()])?;
            run_manager(
                config,
                &[
                    "--user".into(),
                    "enable".into(),
                    config.service_name.clone().into(),
                ],
            )?;
            run_manager(
                config,
                &[
                    "--user".into(),
                    "restart".into(),
                    config.service_name.clone().into(),
                ],
            )?;
        }
    }
    Ok(())
}

fn start_installed(config: &ServiceConfig) -> Result<()> {
    let loaded = manager_loaded(config)?;
    let connection = managed_connection(config, loaded)?;
    if connection.is_some() {
        return Ok(());
    }
    match config.platform {
        ServicePlatform::Launchd if loaded => {
            run_manager(config, &["kickstart".into(), launchd_target(config)])?;
        }
        ServicePlatform::Launchd => {
            run_manager(
                config,
                &[
                    "bootstrap".into(),
                    launchd_domain(),
                    config.definition_path.as_os_str().to_owned(),
                ],
            )?;
        }
        ServicePlatform::Systemd => {
            run_manager(
                config,
                &[
                    "--user".into(),
                    "start".into(),
                    config.service_name.clone().into(),
                ],
            )?;
        }
    }
    Ok(())
}

fn stop(config: &ServiceConfig) -> Result<()> {
    let loaded = manager_loaded(config)?;
    let connection = managed_connection(config, loaded)?;
    if let Some(mut stream) = connection {
        graceful_shutdown(&mut stream)?;
    }
    if !loaded {
        return Ok(());
    }
    match config.platform {
        ServicePlatform::Launchd => {
            run_manager(config, &["bootout".into(), launchd_target(config)])?;
        }
        ServicePlatform::Systemd => {
            run_manager(
                config,
                &[
                    "--user".into(),
                    "stop".into(),
                    config.service_name.clone().into(),
                ],
            )?;
        }
    }
    Ok(())
}

fn uninstall(config: &ServiceConfig) -> Result<()> {
    let installed = config.definition_path.is_file();
    let loaded = if installed {
        manager_loaded(config)?
    } else {
        false
    };
    let connection = managed_connection(config, loaded)?;
    if let Some(mut stream) = connection {
        graceful_shutdown(&mut stream)?;
    }
    match config.platform {
        ServicePlatform::Launchd if loaded => {
            run_manager(config, &["bootout".into(), launchd_target(config)])?;
        }
        ServicePlatform::Launchd => {}
        ServicePlatform::Systemd if installed => {
            run_manager(
                config,
                &[
                    "--user".into(),
                    "disable".into(),
                    "--now".into(),
                    config.service_name.clone().into(),
                ],
            )?;
        }
        ServicePlatform::Systemd => {}
    }
    match fs::remove_file(&config.definition_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("remove service definition"),
    }
    if config.platform == ServicePlatform::Systemd && installed {
        run_manager(config, &["--user".into(), "daemon-reload".into()])?;
    }
    Ok(())
}

fn status(config: &ServiceConfig, json: bool) -> Result<()> {
    let installed = config.definition_path.is_file();
    let loaded = if installed {
        manager_loaded(config)?
    } else {
        false
    };
    let connection = connect_if_running(&config.server_paths)?;
    let running = connection.is_some();
    let managed = match &connection {
        Some(stream) if loaded => manager_pid(config)? == Some(peer_pid(stream)?),
        _ => false,
    };
    let output = StatusOutput {
        installed,
        loaded,
        running: managed,
        unmanaged: running && !managed,
        definition: &config.definition_path,
    };
    if json {
        println!(
            "{}",
            serde_json::to_string(&output).context("serialize service status")?
        );
    } else {
        let state = if output.unmanaged {
            "unmanaged"
        } else if output.running {
            "running"
        } else if output.installed {
            "stopped"
        } else {
            "not installed"
        };
        println!("{state}\t{}", config.definition_path.display());
    }
    Ok(())
}

fn graceful_shutdown(stream: &mut UnixStream) -> Result<()> {
    client::shutdown(stream, 1, true).map_err(|error| {
        match error.downcast_ref::<client::ServerError>() {
            Some(refused) => anyhow!("OVRCR refused shutdown: {}", refused.message),
            None => error.context("request graceful OVRCR shutdown"),
        }
    })
}

fn count_sessions(config: &ServiceConfig) -> Result<usize> {
    let mut stream = connect_if_running(&config.server_paths)?
        .context("OVRCR server stopped while refusing shutdown")?;
    let snapshot = client::list(&mut stream, 1).context("request OVRCR session list")?;
    Ok(client::session_count(&snapshot))
}

// Match the socket assignment emitted by our definitions before controlling the job.
// Peer credentials below establish ownership even if a definition has been edited.
fn validate_installed_socket(config: &ServiceConfig) -> Result<()> {
    let contents = match fs::read_to_string(&config.definition_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("read installed service definition"),
    };
    let socket = path_text(&config.server_paths.socket)?;
    let matches = match config.platform {
        ServicePlatform::Launchd => {
            let mut lines = contents.lines().map(str::trim);
            lines.any(|line| line == "<key>OVRCR_SOCKET</key>")
                && lines.next()
                    == Some(format!("<string>{}</string>", xml_escape(&socket)).as_str())
        }
        ServicePlatform::Systemd => contents.lines().any(|line| {
            line == format!("Environment=\"OVRCR_SOCKET={}\"", systemd_escape(&socket))
        }),
    };
    if !matches {
        bail!(
            "requested socket does not match the installed service; use its OVRCR_SOCKET configuration"
        );
    }
    Ok(())
}

fn managed_connection(config: &ServiceConfig, loaded: bool) -> Result<Option<UnixStream>> {
    // Identify the peer before the protocol handshake: an unmanaged listener
    // may never answer it, and its identity is what decides the refusal.
    let Some(mut stream) = connect_raw_if_running(&config.server_paths)? else {
        return Ok(None);
    };
    if !loaded || manager_pid(config)? != Some(peer_pid(&stream)?) {
        bail!("refusing to take over unmanaged OVRCR server");
    }
    handshake(&mut stream, &config.server_paths.socket)?;
    Ok(Some(stream))
}

fn peer_pid(stream: &UnixStream) -> Result<u32> {
    #[cfg(target_os = "macos")]
    let (level, option, mut credentials) = (libc::SOL_LOCAL, libc::LOCAL_PEERPID, 0 as libc::pid_t);
    #[cfg(target_os = "linux")]
    let (level, option, mut credentials) = (
        libc::SOL_SOCKET,
        libc::SO_PEERCRED,
        libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        },
    );
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    // SAFETY: credentials is a correctly sized, writable value for the platform's socket option.
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            level,
            option,
            std::ptr::from_mut(&mut credentials).cast(),
            &mut length,
        )
    } == -1
    {
        return Err(std::io::Error::last_os_error()).context("identify service socket peer");
    }
    #[cfg(target_os = "macos")]
    let pid = credentials;
    #[cfg(target_os = "linux")]
    let pid = credentials.pid;
    if pid <= 0 {
        bail!("service socket peer has no live process identity");
    }
    Ok(pid as u32)
}

fn manager_pid(config: &ServiceConfig) -> Result<Option<u32>> {
    let arguments = match config.platform {
        ServicePlatform::Launchd => vec!["print".into(), launchd_target(config)],
        ServicePlatform::Systemd => vec![
            "--user".into(),
            "show".into(),
            "--property=MainPID".into(),
            "--value".into(),
            config.service_name.clone().into(),
        ],
    };
    let output = manager_output(config, &arguments)?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(output.stdout).context("read service process identity")?;
    let value = match config.platform {
        ServicePlatform::Launchd => text.lines().find_map(|line| line.strip_prefix("\tpid = ")),
        ServicePlatform::Systemd => Some(text.trim()),
    };
    Ok(value
        .and_then(|value| value.parse().ok())
        .filter(|pid| *pid > 0))
}

fn manager_loaded(config: &ServiceConfig) -> Result<bool> {
    let arguments = match config.platform {
        ServicePlatform::Launchd => vec!["print".into(), launchd_target(config)],
        ServicePlatform::Systemd => vec![
            "--user".into(),
            "is-active".into(),
            "--quiet".into(),
            config.service_name.clone().into(),
        ],
    };
    Ok(manager_output(config, &arguments)?.status.success())
}

fn run_manager(config: &ServiceConfig, arguments: &[std::ffi::OsString]) -> Result<()> {
    let output = manager_output(config, arguments)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "service manager failed ({}): {}",
            output.status,
            stderr.trim()
        );
    }
    Ok(())
}

fn manager_output(config: &ServiceConfig, arguments: &[std::ffi::OsString]) -> Result<Output> {
    Command::new(&config.manager_executable)
        .args(arguments)
        .output()
        .with_context(|| {
            format!(
                "run service manager {}",
                config.manager_executable.display()
            )
        })
}

fn launchd_domain() -> std::ffi::OsString {
    format!("gui/{}", unsafe { libc::getuid() }).into()
}

fn launchd_target(config: &ServiceConfig) -> std::ffi::OsString {
    format!("gui/{}/{}", unsafe { libc::getuid() }, config.service_name).into()
}

fn launchd_definition(config: &ServiceConfig, environment_file: Option<&Path>) -> Result<String> {
    let mut environment = service_environment(config, environment_file);
    let stdout = config.registry_path.join("service.log");
    let stderr = config.registry_path.join("service.err.log");
    let mut variables = String::new();
    for (key, value) in &mut environment {
        variables.push_str(&format!(
            "    <key>{}</key>\n    <string>{}</string>\n",
            xml_escape(key),
            xml_escape(value)
        ));
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         <key>Label</key>\n  <string>{}</string>\n\
         <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>server</string>\n  </array>\n\
         <key>EnvironmentVariables</key>\n  <dict>\n{variables}  </dict>\n\
         <key>RunAtLoad</key>\n  <true/>\n\
         <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
         <key>StandardOutPath</key>\n  <string>{}</string>\n\
         <key>StandardErrorPath</key>\n  <string>{}</string>\n\
         </dict>\n</plist>\n",
        xml_escape(&config.service_name),
        xml_escape(&path_text(&config.executable)?),
        xml_escape(&path_text(&stdout)?),
        xml_escape(&path_text(&stderr)?),
    ))
}

fn systemd_definition(config: &ServiceConfig, environment_file: Option<&Path>) -> Result<String> {
    let mut environment_lines = String::new();
    for (key, value) in service_environment(config, environment_file) {
        environment_lines.push_str(&format!(
            "Environment=\"{}={}\"\n",
            systemd_escape(&key),
            systemd_escape(&value)
        ));
    }
    Ok(format!(
        "[Unit]\nDescription=OVRCR background server\n\n\
         [Service]\nType=simple\n{environment_lines}\
         ExecStart=\"{}\" server\nRestart=on-failure\n\n\
         [Install]\nWantedBy=default.target\n",
        systemd_escape(&path_text(&config.executable)?)
    ))
}

fn service_environment(
    config: &ServiceConfig,
    environment_file: Option<&Path>,
) -> BTreeMap<String, String> {
    let mut environment = config
        .environment
        .iter()
        .cloned()
        .collect::<BTreeMap<_, _>>();
    environment.insert(
        crate::config::HOME_ENV.into(),
        config.registry_path.display().to_string(),
    );
    environment.insert(
        "OVRCR_SOCKET".into(),
        config.server_paths.socket.display().to_string(),
    );
    if let Some(path) = environment_file {
        environment.insert(ENVIRONMENT_FILE_VARIABLE.into(), path.display().to_string());
    }
    environment
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().context("service definition has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create service directory {}", parent.display()))?;
    let temporary = parent.join(format!(".ovrcr-service-{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .context("create temporary service definition")?;
        file.write_all(contents)
            .context("write service definition")?;
        file.sync_all().context("sync service definition")?;
        fs::rename(&temporary, path).context("replace service definition")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn stable_path_id(path: &Path) -> u64 {
    path.as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn absolute(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(env::current_dir()
            .context("resolve current directory")?
            .join(path))
    }
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .with_context(|| format!("path is not valid UTF-8: {}", path.display()))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn systemd_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "$$")
        .replace('%', "%%")
        .replace('\n', "\\n")
}

fn print_action(action: &str, json: bool) -> Result<()> {
    if json {
        println!("{{\"ok\":true,\"action\":\"{action}\"}}");
    } else {
        println!("service {action}");
    }
    Ok(())
}
