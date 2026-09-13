//! Managed interactive Oh My Pi: argv eligibility (the receiver arrives with #94).
use std::ffi::{OsStr, OsString};
use std::path::Path;

pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("omp")) {
        return false;
    }
    argv[1..].iter().all(|arg| {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        !matches!(
            arg,
            "--mode" | "-p" | "--print" | "-h" | "--help" | "-v" | "-V" | "--version"
        )
    })
}
