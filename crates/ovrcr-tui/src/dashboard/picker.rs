use std::path::{Path, PathBuf};

const PATH_LIST_LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickItem {
    pub label: String,
    pub value: String,
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

    pub fn on_insert(&mut self, text: &str) {
        self.query.push_str(text);
        self.selected = 0;
    }

    pub fn on_backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
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
}

impl PathPicker {
    pub fn new() -> Self {
        Self { selected: 0 }
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
}

pub fn list_path_entries(input: &str, roots: &[PathBuf]) -> PathListing {
    let (prefix, segment) = split_input(input);
    let show_hidden = segment.starts_with('.');
    let mut entries = if prefix.is_empty() {
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
        read_dirs(&dir, segment, show_hidden)
    };
    entries.sort_by(|a, b| b.git.cmp(&a.git).then_with(|| a.name.cmp(&b.name)));
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
    use super::super::settings::HOME_ENV_LOCK;
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
