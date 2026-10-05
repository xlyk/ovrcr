//! The Dashboard's settings: what the Server published, and the requests that
//! change it. Reading and writing belong to the Server, which publishes its
//! reading as a `SettingsReport`; the Dashboard has no parser and touches no
//! file.
use ovrcr_protocol::Request;
pub use ovrcr_protocol::{AgentOverride, AutomaticLocalTerminals, LaunchChoice, Settings};

pub(super) const INVALID_SOUND_CHOICE_NOTICE: &str =
    "Ready sound choice invalid; banners are silent. Choose a sound in Settings";

pub(super) fn invalid_sound_choice_notice(settings: &Settings) -> Option<&'static str> {
    (cfg!(target_os = "macos")
        && settings.desktop_notifications
        && settings.ready_sound
        && settings.ready_sound_choice.is_none())
    .then_some(INVALID_SOUND_CHOICE_NOTICE)
}

/// Keep the actionable sound finding visible when both macOS alert flags are on.
/// The full reading and every finding remain available in Settings.
pub(super) fn findings_notice(settings: &Settings, count: usize) -> String {
    if let Some(notice) = invalid_sound_choice_notice(settings) {
        return notice.into();
    }
    match count {
        0 => "No settings findings".into(),
        1 => "1 settings finding; see Settings".into(),
        count => format!("{count} settings findings; see Settings"),
    }
}

/// The `SetSetting` request that remembers `choice` for `project`. The
/// Server merges it into the project's entry and keeps other projects.
pub(super) fn launch_choice_request(project: &str, choice: &LaunchChoice) -> Request {
    let mut entry = toml_edit::InlineTable::new();
    match choice {
        LaunchChoice::Terminal => {
            entry.insert("kind", "Terminal".into());
        }
        LaunchChoice::Agent(preset) => {
            entry.insert("kind", "Agent".into());
            entry.insert("preset", preset.as_str().into());
        }
    }
    Request::SetSetting {
        path: format!("launch_choices.{}", toml_edit::Key::new(project)),
        value: Some(entry.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ovrcr-runtime`'s writer tests apply exactly these values.
    #[test]
    fn launch_choice_requests_quote_the_project_and_name_every_field() {
        assert_eq!(
            launch_choice_request("project.with.dots", &LaunchChoice::Agent("new".into())),
            Request::SetSetting {
                path: "launch_choices.\"project.with.dots\"".into(),
                value: Some("{ kind = \"Agent\", preset = \"new\" }".into()),
            }
        );
        assert_eq!(
            launch_choice_request("demo", &LaunchChoice::Terminal),
            Request::SetSetting {
                path: "launch_choices.demo".into(),
                value: Some("{ kind = \"Terminal\" }".into()),
            }
        );
    }
}
