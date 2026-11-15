//! Interactive `/tree` branch selector state, filter modes, connector layout,
//! and multi-token search over `OperatorSessionTree`.

use super::super::text_cursor::TextCursor;
use ovrcr_protocol::{OperatorSessionTree, OperatorTreeEntry, OperatorTreeEntryKind};
use std::collections::{HashMap, HashSet};

pub(super) const MAX_TREE_SEARCH_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TreeFilterMode {
    #[default]
    Default,
    NoTools,
    UserOnly,
    All,
}

impl TreeFilterMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::NoTools => "no-tools",
            Self::UserOnly => "user-only",
            Self::All => "all",
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::Default => Self::NoTools,
            Self::NoTools => Self::UserOnly,
            Self::UserOnly => Self::All,
            Self::All => Self::Default,
        }
    }

    pub(super) fn prev(self) -> Self {
        match self {
            Self::Default => Self::All,
            Self::NoTools => Self::Default,
            Self::UserOnly => Self::NoTools,
            Self::All => Self::UserOnly,
        }
    }

    fn keeps(self, entry: &OperatorTreeEntry, current_leaf_id: Option<&str>) -> bool {
        match self {
            Self::All => true,
            Self::Default => !matches!(
                entry.kind,
                OperatorTreeEntryKind::ModelChange
                    | OperatorTreeEntryKind::ThinkingLevelChange
                    | OperatorTreeEntryKind::Other
            ),
            Self::NoTools => {
                if matches!(
                    entry.kind,
                    OperatorTreeEntryKind::ModelChange
                        | OperatorTreeEntryKind::ThinkingLevelChange
                        | OperatorTreeEntryKind::Other
                        | OperatorTreeEntryKind::ToolResult
                ) {
                    return false;
                }
                if matches!(
                    entry.kind,
                    OperatorTreeEntryKind::Assistant { tool_only: true }
                ) {
                    return current_leaf_id == Some(entry.id.as_str());
                }
                true
            }
            Self::UserOnly => matches!(entry.kind, OperatorTreeEntryKind::User),
        }
    }
}

