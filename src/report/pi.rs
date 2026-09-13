//! Managed interactive Pi: argv eligibility (this module grows the receiver in #89).
use std::ffi::{OsStr, OsString};
use std::path::Path;

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
