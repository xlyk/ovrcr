use super::render::{CRUST, MAUVE, TEXT};
use ovrcr_protocol::HierarchySnapshot;
use ratatui::{style::Style, text::Line};
use std::path::{Path, PathBuf};

const PATH_LIST_LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickItem {
    pub label: String,
    pub value: String,
}

pub(crate) fn workspace_pick_value(project: &str, id: &str) -> String {
    format!("{project}/{id}")
}

pub(crate) fn split_workspace_pick(value: &str) -> (String, String) {
    value
        .split_once('/')
        .map(|(project, id)| (project.to_string(), id.to_string()))
        .unwrap_or_else(|| (value.to_string(), String::new()))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickList {
    pub items: Vec<PickItem>,
    pub query: String,
    pub selected: usize,
}

impl PickList {
    pub fn new(items: Vec<PickItem>) -> Self {
        Self {
            items,
            query: String::new(),
            selected: 0,
        }
    }

    pub fn projects(hierarchy: &HierarchySnapshot, preferred: &str) -> Self {
        let mut list = Self::new(
            hierarchy
                .projects
                .iter()
                .map(|project| PickItem {
                    label: project.name.clone(),
                    value: project.name.clone(),
                })
                .collect(),
        );
        list.select_value(preferred);
        list
    }

    pub fn workspaces(
        hierarchy: &HierarchySnapshot,
        project: &str,
        workspace_id: &str,
        include_root: bool,
    ) -> Self {
        let mut list = Self::new(
            hierarchy
                .projects
                .iter()
                .flat_map(|project| {
                    project.workspaces.iter().filter_map(move |workspace| {
                        if !include_root && workspace.root {
                            return None;
                        }
                        let mut label = format!("{} / {}", project.name, workspace.name);
                        if project
                            .workspaces
                            .iter()
                            .filter(|candidate| candidate.name == workspace.name)
                            .count()
                            > 1
                        {
                            label = format!("{label} ({})", workspace.path.display());
                        }
                        Some(PickItem {
                            value: workspace_pick_value(&project.name, &workspace.id),
                            label,
                        })
                    })
                })
                .collect(),
        );
        if workspace_id.is_empty() {
            if let Some(id) = hierarchy
                .projects
                .iter()
                .find(|candidate| candidate.name == project)
                .and_then(|candidate| {
                    candidate
                        .workspaces
                        .iter()
                        .find(|workspace| include_root || !workspace.root)
                })
                .map(|workspace| workspace.id.clone())
            {
                list.select_value(&workspace_pick_value(project, &id));
            }
        } else {
            list.select_value(&workspace_pick_value(project, workspace_id));
        }
        list
    }

    /// Visible options and the selected row, shared by palette and task forms.
    pub fn lines(&self, limit: usize) -> (Vec<Line<'_>>, usize) {
        let filtered = self.filtered();
        if filtered.is_empty() {
            return (
                vec![Line::styled("    No matches", Style::default().fg(TEXT))],
                0,
            );
        }
        let count = limit.min(filtered.len());
        let start = self
            .selected
            .min(filtered.len() - 1)
            .saturating_sub(count.saturating_sub(1));
        let lines = filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(count)
            .map(|(offset, item)| {
                let chosen = offset == self.selected;
                Line::styled(
                    format!("  {} {}", if chosen { "›" } else { " " }, item.label),
                    if chosen {
                        Style::default().bg(MAUVE).fg(CRUST)
                    } else {
                        Style::default().fg(TEXT)
                    },
                )
            })
            .collect();
        (lines, self.selected.saturating_sub(start))
    }

    pub fn filtered(&self) -> Vec<&PickItem> {
        self.items
            .iter()
            .filter(|item| subsequence(&self.query, &item.label))
            .collect()
    }

    pub fn move_selection(&mut self, delta: isize) {
        let count = self.filtered().len();
        if count == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, count as isize - 1) as usize;
    }

    pub fn accepted(&self) -> Option<&PickItem> {
        self.filtered().get(self.selected).copied()
    }

    pub fn select_value(&mut self, value: &str) {
        if let Some(index) = self.filtered().iter().position(|item| item.value == value) {
            self.selected = index;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathEntry {
    pub name: String,
    pub label: String,
    pub git: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathListing {
    pub entries: Vec<PathEntry>,
    pub omitted: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathPicker {
    pub selected: usize,
    cached: Option<(String, PathListing)>,
}

impl PathPicker {
    pub fn new() -> Self {
        Self {
            selected: 0,
            cached: None,
        }
    }

    pub fn move_selection(&mut self, listing: &PathListing, delta: isize) {
        let count = listing.entries.len();
        if count == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, count as isize - 1) as usize;
    }

    /// Recomputes the directory listing only when `input` differs from the
    /// last computed value; `draw_palette` cannot recompute it itself since
    /// it only holds `&self`, so callers must refresh this after every edit.
    pub fn listing(&mut self, input: &str, roots: &[PathBuf]) -> &PathListing {
        if self.cached.as_ref().is_none_or(|(key, _)| key != input) {
            self.cached = Some((input.to_owned(), list_path_entries(input, roots)));
        }
        &self.cached.as_ref().unwrap().1
    }

    /// The most recently computed listing, if any; `None` before the first
    /// call to `listing`.
    pub fn cached(&self) -> Option<&PathListing> {
        self.cached.as_ref().map(|(_, listing)| listing)
    }
}

pub fn list_path_entries(input: &str, roots: &[PathBuf]) -> PathListing {
    let (prefix, segment) = split_input(input);
    let show_hidden = segment.starts_with('.');
    let mut entries = if prefix.is_empty() {
        // Roots stay in the order the settings document saved them. Git-first
        // sorting is for the children of a directory, below.
        roots
            .iter()
            .filter_map(|root| {
                if !root.is_dir() {
                    return None;
                }
                let name = display_path(root);
                if !subsequence(segment, &name) && !subsequence(segment, &file_name(root)) {
                    return None;
                }
                Some(path_entry(name, root.join(".git").exists()))
            })
            .collect::<Vec<_>>()
    } else {
        let dir = expand_dir(prefix);
        let mut entries = read_dirs(&dir, segment, show_hidden);
        entries.sort_by(|a, b| b.git.cmp(&a.git).then_with(|| a.name.cmp(&b.name)));
        entries
    };
    let omitted = entries.len().saturating_sub(PATH_LIST_LIMIT);
    entries.truncate(PATH_LIST_LIMIT);
    PathListing { entries, omitted }
}

pub fn complete_path(input: &str, roots: &[PathBuf], selected: usize) -> Option<String> {
    let listing = list_path_entries(input, roots);
    let entry = listing.entries.get(selected)?;
    let (prefix, _) = split_input(input);
    if prefix.is_empty() {
        Some(format!("{}/", entry.name))
    } else {
        Some(format!("{prefix}{}/", entry.name))
    }
}

fn path_entry(name: String, git: bool) -> PathEntry {
    PathEntry {
        label: if git {
            format!("{name}  git")
        } else {
            name.clone()
        },
        name,
        git,
    }
}

fn read_dirs(dir: &Path, segment: &str, show_hidden: bool) -> Vec<PathEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(|entry| {
        let entry = entry.ok()?;
        let path = entry.path();
        if !path.is_dir() {
            return None;
        }
        let name = file_name(&path).to_string();
        if name == "." || name == ".." {
            return None;
        }
        if name.starts_with('.') && !show_hidden {
            return None;
        }
        if !subsequence(segment, &name) {
            return None;
        }
        // The git flag decides sort order before truncation, so every
        // matching entry needs it, not just the first PATH_LIST_LIMIT
        // encountered in read_dir's (unsorted) order.
        Some(path_entry(name, path.join(".git").exists()))
    })
    .collect()
}

fn split_input(input: &str) -> (&str, &str) {
    match input.rfind('/') {
        Some(index) => (&input[..=index], &input[index + 1..]),
        None => ("", input),
    }
}

fn expand_dir(prefix: &str) -> PathBuf {
    expand_path(prefix)
}

pub(crate) fn expand_path(input: &str) -> PathBuf {
    let trimmed = input.trim().trim_end_matches('/');
    match trimmed {
        "" if input.trim().starts_with('/') => PathBuf::from("/"),
        "" | "~" => home_dir(),
        _ => match trimmed.strip_prefix("~/") {
            Some(rest) => home_dir().join(rest),
            None => PathBuf::from(trimmed),
        },
    }
}

fn display_path(path: &Path) -> String {
    let home = home_dir();
    if path == home {
        "~".into()
    } else if let Ok(rest) = path.strip_prefix(&home) {
        format!("~/{}", rest.display())
    } else {
        path.display().to_string()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn subsequence(query: &str, label: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut chars = label.chars().map(|ch| ch.to_ascii_lowercase());
    query
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .all(|needle| chars.any(|hay| hay == needle))
}

#[cfg(test)]
mod tests {
    static HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    use super::*;

    fn with_home<T>(home: &Path, body: impl FnOnce() -> T) -> T {
        let _guard = HOME_ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", home) };
        let result = body();
        if let Some(previous) = previous {
            unsafe { std::env::set_var("HOME", previous) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        result
    }

    #[test]
    fn expand_path_resolves_tilde_and_keeps_absolute() {
        let dir = tempfile::tempdir().unwrap();
        with_home(dir.path(), || {
            assert_eq!(expand_path("~/Code/repo/"), dir.path().join("Code/repo"));
            assert_eq!(expand_path("~"), dir.path().to_path_buf());
            assert_eq!(expand_path("/tmp/x"), PathBuf::from("/tmp/x"));
        });
    }

    #[test]
    fn tilde_prefix_lists_code_and_tab_completes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Code")).unwrap();
        with_home(dir.path(), || {
            let roots = [dir.path().join("Code")];
            let listing = list_path_entries("~/Co", &roots);
            assert_eq!(
                listing
                    .entries
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .collect::<Vec<_>>(),
                ["Code"]
            );
            assert_eq!(complete_path("~/Co", &roots, 0).as_deref(), Some("~/Code/"));
        });
    }

    #[test]
    fn slash_prefix_lists_filesystem_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Code")).unwrap();
        with_home(dir.path(), || {
            let roots = [dir.path().join("Code")];
            let listing = list_path_entries("/", &roots);
            assert!(!listing.entries.is_empty());
            assert!(
                listing.entries.iter().all(|entry| entry.name != "Code"),
                "{:?}",
                listing.entries
            );
            assert_eq!(expand_path("/"), PathBuf::from("/"));
        });
    }

    #[test]
    fn listing_is_cached_until_input_changes() {
        let dir = tempfile::tempdir().unwrap();
        let code = dir.path().join("Code");
        std::fs::create_dir(&code).unwrap();
        std::fs::create_dir(code.join("repo")).unwrap();
        with_home(dir.path(), || {
            let roots = [code.clone()];
            let mut picker = PathPicker::new();
            let first = picker.listing("~/Code/", &roots).clone();
            assert_eq!(
                first
                    .entries
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .collect::<Vec<_>>(),
                ["repo"]
            );
            std::fs::remove_dir(code.join("repo")).unwrap();
            let second = picker.listing("~/Code/", &roots).clone();
            assert_eq!(first, second);
            let third = picker.listing("~/Code/r", &roots).clone();
            assert!(third.entries.is_empty(), "{:?}", third.entries);
        });
    }

    /// An empty field is the picker roots themselves. Git-first and name
    /// sorting apply to a directory's children, not to this list.
    #[test]
    fn empty_field_lists_roots_in_the_given_order() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("z-plain");
        let repo = dir.path().join("a-git");
        std::fs::create_dir(&plain).unwrap();
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(repo.join(".git")).unwrap();
        with_home(dir.path(), || {
            let listing = list_path_entries("", &[plain.clone(), repo.clone()]);
            assert_eq!(
                listing
                    .entries
                    .iter()
                    .map(|entry| (entry.name.as_str(), entry.git))
                    .collect::<Vec<_>>(),
                [("~/z-plain", false), ("~/a-git", true)]
            );
        });
    }

    #[test]
    fn git_dirs_sort_first_and_hidden_needs_dot_segment() {
        let dir = tempfile::tempdir().unwrap();
        let code = dir.path().join("Code");
        std::fs::create_dir(&code).unwrap();
        std::fs::create_dir(code.join("plain")).unwrap();
        std::fs::create_dir(code.join("repo")).unwrap();
        std::fs::create_dir(code.join("repo").join(".git")).unwrap();
        std::fs::create_dir(code.join(".hidden")).unwrap();
        with_home(dir.path(), || {
            let roots = [code.clone()];
            let listing = list_path_entries("~/Code/", &roots);
            assert_eq!(
                listing
                    .entries
                    .iter()
                    .map(|entry| (entry.name.as_str(), entry.git, entry.label.as_str()))
                    .collect::<Vec<_>>(),
                [("repo", true, "repo  git"), ("plain", false, "plain")]
            );
            let hidden = list_path_entries("~/Code/.", &roots);
            assert!(hidden.entries.iter().any(|entry| entry.name == ".hidden"));
        });
    }
}
