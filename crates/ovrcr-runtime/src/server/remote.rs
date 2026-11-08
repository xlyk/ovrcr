//! Supervises the optional loopback remote gateway as a child of the Server.
//!
//! The settings document is the source of truth. When `[remote] enabled` is
//! true, the Server starts `ovrcr remote serve` as its own process (same
//! binary, own process group via the default), logs it to `remote.log`, and
//! stops it on shutdown. A crash is retried with bounded backoff so autostart
//! survives a bad port without spinning.

use super::*;
use std::os::unix::fs::OpenOptionsExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const RETRY_MAX: Duration = Duration::from_secs(30);
const STABLE_AFTER: Duration = Duration::from_secs(30);

struct Running {
    child: Child,
    port: u16,
    login: Option<String>,
    started: Instant,
}

#[derive(Default)]
pub(super) struct Supervisor {
    running: Option<Running>,
    failures: u32,
    next_attempt: Option<Instant>,
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if let Some(mut running) = self.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }
}

impl Supervisor {
    fn backoff(&self) -> Duration {
        let exponent = self.failures.saturating_sub(1).min(5);
        Duration::from_secs(1_u64 << exponent).min(RETRY_MAX)
    }

    fn stop(&mut self, state: Option<&ServerState>) {
        if let Some(mut running) = self.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
            if let Some(state) = state {
                state.record_event(
                    ovrcr_protocol::EventComponent::Remote,
                    None,
                    format!("stopped on port {}", running.port),
                );
            }
        }
    }

    fn failed(&mut self, state: &ServerState, message: String) {
        self.failures = self.failures.saturating_add(1);
        self.next_attempt = Some(Instant::now() + self.backoff());
        state.record_event(ovrcr_protocol::EventComponent::Remote, None, message);
    }

    fn reconcile(&mut self, state: &ServerState, enabled: bool, port: u16, login: &Option<String>) {
        if !enabled {
            self.stop(Some(state));
            self.failures = 0;
            self.next_attempt = None;
            return;
        }

        if let Some(running) = self.running.as_mut() {
            if running.port != port || running.login != *login {
                self.stop(Some(state));
            } else {
                match running.child.try_wait() {
                    Ok(None) => return,
                    Ok(Some(status)) => {
                        let stable = running.started.elapsed() >= STABLE_AFTER;
                        self.running = None;
                        if stable {
                            self.failures = 0;
                        }
                        self.failed(state, format!("exited with status {status}"));
                    }
                    Err(error) => {
                        self.running = None;
                        self.failed(state, format!("status is unavailable: {error}"));
                    }
                }
            }
        }

        if let Some(at) = self.next_attempt
            && Instant::now() < at
        {
            return;
        }

        match spawn(state, port, login) {
            Ok(child) => {
                self.next_attempt = None;
                self.running = Some(Running {
                    child,
                    port,
                    login: login.clone(),
                    started: Instant::now(),
                });
                state.record_event(
                    ovrcr_protocol::EventComponent::Remote,
                    None,
                    format!("started on 127.0.0.1:{port}"),
                );
            }
            Err(error) => self.failed(state, format!("could not start: {error:#}")),
        }
    }
}

fn spawn(state: &ServerState, port: u16, login: &Option<String>) -> Result<Child> {
    let executable = std::env::current_exe().context("resolve the running executable")?;
    let mut command = Command::new(executable);
    command
        .arg("remote")
        .arg("serve")
        .arg("--port")
        .arg(port.to_string());
    // The settings document, not the Server's environment, decides the gate.
    // Pass the login through the environment so it never appears in argv.
    command.env_remove("OVRCR_REMOTE_LOGIN");
    if let Some(login) = login {
        command.env("OVRCR_REMOTE_LOGIN", login);
    }
    command.stdin(Stdio::null());
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(state.registry_path.join("remote.log"))
        .context("open remote.log")?;
    command.stdout(log.try_clone().context("share remote.log")?);
    command.stderr(log);
    command.spawn().context("spawn ovrcr remote serve")
}

impl ServerState {
    /// Reconcile the child with the current `[remote]` settings. Idempotent;
    /// safe to call from the settings tick, a write, and startup.
    pub(super) fn reconcile_remote(&self) {
        let remote = self.settings.lock().unwrap().report.settings.remote.clone();
        let mut supervisor = self.remote.lock().unwrap();
        supervisor.reconcile(self, remote.enabled, remote.port, &remote.login);
    }

    /// Stop the child before the Server drops its state.
    pub(super) fn stop_remote(&self) {
        self.remote.lock().unwrap().stop(Some(self));
    }
}
