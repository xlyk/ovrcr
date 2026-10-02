//! A stand-in `claude` that answers `claude auth status --json`.
//!
//! The script reads its answer from a state file on every call, so a test can
//! change the account while a Server keeps running. Any other arguments exit 2,
//! so a caller that runs more than the status command fails loudly.

#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct ClaudeAuth {
    /// The executable to hand the Server as `claude`.
    pub command: PathBuf,
    state: PathBuf,
}

impl ClaudeAuth {
    /// Install `claude` into `dir`, signed in through claude.ai on Max.
    pub fn install(dir: &Path) -> Self {
        let command = dir.join("claude");
        let state = dir.join("claude-auth-status.json");
        std::fs::write(
            &command,
            format!(
                "#!/bin/sh\nif [ \"$*\" = 'auth status --json' ]; then exec cat '{}'; fi\necho \"claude fixture: unsupported arguments: $*\" >&2\nexit 2\n",
                state.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755)).unwrap();
        let auth = Self { command, state };
        auth.set(true, "claude.ai", Some("max"));
        auth
    }

    /// The next `claude auth status --json` prints this state. The email is
    /// there because the real command prints identifiers OVRCR must not keep.
    pub fn set(&self, logged_in: bool, auth_method: &str, subscription_type: Option<&str>) {
        let status = serde_json::json!({
            "loggedIn": logged_in,
            "authMethod": auth_method,
            "apiProvider": "firstParty",
            "email": "fixture@example.invalid",
            "subscriptionType": subscription_type,
        });
        // Replace in one rename so a concurrent call never reads half a state.
        let next = self.state.with_extension("next");
        std::fs::write(&next, format!("{status}\n")).unwrap();
        std::fs::rename(next, &self.state).unwrap();
    }
}
