//! Managed interactive Oh My Pi: argv eligibility and the harness the shared extension
//! receiver is parameterized by. Oh My Pi has no settled event of its own; its extension
//! synthesizes one from an `agent_end` that does not declare a continuation, so the Ready
//! published there is Observed — a stop hook may still continue after a clean end.
use super::{InvocationLease, extension::Harness};
use ovrcr_protocol::{AgentProvider, SampleQuality};
use ovrcr_runtime::agent_runner::HookHandler;
use std::ffi::{OsStr, OsString};
use std::path::Path;

pub const EXTENSION_SOURCE: &str = include_str!("../omp-reporting-extension.mjs");

pub static HARNESS: Harness = Harness {
    provider: AgentProvider::Omp,
    name: "omp",
    display: "Oh My Pi",
    origin: "omp-extension",
    extension_file: "ovrcr-omp-reporting.mjs",
    extension_source: EXTENSION_SOURCE,
    eligible_argv,
    settled_quality: SampleQuality::Observed,
};

pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("omp")) {
        return false;
    }
    let Some(rest) = argv.get(1..) else {
        return false;
    };
    rest.iter().all(|arg| {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        !matches!(
            arg,
            "--mode" | "-p" | "--print" | "-h" | "--help" | "-v" | "-V" | "--version"
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
