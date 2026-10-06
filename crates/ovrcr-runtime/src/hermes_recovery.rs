//! Hermes resume. The session id comes from Hermes `on_session_start`, and the
//! row lives in that profile's `state.db`. Resume is `hermes --resume ID`, with
//! `-p PROFILE` when the database is under `profiles/<name>/`.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::HermesConversation;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `YYYYMMDD_HHMMSS_` plus six lowercase hex digits, the id Hermes generates.
pub fn valid_session_id(value: &str) -> bool {
    let mut parts = value.split('_');
    let (Some(date), Some(time), Some(suffix), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    date.len() == 8
        && date.bytes().all(|byte| byte.is_ascii_digit())
        && time.len() == 6
        && time.bytes().all(|byte| byte.is_ascii_digit())
        && suffix.len() == 6
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn state_db() -> Result<PathBuf> {
    let home = std::env::var_os("HERMES_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".hermes")))
        .context("Hermes home is unavailable")?;
    if !home.is_absolute() {
        bail!("Hermes home must be absolute");
    }
    Ok(home.join("state.db"))
}

/// `profiles/<name>/state.db` names a Hermes profile. The default home does not.
pub fn profile_name(state_db: &Path) -> Option<&str> {
    let home = state_db.parent()?;
    let profiles = home.parent()?;
    if profiles.file_name().and_then(|name| name.to_str()) != Some("profiles") {
        return None;
    }
    home.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !name.contains('/') && !name.starts_with('.'))
}

pub fn validate(reference: &HermesConversation) -> Result<()> {
    if !valid_session_id(&reference.conversation) {
        bail!("Hermes recovery requires the native session id");
    }
    if reference.executable.as_os_str().is_empty() || !reference.state_db.is_absolute() {
        bail!("Hermes recovery reference is incomplete");
    }
    if !reference.state_db.is_file() {
        bail!("Hermes session database is unavailable");
    }
    let connection = rusqlite::Connection::open_with_flags(
        &reference.state_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Hermes session database cannot be opened")?;
    let found: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE id = ?1",
            [&reference.conversation],
            |row| row.get(0),
        )
        .context("Hermes session lookup failed")?;
    if found != 1 {
        bail!("Hermes session is not in the recorded database");
    }
    Ok(())
}

pub fn resume_argv(reference: &HermesConversation) -> Result<Vec<OsString>> {
    validate(reference)?;
    let mut argv = vec![reference.executable.clone().into_os_string()];
    if let Some(profile) = profile_name(&reference.state_db) {
        argv.push("-p".into());
        argv.push(profile.into());
    }
    argv.push("--resume".into());
    argv.push(reference.conversation.clone().into());
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_matches_the_native_generator() {
        assert!(valid_session_id("20261006_101500_ab12cd"));
        assert!(!valid_session_id("20261006_101500_AB12CD"));
        assert!(!valid_session_id("sess_abc123"));
        assert!(!valid_session_id("20261006_101500_ab12c"));
    }

    #[test]
    fn profile_resume_names_the_profile_directory() {
        let reference = HermesConversation {
            conversation: "20261006_101500_ab12cd".into(),
            executable: "/bin/hermes".into(),
            state_db: "/home/profiles/researcher/state.db".into(),
        };
        assert_eq!(profile_name(&reference.state_db), Some("researcher"));
        assert_eq!(profile_name(Path::new("/home/.hermes/state.db")), None);
    }
}
