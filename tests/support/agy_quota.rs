//! A stand-in `agy` that answers `agy -p /usage --output-format json --settings {}`.
//!
//! The script reads its answer from a state file on every call, so a test can
//! change the account response while a Server keeps running. Sensitive fixture
//! strings are embedded in the default response because real `/usage` bodies can
//! carry identifiers OVRCR must never retain or log.

#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

static SHARED_SCRIPT: Mutex<Weak<tempfile::TempDir>> = Mutex::new(Weak::new());

pub struct AgyQuota {
    /// The executable to hand the Server as `agy`.
    pub command: PathBuf,
    state: PathBuf,
    _shared: Arc<tempfile::TempDir>,
}

impl AgyQuota {
    /// Install `agy` into `dir`, signed in with default Gemini and 3p buckets.
    pub fn install(dir: &Path) -> Self {
        let shared = shared_script_dir();
        let command = dir.join("agy");
        let state = dir.join("agy-usage.json");
        std::os::unix::fs::symlink(shared.path().join("agy"), &command).unwrap();
        let agy = Self {
            command,
            state,
            _shared: shared,
        };
        agy.set_default();
        agy
    }

    pub fn set_default(&self) {
        self.set_json(&serde_json::json!({
            "status": "SUCCESS",
            "command": {
                "name": "usage",
                "data": {
                    "description": "FIXTURE_BODY fixture@example.invalid org-fixture-secret https://auth.example.invalid/oauth",
                    "email": "fixture@example.invalid",
                    "groups": [
                        {
                            "name": "Gemini",
                            "description": "Primary Gemini allowance",
                            "buckets": [
                                {
                                    "id": "gemini-weekly",
                                    "name": "Gemini Weekly",
                                    "remaining_fraction": 0.82,
                                    "reset_time": "2099-01-02T00:00:00Z",
                                    "window": "weekly"
                                },
                                {
                                    "id": "gemini-5h",
                                    "name": "Gemini 5-Hour",
                                    "remaining_fraction": 0.64,
                                    "reset_time": "2099-01-01T00:00:00Z",
                                    "window": "5h"
                                }
                            ]
                        },
                        {
                            "name": "Claude + GPT",
                            "description": "Third-party models",
                            "buckets": [
                                {
                                    "id": "3p-weekly",
                                    "name": "Claude + GPT Weekly",
                                    "remaining_fraction": 0.9,
                                    "reset_time": "2099-01-02T00:00:00Z",
                                    "window": "weekly"
                                },
                                {
                                    "id": "3p-5h",
                                    "name": "Claude + GPT 5-Hour",
                                    "remaining_fraction": 0.75,
                                    "reset_time": "2099-01-01T00:00:00Z",
                                    "window": "5h"
                                }
                            ]
                        }
                    ]
                }
            }
        }));
    }

    pub fn set_json(&self, value: &serde_json::Value) {
        let next = self.state.with_extension("next");
        std::fs::write(&next, format!("{value}\n")).unwrap();
        std::fs::rename(next, &self.state).unwrap();
    }
}

fn shared_script_dir() -> Arc<tempfile::TempDir> {
    let mut guard = SHARED_SCRIPT.lock().unwrap();
    if let Some(existing) = guard.upgrade() {
        return existing;
    }
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let script = dir.path().join("agy");
    std::fs::write(
        &script,
        b"#!/bin/sh\n\
if [ \"$*\" = '-p /usage --output-format json --settings {}' ]; then\n\
  case \"$0\" in */*) dir=\"${0%/*}\" ;; *) dir=\".\" ;; esac\n\
  while IFS= read -r line || [ -n \"$line\" ]; do printf '%s\\n' \"$line\"; done < \"$dir/agy-usage.json\"\n\
  exit 0\n\
fi\n\
if [ \"$1\" = 'models' ]; then exit 0; fi\n\
echo \"agy fixture: unsupported arguments: $*\" >&2\n\
exit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::process::Command::new(&script).arg("models").output();
    *guard = Arc::downgrade(&dir);
    dir
}
