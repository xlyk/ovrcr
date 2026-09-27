use super::keymap::keymap;
use super::{Dashboard, InputMode};

pub(super) fn footer(dashboard: &Dashboard, width: u16) -> String {
    use unicode_width::UnicodeWidthStr;
    let history = dashboard
        .history
        .as_ref()
        .filter(|_| dashboard.mode == InputMode::History);
    let mode = match dashboard.mode {
        InputMode::Browse => "BROWSE",
        InputMode::Copy => "COPY",
        InputMode::History
            if history.is_some_and(|v| v.copy_job.is_some() || v.copy_completion.is_some()) =>
        {
            "HISTORY COPY"
        }
        InputMode::History if history.is_some_and(|v| v.cursor_target.is_some()) => {
            "HISTORY Waiting"
        }
        InputMode::History if history.is_some_and(|v| v.anchor.is_some()) => "HISTORY SELECT",
        InputMode::History => "HISTORY",
        InputMode::Terminal => return "Terminal mode  Ctrl-g browse".into(),
    };
    let mut text: String = mode.chars().take(usize::from(width)).collect();
    let groups = keymap(dashboard);
    let hints: Vec<_> = groups.iter().flat_map(|g| &g.keys).collect();
    // Browse shows only the keys that open everything else; the menu and the
    // palette carry the per-action hints.
    let priority: &[&str] = if dashboard.mode == InputMode::Browse {
        &["Space", ":", "?"]
    } else {
        &[
            "?", "Esc", "v", "y", "h/Left", "j/Down", "k/Up", "l/Right", "PageUp", "PageDown",
            "Space",
        ]
    };
    for key in priority {
        let Some(hint) = hints.iter().find(|h| h.key == *key && h.enabled()) else {
            continue;
        };
        let part = format!(
            "  {} {}",
            hint.key.split('/').next().unwrap_or(hint.key),
            hint.name
        );
        if text.width() + part.width() <= usize::from(width) {
            text.push_str(&part);
        }
    }
    if let Some(view) = history {
        let detail = if view.copy_job.is_some() || view.copy_completion.is_some() {
            "Copying selection".into()
        } else if view.cursor_target.is_some() {
            "Waiting for history cell".into()
        } else if view.anchor.is_some() {
            view.cursor.map_or_else(
                || "Nothing to select on this row".into(),
                |cursor| {
                    format!(
                        "row {}:{}",
                        cursor.point.row + 1,
                        u32::from(cursor.point.col) + 1
                    )
                },
            )
        } else {
            String::new()
        };
        if !detail.is_empty() && text.width() + detail.width() + 2 <= usize::from(width) {
            text.push_str("  ");
            text.push_str(&detail);
        }
    }
    text
}