pub(super) fn entry_role_label(kind: OperatorTreeEntryKind) -> &'static str {
    match kind {
        OperatorTreeEntryKind::User => "user",
        OperatorTreeEntryKind::Assistant { tool_only: false } => "assistant",
        OperatorTreeEntryKind::Assistant { tool_only: true } => "assistant [tools]",
        OperatorTreeEntryKind::ToolResult => "tool",
        OperatorTreeEntryKind::Compaction => "[compaction]",
        OperatorTreeEntryKind::BranchSummary => "[branch summary]",
        OperatorTreeEntryKind::CustomMessage => "custom",
        OperatorTreeEntryKind::ModelChange => "[model]",
        OperatorTreeEntryKind::ThinkingLevelChange => "[thinking]",
        OperatorTreeEntryKind::Other => "[meta]",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TreeRow {
    pub entry_index: usize,
    pub entry_id: String,
    pub kind: OperatorTreeEntryKind,
    pub connector: String,
    pub active_branch: bool,
    pub current_leaf: bool,
    pub role_label: &'static str,
    pub preview: String,
}

pub(super) enum TreeModal {
    Loading,
    Loaded(TreePicker),
}

pub(super) struct TreePicker {
    pub tree: OperatorSessionTree,
    pub filter: TreeFilterMode,
    pub query: String,
    pub cursor: TextCursor,
    pub selected: usize,
    pub submitting: bool,
    pub error: Option<String>,
}

impl TreePicker {
    pub(super) fn new(tree: OperatorSessionTree) -> Self {
        let mut picker = Self {
            tree,
            filter: TreeFilterMode::Default,
            query: String::new(),
            cursor: TextCursor::default(),
            selected: 0,
            submitting: false,
            error: None,
        };
        let target = picker.tree.current_leaf_id.clone();
        picker.select_target_or_ancestor(target.as_deref());
        picker
    }

    pub(super) fn is_noop_selection(tree: &OperatorSessionTree, entry: &OperatorTreeEntry) -> bool {
        !matches!(
            entry.kind,
            OperatorTreeEntryKind::User | OperatorTreeEntryKind::CustomMessage
        ) && tree.current_leaf_id.as_deref() == Some(entry.id.as_str())
    }

    pub(super) fn visible_rows(&self) -> Vec<TreeRow> {
        let mode_rows = build_mode_rows(&self.tree, self.filter);
        let trimmed = self.query.trim();
        if trimmed.is_empty() {
            return mode_rows;
        }
        let tokens: Vec<String> = trimmed.split_whitespace().map(str::to_lowercase).collect();
        mode_rows
            .into_iter()
            .filter(|row| {
                let haystack =
                    format!("{} {} {}", row.role_label, row.preview, row.entry_id).to_lowercase();
                tokens.iter().all(|tok| haystack.contains(tok))
            })
            .collect()
    }

    pub(super) fn selected_entry(&self) -> Option<&OperatorTreeEntry> {
        let rows = self.visible_rows();
        let idx = self.selected.min(rows.len().saturating_sub(1));
        rows.get(idx).map(|row| &self.tree.entries[row.entry_index])
    }

    pub(super) fn cycle_filter(&mut self, forward: bool) {
        let anchor_id = self
            .selected_entry()
            .map(|entry| entry.id.clone())
            .or_else(|| self.tree.current_leaf_id.clone());
        self.filter = if forward {
            self.filter.next()
        } else {
            self.filter.prev()
        };
        self.select_target_or_ancestor(anchor_id.as_deref());
    }

    pub(super) fn on_query_changed(&mut self) {
        if self.query.trim().is_empty() {
            let leaf = self.tree.current_leaf_id.clone();
            self.select_target_or_ancestor(leaf.as_deref());
        } else {
            self.selected = 0;
        }
    }

    fn select_target_or_ancestor(&mut self, target_id: Option<&str>) {
        let rows = self.visible_rows();
        if rows.is_empty() {
            self.selected = 0;
            return;
        }
        if let Some(start_id) = target_id {
            let by_id: HashMap<&str, &OperatorTreeEntry> = self
                .tree
                .entries
                .iter()
                .map(|entry| (entry.id.as_str(), entry))
                .collect();
            let mut visited = HashSet::new();
            let mut cur = Some(start_id);
            while let Some(id) = cur {
                if !visited.insert(id) {
                    break;
                }
                if let Some(pos) = rows.iter().position(|row| row.entry_id == id) {
                    self.selected = pos;
                    return;
                }
                cur = by_id.get(id).and_then(|entry| entry.parent_id.as_deref());
            }
        }
        self.selected = 0;
    }
}

fn build_mode_rows(tree: &OperatorSessionTree, filter: TreeFilterMode) -> Vec<TreeRow> {
    if tree.entries.is_empty() {
        return Vec::new();
    }
    let leaf_id = tree.current_leaf_id.as_deref();
    let by_id: HashMap<&str, usize> = tree
        .entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| (entry.id.as_str(), idx))
        .collect();

    let mut active_path: HashSet<&str> = HashSet::new();
    let mut cur = leaf_id;
    while let Some(id) = cur {
        if !active_path.insert(id) {
            break;
        }
        cur = by_id
            .get(id)
            .and_then(|&idx| tree.entries[idx].parent_id.as_deref());
    }

    let kept: Vec<bool> = tree
        .entries
        .iter()
        .map(|entry| filter.keeps(entry, leaf_id))
        .collect();

    let mut roots: Vec<usize> = Vec::new();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    for (idx, entry) in tree.entries.iter().enumerate() {
        if !kept[idx] {
            continue;
        }
        let mut visited = HashSet::new();
        visited.insert(idx);
        let mut parent_id = entry.parent_id.as_deref();
        let mut visible_parent = None;
        while let Some(pid) = parent_id {
            let Some(&pidx) = by_id.get(pid) else {
                break;
            };
            if !visited.insert(pidx) {
                break;
            }
            if kept[pidx] {
                visible_parent = Some(pidx);
                break;
            }
            parent_id = tree.entries[pidx].parent_id.as_deref();
        }
        match visible_parent {
            Some(pidx) => children.entry(pidx).or_default().push(idx),
            None => roots.push(idx),
        }
    }

    let mut out = Vec::new();
    let root_branches = roots.len() > 1;
    let root_len = roots.len();
    for (pos, root_idx) in roots.into_iter().enumerate() {
        let (connector, next_gutter) = if root_branches {
            let last = pos + 1 == root_len;
            (
                if last {
                    "└─ ".to_owned()
                } else {
                    "├─ ".to_owned()
                },
                if last {
                    "   ".to_owned()
                } else {
                    "│  ".to_owned()
                },
            )
        } else {
            (String::new(), String::new())
        };
        emit_subtree(
            tree,
            &children,
            &active_path,
            leaf_id,
            root_idx,
            connector,
            next_gutter,
            &mut out,
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn emit_subtree(
    tree: &OperatorSessionTree,
    children: &HashMap<usize, Vec<usize>>,
    active_path: &HashSet<&str>,
    leaf_id: Option<&str>,
    idx: usize,
    connector: String,
    gutter: String,
    out: &mut Vec<TreeRow>,
) {
    let entry = &tree.entries[idx];
    out.push(TreeRow {
        entry_index: idx,
        entry_id: entry.id.clone(),
        kind: entry.kind,
        connector,
        active_branch: active_path.contains(entry.id.as_str()),
        current_leaf: leaf_id == Some(entry.id.as_str()),
        role_label: entry_role_label(entry.kind),
        preview: entry.preview.clone(),
    });
    let Some(kids) = children.get(&idx) else {
        return;
    };
    let branches = kids.len() > 1;
    let len = kids.len();
    for (pos, &child_idx) in kids.iter().enumerate() {
        let (child_connector, child_gutter) = if branches {
            let last = pos + 1 == len;
            (
                format!("{gutter}{}", if last { "└─ " } else { "├─ " }),
                format!("{gutter}{}", if last { "   " } else { "│  " }),
            )
        } else {
            (gutter.clone(), gutter.clone())
        };
        emit_subtree(
            tree,
            children,
            active_path,
            leaf_id,
            child_idx,
            child_connector,
            child_gutter,
            out,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        id: &str,
        parent: Option<&str>,
        kind: OperatorTreeEntryKind,
        preview: &str,
    ) -> OperatorTreeEntry {
        OperatorTreeEntry {
            id: id.into(),
            parent_id: parent.map(Into::into),
            kind,
            preview: preview.into(),
        }
    }

    fn sample_branched_tree() -> OperatorSessionTree {
        OperatorSessionTree {
            pi_session_id: Some("pi-1".into()),
            current_leaf_id: Some("a2b".into()),
            entries: vec![
                entry(
                    "m1",
                    None,
                    OperatorTreeEntryKind::ModelChange,
                    "openai/gpt-6.1",
                ),
                entry(
                    "u1",
                    Some("m1"),
                    OperatorTreeEntryKind::User,
                    "inspect server logs",
                ),
                entry(
                    "a1_tools",
                    Some("u1"),
                    OperatorTreeEntryKind::Assistant { tool_only: true },
                    "[tool: list_sessions]",
                ),
                entry(
                    "tr1",
                    Some("a1_tools"),
                    OperatorTreeEntryKind::ToolResult,
                    "tool result: list_sessions",
                ),
                entry(
                    "a1",
                    Some("tr1"),
                    OperatorTreeEntryKind::Assistant { tool_only: false },
                    "Found two active sessions.",
                ),
                // Branch A off a1
                entry(
                    "u2a",
                    Some("a1"),
                    OperatorTreeEntryKind::User,
                    "restart alpha worker",
                ),
                entry(
                    "a2a",
                    Some("u2a"),
                    OperatorTreeEntryKind::Assistant { tool_only: false },
                    "Alpha restarted.",
                ),
                // Branch B off a1 (active leaf = a2b)
                entry(
                    "u2b",
                    Some("a1"),
                    OperatorTreeEntryKind::User,
                    "check beta latency",
                ),
                entry(
                    "a2b",
                    Some("u2b"),
                    OperatorTreeEntryKind::Assistant { tool_only: false },
                    "Beta p99 is 12ms.",
                ),
            ],
        }
    }

    #[test]
    fn tree_picker_renders_connectors_active_markers_filters_and_multi_token_search() {
        let mut picker = TreePicker::new(sample_branched_tree());
        assert_eq!(picker.filter, TreeFilterMode::Default);

        // Initial selection lands on current_leaf_id (`a2b`).
        assert_eq!(picker.selected_entry().map(|e| e.id.as_str()), Some("a2b"));

        // Default mode hides `m1` (ModelChange) and re-parents `u1` as root; keeps tool rows.
        let default_rows = picker.visible_rows();
        let default_ids: Vec<&str> = default_rows.iter().map(|r| r.entry_id.as_str()).collect();
        assert_eq!(
            default_ids,
            vec!["u1", "a1_tools", "tr1", "a1", "u2a", "a2a", "u2b", "a2b"]
        );

        // Active path (`u1`, `a1_tools`, `tr1`, `a1`, `u2b`, `a2b`) carries `active_branch = true`;
        // sibling branch A (`u2a`, `a2a`) has `active_branch = false`.
        let by_id: HashMap<&str, &TreeRow> = default_rows
            .iter()
            .map(|r| (r.entry_id.as_str(), r))
            .collect();
        assert!(by_id["u1"].active_branch);
        assert!(by_id["a1"].active_branch);
        assert!(!by_id["u2a"].active_branch);
        assert!(!by_id["a2a"].active_branch);
        assert!(by_id["u2b"].active_branch);
        assert!(by_id["a2b"].active_branch);
        assert!(by_id["a2b"].current_leaf);

        // Connectors at divergence off `a1`:
        assert_eq!(by_id["u1"].connector, "");
        assert_eq!(by_id["a1"].connector, "");
        assert_eq!(by_id["u2a"].connector, "├─ ");
        assert_eq!(by_id["a2a"].connector, "│  ");
        assert_eq!(by_id["u2b"].connector, "└─ ");
        assert_eq!(by_id["a2b"].connector, "   ");

        // Cycle forward -> `no-tools` hides `a1_tools` and `tr1`, re-parenting `a1` under `u1`.
        picker.cycle_filter(true);
        assert_eq!(picker.filter, TreeFilterMode::NoTools);
        let no_tools_rows = picker.visible_rows();
        let no_tools_ids: Vec<&str> = no_tools_rows.iter().map(|r| r.entry_id.as_str()).collect();
        assert_eq!(no_tools_ids, vec!["u1", "a1", "u2a", "a2a", "u2b", "a2b"]);

        // Cycle forward -> `user-only` keeps only user prompts, re-parenting `u2a` & `u2b` under `u1`,
        // and snaps selection from hidden `a2b` to nearest visible ancestor `u2b`.
        picker.cycle_filter(true);
        assert_eq!(picker.filter, TreeFilterMode::UserOnly);
        let user_rows = picker.visible_rows();
        let user_ids: Vec<&str> = user_rows.iter().map(|r| r.entry_id.as_str()).collect();
        assert_eq!(user_ids, vec!["u1", "u2a", "u2b"]);
        assert_eq!(user_rows[1].connector, "├─ ");
        assert_eq!(user_rows[2].connector, "└─ ");
        assert_eq!(picker.selected_entry().map(|e| e.id.as_str()), Some("u2b"));

        // Cycle forward -> `all` includes `m1` too.
        picker.cycle_filter(true);
        assert_eq!(picker.filter, TreeFilterMode::All);
        assert_eq!(picker.visible_rows().len(), 9);

        // Case-insensitive multi-token search matches across role + preview regardless of word order.
        picker.query = "BETA assistant 12ms".into();
        picker.on_query_changed();
        let matched = picker.visible_rows();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].entry_id, "a2b");

        // Selection noop check: `a2b` (active assistant leaf) is noop, whereas `u2b` (user rewind) is NOT noop.
        assert!(TreePicker::is_noop_selection(
            &picker.tree,
            &picker.tree.entries[8]
        ));
        assert!(!TreePicker::is_noop_selection(
            &picker.tree,
            &picker.tree.entries[7]
        ));
    }
}
