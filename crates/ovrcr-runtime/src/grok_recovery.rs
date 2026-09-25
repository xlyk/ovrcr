//! Grok history retention for the title path. Grok has no resume adapter: this module
//! names the one file Grok's documented session store keeps for a managed launch and
//! checks that the file belongs to the recorded conversation before anyone reads it.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::GrokConversation;
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

/// `$GROK_HOME`, else `~/.grok`, as Grok's session documentation defines it.
pub fn config_dir() -> Result<PathBuf> {
    let path = std::env::var_os("GROK_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".grok")))
        .context("Grok home directory is unavailable")?;
    if !path.is_absolute() {
        bail!("Grok home directory must be absolute");
    }
    Ok(path)
}

/// `<home>/sessions/<encoded cwd>/<conversation>/updates.jsonl`: the ACP update stream
/// Grok documents as the authoritative conversation log. The group name is the working
/// directory percent-encoded (every byte outside `A-Za-z0-9-_.~`), which is what the
/// installed Grok writes. A directory whose encoded name exceeds 255 bytes uses a slug
/// Grok does not document; that path simply never exists, and no title is produced.
pub fn history_path(config_dir: &Path, cwd: &Path, conversation: &str) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let mut group = String::new();
    for byte in cwd.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(byte) {
            group.push(*byte as char);
        } else {
            group.push_str(&format!("%{byte:02X}"));
        }
    }
    config_dir
        .join("sessions")
        .join(group)
        .join(conversation)
        .join("updates.jsonl")
}

pub fn validate(reference: &GrokConversation) -> Result<()> {
    ovrcr_protocol::validate_agent_id(&reference.conversation)?;
    if !reference.history.is_absolute() {
        bail!("Grok history reference must be absolute");
    }
    Ok(())
}

/// The identity check a reader applies before any text is read: the first line of
/// `updates.jsonl` is one ACP session update whose `params.sessionId` names the recorded
/// conversation. Every line carries that field, so the first is enough and nothing after
/// it is read here.
pub fn validate_history(reference: &GrokConversation) -> Result<()> {
    let file =
        std::fs::File::open(&reference.history).context("Grok history file is unavailable")?;
    if !file.metadata()?.is_file() {
        bail!("Grok history is not a regular file");
    }
    let mut header = String::new();
    BufReader::new(file.take(1024 * 1024)).read_line(&mut header)?;
    let header: serde_json::Value =
        serde_json::from_str(&header).context("Grok history header is invalid")?;
    if header["params"]["sessionId"] != reference.conversation {
        bail!("Grok history identity does not match the recorded conversation");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_path_encodes_the_working_directory_the_way_grok_does() {
        assert_eq!(
            history_path(
                Path::new("/home/u/.grok"),
                Path::new("/Users/x y/Code/ovrcr_1.0~"),
                "01a0c579-40ff-7981-a9cb-0fe920aef561",
            ),
            PathBuf::from(
                "/home/u/.grok/sessions/%2FUsers%2Fx%20y%2FCode%2Fovrcr_1.0~/01a0c579-40ff-7981-a9cb-0fe920aef561/updates.jsonl"
            )
        );
    }

    #[test]
    fn history_identity_is_the_first_update_line_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("updates.jsonl");
        let id = "01a0c579-40ff-7981-a9cb-0fe920aef561";
        let reference = GrokConversation {
            conversation: id.into(),
            history: path.clone(),
        };
        assert!(validate_history(&reference).is_err(), "missing file");
        std::fs::write(&path, format!("{{\"method\":\"session/update\",\"params\":{{\"sessionId\":\"{id}\",\"update\":{{\"sessionUpdate\":\"user_message_chunk\"}}}}}}\nnot json\n")).unwrap();
        validate_history(&reference).unwrap();
        std::fs::write(&path, "{\"method\":\"session/update\",\"params\":{\"sessionId\":\"01a0c579-40ff-7981-a9cb-000000000000\"}}\n").unwrap();
        assert!(validate_history(&reference).is_err(), "other conversation");
        std::fs::write(&path, "{\"params\":{}}\n").unwrap();
        assert!(validate_history(&reference).is_err(), "no identity");
        std::fs::write(&path, "garbage\n").unwrap();
        assert!(validate_history(&reference).is_err(), "not json");
        assert!(
            validate(&GrokConversation {
                conversation: id.into(),
                history: "relative/updates.jsonl".into(),
            })
            .is_err()
        );
    }
}
