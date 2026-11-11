//! Give UI tests a deterministic Pi availability precondition without changing
//! process-wide environment in a parallel test runner.

use std::process::Command;

const CHILD_TEST: &str = "OVRCR_PI_AVAILABLE_TEST_CHILD";

/// Run the caller in a one-test child with a known available executable.
///
/// Returns `true` in the parent (which should return from the test), and
/// `false` in the child so the original assertions execute there.
pub fn run_with_pi_available(test_name: &str) -> bool {
    match std::env::var(CHILD_TEST) {
        Ok(child) if child == test_name => return false,
        Ok(child) => panic!("unexpected Pi test child {child:?} for {test_name:?}"),
        Err(_) => {}
    }

    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env("OVRCR_PI_EXECUTABLE", "/bin/sh")
        .env(CHILD_TEST, test_name)
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
        .output()
        .expect("spawn the isolated Pi-available test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let transcript = format!("{stdout}{stderr}");
    let expected = format!("test {test_name} ... ok");
    assert!(
        output.status.success(),
        "Pi-available child {test_name} failed (status {}):\n{transcript}",
        output.status
    );
    assert!(
        transcript
            .lines()
            .any(|line| line.trim() == "running 1 test"),
        "Pi-available child did not run exactly one test:\n{transcript}"
    );
    assert!(
        transcript.lines().any(|line| line.trim() == expected),
        "Pi-available child did not pass {test_name}:\n{transcript}"
    );
    true
}
