//! The Dashboard's settings: what the Server published, and the requests that
//! change it. Reading and writing belong to the Server, which publishes its
//! reading as a `SettingsReport`; the Dashboard has no parser and touches no
//! file.
use ovrcr_protocol::Request;
pub use ovrcr_protocol::{AgentOverride, AutomaticLocalTerminals, LaunchChoice, Settings};

/// The footer line for a reading with `count` findings.
pub(super) fn findings_notice(count: usize) -> String {
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
