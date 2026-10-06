//! Fixed local installation selection shared by startup and the Dashboard.
//! This module contains no wire types, filesystem probes or executable overrides.
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

pub const LOCAL_DEVELOPMENT_ENV: &str = "OVRCR_BRIDGE_LOCAL_DEVELOPMENT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeProfile {
    Production,
    LocalDevelopment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidBridgeProfile;

impl fmt::Display for InvalidBridgeProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{LOCAL_DEVELOPMENT_ENV} must be unset, empty, 0 or 1"
        )
    }
}

impl std::error::Error for InvalidBridgeProfile {}

impl BridgeProfile {
    /// Only the exact opt-in selects the separate ad-hoc development installation.
    /// Invalid values fail closed rather than silently choosing production.
    pub fn from_opt_in(value: Option<&OsStr>) -> Result<Self, InvalidBridgeProfile> {
        match value {
            None => Ok(Self::Production),
            Some(value) if value.is_empty() || value == "0" => Ok(Self::Production),
            Some(value) if value == "1" => Ok(Self::LocalDevelopment),
            Some(_) => Err(InvalidBridgeProfile),
        }
    }

    pub fn bundle_id(self) -> &'static str {
        match self {
            Self::Production => "com.ovrcr.bridge",
            Self::LocalDevelopment => "com.ovrcr.bridge.local",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Production => "OVRCR",
            Self::LocalDevelopment => "OVRCR Local",
        }
    }

    pub fn app_name(self) -> &'static str {
        match self {
            Self::Production => "OVRCR Bridge.app",
            Self::LocalDevelopment => "OVRCR Bridge Local.app",
        }
    }

    pub fn destination(self, home: &Path) -> PathBuf {
        home.join("Applications").join(self.app_name())
    }

    pub fn client_path(self, home: &Path) -> PathBuf {
        self.destination(home).join("Contents/MacOS/OVRCRBridge")
    }

    pub fn installed_assets(self, executable: &Path) -> Option<PathBuf> {
        Some(executable.parent()?.parent()?.join(match self {
            Self::Production => "lib/ovrcr",
            Self::LocalDevelopment => "lib/ovrcr-local-development",
        }))
    }

    pub fn adjacent_assets(self, executable: &Path) -> Option<PathBuf> {
        Some(executable.parent()?.join(match self {
            Self::Production => "ovrcr-startup",
            Self::LocalDevelopment => "ovrcr-startup-local-development",
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_one_selects_local_development() {
        for value in [None, Some(OsStr::new("")), Some(OsStr::new("0"))] {
            assert_eq!(
                BridgeProfile::from_opt_in(value),
                Ok(BridgeProfile::Production)
            );
        }
        assert_eq!(
            BridgeProfile::from_opt_in(Some(OsStr::new("1"))),
            Ok(BridgeProfile::LocalDevelopment)
        );
        for value in ["true", "yes", "01", " 1", "1 ", "2", "-", "/tmp/client"] {
            assert_eq!(
                BridgeProfile::from_opt_in(Some(OsStr::new(value))),
                Err(InvalidBridgeProfile)
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_opt_in_cannot_select_production() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            BridgeProfile::from_opt_in(Some(OsStr::from_bytes(b"\xff"))),
            Err(InvalidBridgeProfile)
        );
    }

    #[test]
    fn installation_and_assets_are_fixed_and_isolated() {
        let home = Path::new("/private/home");
        let executable = Path::new("/private/home/.local/bin/ovrcr");
        for (profile, id, display, app, installed, adjacent) in [
            (
                BridgeProfile::Production,
                "com.ovrcr.bridge",
                "OVRCR",
                "OVRCR Bridge.app",
                "/private/home/.local/lib/ovrcr",
                "/private/home/.local/bin/ovrcr-startup",
            ),
            (
                BridgeProfile::LocalDevelopment,
                "com.ovrcr.bridge.local",
                "OVRCR Local",
                "OVRCR Bridge Local.app",
                "/private/home/.local/lib/ovrcr-local-development",
                "/private/home/.local/bin/ovrcr-startup-local-development",
            ),
        ] {
            assert_eq!(profile.bundle_id(), id);
            assert_eq!(profile.display_name(), display);
            assert_eq!(profile.app_name(), app);
            assert_eq!(
                profile.destination(home),
                home.join("Applications").join(app)
            );
            assert_eq!(
                profile.client_path(home),
                profile.destination(home).join("Contents/MacOS/OVRCRBridge")
            );
            assert_eq!(
                profile.installed_assets(executable),
                Some(PathBuf::from(installed))
            );
            assert_eq!(
                profile.adjacent_assets(executable),
                Some(PathBuf::from(adjacent))
            );
        }
    }
}
