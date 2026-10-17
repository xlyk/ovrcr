//! Actual CLI materialization -> installed Pi SDK loader; no model/provider request.
#[path = "support/live.rs"]
mod live;

use live::{Live, PROJECT};
use ovrcr::protocol::{CreateSessionRequest, Request, Response, SessionKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

fn wait_until(timeout: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

fn wait_child(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let mut status = None;
    wait_until(timeout, || {
        status = child.try_wait().expect("poll owned Node child");
        status.is_some()
    });
    status
}

fn probe(pid: u32) -> Value {
    match Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid=,pgid=,lstart="])
        .output()
    {
        Ok(output) => json!({"pid":pid,"status":output.status.code(),
            "stdout":String::from_utf8_lossy(&output.stdout),
            "stderr":String::from_utf8_lossy(&output.stderr)}),
        Err(error) => json!({"pid":pid,"error":error.to_string()}),
    }
}

fn absent(id: i32, group: bool) -> bool {
    let result = unsafe { libc::kill(if group { -id } else { id }, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn stop_node(child: &mut Child, original: &Value) -> bool {
    if child.try_wait().ok().flatten().is_none() {
        for signal in [libc::SIGTERM, libc::SIGKILL] {
            let current = probe(child.id());
            // Only our still-current, separately created group leader may be signalled.
            let fields: Vec<_> = original["stdout"]
                .as_str()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let owned = fields.len() == 7
                && fields[0].parse::<u32>().ok() == Some(child.id())
                && fields[1].parse::<u32>().ok() == Some(child.id())
                && original["status"] == 0
                && current["status"] == 0
                && original["stdout"] == current["stdout"]
                && current["stderr"].as_str() == Some("");
            eprintln!(
                "PI_PACKAGE_SIGNAL {}",
                json!({"signal":signal,"owned":owned,"probe":current})
            );
            if !owned {
                return false;
            }
            if unsafe { libc::getpgid(child.id() as i32) } != child.id() as i32 {
                return false;
            }
            if unsafe { libc::kill(-(child.id() as i32), signal) } != 0
                && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
            {
                return false;
            }
            if wait_child(child, live::wait_deadline()).is_some() {
                break;
            }
        }
    }
    if child.try_wait().ok().flatten().is_none() {
        return false;
    }
    child.wait().is_ok()
}

#[test]
#[ignore = "dedicated installed-Pi gate: CI provisions its SDK and runs this exact test explicitly"]
fn production_materializer_loads_local_package_with_one_injected_owner() {
    let sdk = PathBuf::from(
        std::env::var_os("OVRCR_TEST_PI_PACKAGE_ROOT")
            .expect("installed-Pi gate requires OVRCR_TEST_PI_PACKAGE_ROOT"),
    );
    assert!(sdk.is_absolute());
    let metadata: Value =
        serde_json::from_slice(&fs::read(sdk.join("package.json")).unwrap()).unwrap();
    assert_eq!(metadata["name"], "@earendil-works/pi-coding-agent");
    assert_eq!(metadata["version"], "0.87.1");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut live = Live::idle().bounded();
    let capture = live.root.path().join("capture");
    let temporary = live.root.path().join("tmp");
    for path in [&capture, &temporary] {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let native = live.claude.command.parent().unwrap().join("pi");
    // This receiver records only production constructor output, then holds the PTY open.
    fs::write(
        &native,
        r#"#!/bin/sh
umask 077
printf '%s\n' "$@" > "$OVRCR_TEST_PACKAGE_CAPTURE/argv"
printf '%s\n' "$OVRCR_PI_ENTRY" > "$OVRCR_TEST_PACKAGE_CAPTURE/selection"
printf '%s\n' "$$" > "$OVRCR_TEST_PACKAGE_CAPTURE/native-pid"
printf '%s\n' "$PPID" > "$OVRCR_TEST_PACKAGE_CAPTURE/launcher-pid"
: > "$OVRCR_TEST_PACKAGE_CAPTURE/ready"
while IFS= read -r line; do
    [ "$line" = release ] && exit 0
done
"#,
    )
    .unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
    let mut pids = Vec::new();
    let mut node = None;
    let mut node_birth = Value::Null;
    let mut node_group = None;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        live.start_binary_env(&[
            ("OVRCR_TEST_PACKAGE_CAPTURE", capture.as_os_str()),
            ("TMPDIR", temporary.as_os_str()),
        ]);
        let server = live.server_pid().unwrap();
        pids.push(server);
        eprintln!("PI_PACKAGE_BIRTH {}", probe(server));
        live.ready("package-loader");
        let Response::CreatedSession(parent) =
            live.request(Request::CreateSession(CreateSessionRequest {
                project: PROJECT.into(),
                workspace: live::WORKSPACE.into(),
                name: "package-parent".into(),
                label: None,
                argv: vec![
                    live.executable.clone().into_os_string(),
                    "agent".into(),
                    "run".into(),
                    "pi".into(),
                    "--".into(),
                    "pi".into(),
                ],
                kind: SessionKind::Agent { name: "pi".into() },
            }))
        else {
            panic!("managed parent");
        };
        let launcher = parent.pid.expect("managed launcher PID");
        pids.push(launcher);
        eprintln!("PI_PACKAGE_BIRTH {}", probe(launcher));
        for group in live.session_groups() {
            live.own_group(group);
            if !pids.contains(&(group as u32)) {
                pids.push(group as u32);
                eprintln!("PI_PACKAGE_BIRTH {}", probe(group as u32));
            }
        }
        assert!(
            wait_until(live::wait_deadline() * 3, || capture.join("ready").exists()),
            "native receiver did not receive production materialization"
        );
        let native_pid: u32 = fs::read_to_string(capture.join("native-pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let received_launcher: u32 = fs::read_to_string(capture.join("launcher-pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(received_launcher, launcher);
        pids.push(native_pid);
        eprintln!("PI_PACKAGE_BIRTH {}", probe(native_pid));
        let native_group = unsafe { libc::getpgid(native_pid as i32) };
        assert_eq!(
            native_group, native_pid as i32,
            "native supervision owns a distinct group"
        );
        live.own_group(native_group);
        let argv = fs::read_to_string(capture.join("argv")).unwrap();
        let argv: Vec<_> = argv.lines().collect();
        assert_eq!(argv.first(), Some(&"-e"));
        let entry = PathBuf::from(argv[1]);
        let selected: Value =
            serde_json::from_slice(&fs::read(capture.join("selection")).unwrap()).unwrap();
        assert_eq!(selected, json!({"schema":1,"path":entry}));
        assert_eq!(entry.extension().unwrap(), "ts");
        assert!(
            entry.starts_with(&temporary),
            "entry must belong to this invocation's private TMPDIR"
        );
        let directory = entry.parent().unwrap();
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let assets = [
            (entry.file_name().unwrap().to_str().unwrap(), None),
            (
                "ovrcr-pi-runtime.ts",
                Some(include_str!("../src/ovrcr-pi-runtime.ts")),
            ),
            (
                "ovrcr-pi-interrupt.ts",
                Some(include_str!("../src/ovrcr-pi-interrupt.ts")),
            ),
            (
                "ovrcr-reporting-transport.mjs",
                Some(include_str!("../src/ovrcr-reporting-transport.mjs")),
            ),
            (
                "ovrcr-delegation-observer.mjs",
                Some(include_str!("../src/ovrcr-delegation-observer.mjs")),
            ),
            (
                "ovrcr-delegation-continuation.mjs",
                Some(include_str!("../src/ovrcr-delegation-continuation.mjs")),
            ),
        ];
        for (name, expected) in assets {
            let path = directory.join(name);
            let bytes = fs::read(&path).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            if let Some(source) = expected {
                assert_eq!(
                    bytes,
                    source.as_bytes(),
                    "actual materialized sibling {name}"
                );
            } else {
                assert!(
                    String::from_utf8_lossy(&bytes)
                        .contains(&serde_json::to_string(&live.executable).unwrap())
                );
            }
            eprintln!(
                "PI_PACKAGE_ASSET {}",
                json!({"path":path,"sha256":format!("{:x}",Sha256::digest(&bytes)),"bytes":bytes.len()})
            );
        }
        let log = capture.join("installed-node.log");
        let mut command = Command::new("node");
        command.env_clear();
        for key in ["PATH", "LANG", "LC_ALL", "LC_CTYPE"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let child = command
            .args([
                "--test",
                "--test-concurrency=1",
                "--test-reporter=tap",
                "tests/pi_package_installed.mjs",
            ])
            .current_dir(repository)
            .env("HOME", live.root.path())
            .env("TMPDIR", &temporary)
            .env("OVRCR_TEST_PI_PACKAGE_ROOT", &sdk)
            .env("OVRCR_TEST_PI_MATERIALIZED_ENTRY", &entry)
            .stdin(Stdio::null())
            .stdout(fs::File::create(&log).unwrap())
            .stderr(fs::File::options().append(true).open(&log).unwrap())
            .process_group(0)
            .spawn()
            .expect("spawn installed SDK suite");
        node_birth = probe(child.id());
        pids.push(child.id());
        node_group = Some(child.id() as i32);
        eprintln!("PI_PACKAGE_BIRTH {node_birth}");
        node = Some(child);
        let status = wait_child(node.as_mut().unwrap(), Duration::from_secs(120))
            .expect("installed SDK suite exceeded its deadline");
        let output = fs::read_to_string(&log).unwrap();
        eprintln!("{output}");
        assert!(status.success(), "installed SDK suite failed: {status}");
        for required in ["# tests 2", "# pass 2", "# fail 0", "# skipped 0"] {
            assert!(
                output.lines().any(|line| line == required),
                "missing Node receipt: {required}"
            );
        }
        assert!(
            entry.exists(),
            "real invocation must retain its materialization throughout SDK load"
        );
    }));
    if outcome.is_err()
        && let Ok(output) = fs::read_to_string(capture.join("installed-node.log"))
    {
        eprintln!("{output}");
    }
    // Cleanup is outside the assertion closure so producer, loader and timeout failures use it too.
    let node_clean = node
        .as_mut()
        .is_none_or(|child| stop_node(child, &node_birth));
    let shutdown = live::request_with_timeout(
        &live.socket,
        900,
        Request::Shutdown { kill: true },
        live::wait_deadline(),
    );
    let stopped = live.join_within(live::wait_deadline()).is_finished();
    let mut clean = node_clean && stopped && shutdown == Some(Response::Ok);
    for pid in pids {
        let gone = wait_until(live::wait_deadline(), || absent(pid as i32, false));
        let final_probe = probe(pid);
        clean &= gone && final_probe["status"] == 1 && final_probe["stderr"].as_str() == Some("");
        eprintln!(
            "PI_PACKAGE_CLEANUP {}",
            json!({"pid":pid,"kernel_absent_ESRCH":gone,"ps":final_probe})
        );
    }
    if let Some(group) = node_group {
        let gone = wait_until(live::wait_deadline(), || absent(group, true));
        clean &= gone;
        eprintln!(
            "PI_PACKAGE_GROUP_CLEANUP {}",
            json!({"pgid":group,"kernel_absent_ESRCH":gone})
        );
    }
    for group in live.owned_groups() {
        let gone = wait_until(live::wait_deadline(), || absent(group, true));
        clean &= gone;
        eprintln!(
            "PI_PACKAGE_GROUP_CLEANUP {}",
            json!({"pgid":group,"kernel_absent_ESRCH":gone})
        );
        if gone {
            live.forget_group(group);
        }
    }
    if !clean {
        let kept = std::mem::replace(&mut live.root, tempfile::tempdir().unwrap()).keep();
        eprintln!("PI_PACKAGE_PRESERVED {}", kept.display());
    }
    drop(live);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    assert!(
        clean,
        "installed package fixture cleanup incomplete; preserve recorded paths"
    );
}
