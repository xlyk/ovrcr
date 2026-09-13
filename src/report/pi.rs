//! Managed interactive Pi: argv eligibility and the harness the shared extension
//! receiver is parameterized by. Pi's `agent_settled` is a real event that fires once the
//! whole prompt is finished, so its Ready is Confirmed.
use super::{InvocationLease, extension::Harness};
use ovrcr_protocol::{AgentProvider, SampleQuality};
use ovrcr_runtime::agent_runner::HookHandler;
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

pub const EXTENSION_SOURCE: &str = include_str!("../pi-reporting-extension.mjs");

pub static HARNESS: Harness = Harness {
    provider: AgentProvider::Pi,
    name: "pi",
    display: "Pi",
    origin: "pi-extension",
    extension_file: "ovrcr-pi-reporting.mjs",
    extension_source: EXTENSION_SOURCE,
    eligible_argv,
    settled_quality: SampleQuality::Confirmed,
};

/// Interactive terminal launches only. Provider arguments are never rewritten here.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("pi")) {
        return false;
    }
    let Some(rest) = argv.get(1..) else {
        return false;
    };
    if let Some(first) = rest.first().and_then(|s| s.to_str())
        && !first.starts_with('-')
        && matches!(
            first,
            "auth"
                | "install"
                | "uninstall"
                | "remove"
                | "list"
                | "update"
                | "upgrade"
                | "config"
                | "help"
        )
    {
        return false;
    }
    rest.iter().all(|arg| {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        !matches!(
            arg,
            "--mode"
                | "-p"
                | "--print"
                | "-h"
                | "--help"
                | "-v"
                | "--version"
                | "--export"
                | "--list-models"
        )
    })
}

/// The extension text for one invocation: the helper path is embedded as a JSON string.
pub fn extension_source(binary: &Path) -> Option<String> {
    super::extension::extension_source(&HARNESS, binary)
}

pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler {
    super::extension::receiver(&HARNESS, lease, argv)
}
