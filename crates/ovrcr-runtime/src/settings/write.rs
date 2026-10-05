//! The settings writer (ADR 0007): one setting path and TOML value text in,
//! one in-place edit of the settings document out.
//!
//! The Server applies every Dashboard and CLI edit through [`set`]; the CLI
//! calls it directly only when no Server is running. An edit keeps comments,
//! unrelated keys and formatting, and is checked by the loader itself: a value
//! the loader would turn into a new finding for the edited setting is
//! rejected and the document is left unchanged.

use super::{Segment, parse};
use anyhow::{Context, bail};
use std::path::Path;
use toml_edit::{InlineTable, Item, Table, TableLike, Value};

/// What a setting path names, for the value checks before the edit.
#[derive(Clone, Copy)]
enum Kind {
    /// String-valued. A value that is not TOML is taken verbatim, so
    /// `ovrcr settings set branch_prefix kh/` needs no quotes.
    Text(&'static str),
    /// Any other type; the loader checks it after the edit.
    Typed(&'static str),
    /// One element or entry of a collection, given as an inline table. Given
    /// fields are set, declared fields left out are removed, and fields the
    /// document has that are not declared are kept.
    Entry(&'static [&'static str], &'static str),
}

impl Kind {
    fn expected(self) -> &'static str {
        match self {
            Self::Text(name) | Self::Typed(name) | Self::Entry(_, name) => name,
        }
    }
}

/// Set (`Some` TOML value text) or remove (`None`) the setting `path` names in
/// the settings document at `document`. Index `len` of a list appends.
pub fn set(document: &Path, path: &str, value: Option<&str>) -> anyhow::Result<()> {
    let (segments, kind) = declared(path).with_context(|| format!("no setting named {path}"))?;
    let fail = || format!("could not save {path}");
    let value = value
        .map(|text| typed_value(kind, text))
        .transpose()
        .with_context(fail)?;
    // Findings the edited setting is judged by: the loader's key for it.
    let root = match segments.as_slice() {
        [Segment::Key(quota), ..] if quota == "quota" => path.to_owned(),
        [Segment::Key(name), ..] => name.clone(),
        _ => unreachable!("declared paths start with a key"),
    };
    save(document, |edited| {
        let before = parse(document, &edited.to_string()).findings;
        let unplaced = apply(edited.as_item_mut(), &segments, kind, value)?;
        if !unplaced.is_empty() {
            let trailing = edited.trailing().as_str().unwrap_or("").to_owned();
            edited.set_trailing(format!("{trailing}{unplaced}"));
        }
        let after = parse(document, &edited.to_string()).findings;
        for finding in after {
            let Some(key) = &finding.key else { continue };
            let ours = key == &root
                || key.starts_with(&format!("{root}."))
                || key.starts_with(&format!("{root}["));
            let new = !before
                .iter()
                .any(|old| old.key == finding.key && old.message == finding.message);
            let invalid_sound_choice = root == ovrcr_protocol::ReadySoundChoice::KEY
                && key == ovrcr_protocol::ReadySoundChoice::KEY;
            if ours && (new || invalid_sound_choice) && finding.message.contains(kind.expected()) {
                bail!("{}", finding.message);
            } else if ours && (new || invalid_sound_choice) {
                bail!("{}; expected {}", finding.message, kind.expected());
            }
        }
        Ok(())
    })
    .with_context(fail)
}

/// The segments and kind of a declared setting path, or `None`.
fn declared(path: &str) -> Option<(Vec<Segment>, Kind)> {
    use Kind::{Entry, Text, Typed};
    use Segment::{Index as I, Key as K};
    let segments = parse_path(path)?;
    let names: Vec<Option<&str>> = segments
        .iter()
        .map(|segment| match segment {
            K(name) => Some(name.as_str()),
            I(_) => None,
        })
        .collect();
    let kind = match names.as_slice() {
        [Some("desktop_notifications" | "ready_sound" | "iterm_focus")] => Typed("a boolean"),
        [Some("ready_sound_choice")] => Text("\"default\", \"tap\", \"chime\" or \"rise\""),
        [Some("automatic_local_terminals")] => Text("\"on\", \"off\" or \"default_branch_only\""),
        [Some("title_model")] => Text("a \"provider/model\" string"),
        [Some("branch_prefix")] => Text("a string"),
        [Some("picker_roots")] => Typed("an array of paths"),
        [Some("agents")] => Typed("an array of { name, argv } tables"),
        [Some("launch_choices")] => Typed("a table of { kind, preset } tables"),
        [Some("quota"), Some("enabled")] => Typed("a boolean"),
        [Some("quota"), Some("claude"), Some("probe")] => Typed("a boolean"),
        [
            Some("quota"),
            Some("codex" | "grok"),
            Some("command" | "home"),
        ] => Text("a path"),
        [Some("picker_roots"), None] => Text("a path"),
        [Some("agents"), None] => Entry(&["name", "argv"], "an inline table { name, argv }"),
        [Some("agents"), None, Some("name")] => Text("a string"),
        [Some("agents"), None, Some("argv")] => Typed("an array of strings"),
        [Some("launch_choices"), Some(_)] => {
            Entry(&["kind", "preset"], "an inline table { kind, preset }")
        }
        [Some("launch_choices"), Some(_), Some("kind")] => Text("\"Terminal\" or \"Agent\""),
        [Some("launch_choices"), Some(_), Some("preset")] => Text("a string"),
        _ => return None,
    };
    Some((segments, kind))
}

/// `picker_roots[2]`, `agents[1].argv`, or TOML dotted keys such as
/// `launch_choices."project.with.dots".kind`.
fn parse_path(path: &str) -> Option<Vec<Segment>> {
    let keys = |text: &str| {
        toml_edit::Key::parse(text).ok().map(|keys| {
            keys.into_iter()
                .map(|key| Segment::Key(key.get().to_owned()))
                .collect::<Vec<_>>()
        })
    };
    for list in ["picker_roots", "agents"] {
        if let Some(rest) = path.strip_prefix(list).and_then(|r| r.strip_prefix('[')) {
            let (index, rest) = rest.split_once(']')?;
            let mut segments = vec![
                Segment::Key(list.into()),
                Segment::Index(index.parse().ok()?),
            ];
            if !rest.is_empty() {
                segments.extend(keys(rest.strip_prefix('.')?)?);
            }
            return Some(segments);
        }
    }
    keys(path)
}

fn typed_value(kind: Kind, text: &str) -> anyhow::Result<Value> {
    let mut value = match (text.parse::<Value>(), kind) {
        (Ok(value), _) => value,
        (Err(_), Kind::Text(_)) => Value::from(text),
        (Err(_), kind) => bail!("{text:?} is not a TOML value; expected {}", kind.expected()),
    };
    value.decor_mut().clear();
    match (kind, &value) {
        (Kind::Text(expected), value) if !value.is_str() => {
            bail!("got {value}; expected {expected}")
        }
        (Kind::Entry(fields, expected), value) => {
            let Some(table) = value.as_inline_table() else {
                bail!("got {value}; expected {expected}")
            };
            if let Some((field, _)) = table.iter().find(|(field, _)| !fields.contains(field)) {
                bail!("unknown field {field}; expected {expected}");
            }
        }
        _ => {}
    }
    Ok(value)
}

/// Returns comment lines a removal could not hand to a following entry.
fn apply(
    root: &mut Item,
    segments: &[Segment],
    kind: Kind,
    value: Option<Value>,
) -> anyhow::Result<String> {
    match segments {
        [Segment::Key(list), Segment::Index(index), rest @ ..] => {
            let table = root.as_table_like_mut().expect("document root is a table");
            let item = table.entry(list).or_insert(Item::None);
            match rest {
                [] => element(list, item, *index, kind, value).map(|()| String::new()),
                [Segment::Key(field)] => {
                    let element = element_table(list, item, *index)?;
                    Ok(set_field(element, field, kind, value, false))
                }
                _ => unreachable!("declared paths end after one field"),
            }
        }
        _ => {
            let (last, tables) = segments.split_last().expect("declared paths are not empty");
            let Segment::Key(last) = last else {
                unreachable!("only list paths end in an index")
            };
            if value.is_none() {
                return Ok(remove_key(root, tables, last));
            }
            let mut item = root;
            for segment in tables {
                let Segment::Key(name) = segment else {
                    unreachable!("only list paths hold an index")
                };
                let inline = item.is_inline_table();
                let Some(table) = item.as_table_like_mut() else {
                    bail!("{name}: its parent in the document is not a table");
                };
                item = table.entry(name).or_insert_with(|| implicit_table(inline));
            }
            let inline = item.is_inline_table();
            let Some(table) = item.as_table_like_mut() else {
                bail!("{last}: its parent in the document is not a table");
            };
            Ok(set_field(table, last, kind, value, inline))
        }
    }
}

/// Remove `last` under `tables`, then each table the removal left empty, so
/// a reset of a key the document never had returns it to its old text.
fn remove_key(root: &mut Item, tables: &[Segment], last: &str) -> String {
    let names: Vec<&str> = tables
        .iter()
        .map(|segment| match segment {
            Segment::Key(name) => name.as_str(),
            Segment::Index(_) => unreachable!("only list paths hold an index"),
        })
        .chain([last])
        .collect();
    let mut comment = String::new();
    for depth in (1..=names.len()).rev() {
        let (key, parents) = names[..depth].split_last().unwrap();
        let mut item = &mut *root;
        for name in parents {
            match item.get_mut(name) {
                Some(next) => item = next,
                None => return comment,
            }
        }
        let Some(table) = item.as_table_like_mut() else {
            return comment;
        };
        let leaf = depth == names.len();
        if !leaf
            && !table
                .get(key)
                .and_then(Item::as_table_like)
                .is_some_and(|t| t.is_empty())
        {
            return comment;
        }
        comment = remove_keeping_comments(table, key, comment);
    }
    comment
}

/// Remove `key` and hand the comment lines above it (after `carried`) to the
/// entry that follows, so a reset never deletes a comment about something
/// else. Returns the comment when nothing follows.
fn remove_keeping_comments(table: &mut dyn TableLike, key: &str, carried: String) -> String {
    let position = table.iter().position(|(name, _)| name == key);
    let prefix = match table.get(key) {
        Some(Item::Table(removed)) => removed.decor().prefix(),
        _ => table.key(key).and_then(|key| key.leaf_decor().prefix()),
    };
    let own = prefix
        .and_then(|prefix| prefix.as_str())
        .filter(|prefix| prefix.contains('#'))
        .unwrap_or("");
    let comment = format!("{carried}{own}");
    table.remove(key);
    if comment.is_empty() {
        return comment;
    }
    let Some((mut next, item)) = position.and_then(|position| table.iter_mut().nth(position))
    else {
        return comment;
    };
    let decor = match item {
        Item::Table(table) => table.decor_mut(),
        _ => next.leaf_decor_mut(),
    };
    let rest = decor
        .prefix()
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_owned();
    decor.set_prefix(format!("{comment}{rest}"));
    String::new()
}

fn implicit_table(inline: bool) -> Item {
    if inline {
        Item::Value(InlineTable::new().into())
    } else {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    }
}

/// Set, merge or remove `key` in `table`; returns a removal's unplaced comment.
fn set_field(
    table: &mut dyn TableLike,
    key: &str,
    kind: Kind,
    value: Option<Value>,
    inline: bool,
) -> String {
    match (value, kind) {
        (None, _) => return remove_keeping_comments(table, key, String::new()),
        (Some(value), Kind::Entry(fields, _)) => {
            let entry = table.entry(key).or_insert_with(|| {
                if inline {
                    Item::Value(InlineTable::new().into())
                } else {
                    Item::Table(Table::new())
                }
            });
            if !entry.is_table_like() {
                *entry = Item::Table(Table::new());
            }
            merge(entry.as_table_like_mut().unwrap(), fields, &value);
        }
        (Some(value), _) => set_value(table.entry(key).or_insert(Item::None), value),
    }
    String::new()
}

/// Set or remove element `index` of list setting `list`; `index == len` appends.
fn element(
    list: &str,
    item: &mut Item,
    index: usize,
    kind: Kind,
    value: Option<Value>,
) -> anyhow::Result<()> {
    if item.is_none() {
        if value.is_none() {
            bail!("no such element; {list} has 0");
        }
        *item = match kind {
            Kind::Entry(..) => Item::ArrayOfTables(Default::default()),
            _ => Item::Value(toml_edit::Array::new().into()),
        };
    }
    let len = match &*item {
        Item::ArrayOfTables(tables) => tables.len(),
        Item::Value(Value::Array(array)) => array.len(),
        _ => bail!("{list} in the document is not an array; reset {list} first"),
    };
    if index > len || (index == len && value.is_none()) {
        bail!("no such element; {list} has {len}");
    }
    match (item, value) {
        (Item::ArrayOfTables(tables), None) => {
            tables.remove(index);
        }
        (Item::Value(Value::Array(array)), None) => {
            array.remove(index);
        }
        (Item::ArrayOfTables(tables), Some(value)) => {
            if index == len {
                tables.push(Table::new());
            }
            let Kind::Entry(fields, _) = kind else {
                unreachable!("only agents is an array of tables")
            };
            merge(tables.get_mut(index).unwrap(), fields, &value);
        }
        (Item::Value(Value::Array(array)), Some(value)) => match kind {
            Kind::Entry(fields, _) => {
                if index == len {
                    array.push(InlineTable::new());
                }
                let Some(table) = array.get_mut(index).unwrap().as_inline_table_mut() else {
                    bail!("that element of {list} in the document is not a table");
                };
                merge(table, fields, &value);
            }
            _ if index == len => array.push(value),
            _ => {
                let slot = array.get_mut(index).unwrap();
                let mut value = value;
                *value.decor_mut() = slot.decor().clone();
                *slot = value;
            }
        },
        _ => unreachable!("length checked above"),
    }
    Ok(())
}

fn element_table<'a>(
    list: &str,
    item: &'a mut Item,
    index: usize,
) -> anyhow::Result<&'a mut dyn TableLike> {
    let found: Option<&mut dyn TableLike> = match item {
        Item::ArrayOfTables(tables) => tables.get_mut(index).map(|t| t as &mut dyn TableLike),
        Item::Value(Value::Array(array)) => array
            .get_mut(index)
            .and_then(Value::as_inline_table_mut)
            .map(|t| t as &mut dyn TableLike),
        _ => None,
    };
    found.with_context(|| format!("no such element of {list}"))
}

/// Set each declared field `value` gives and remove the declared ones it
/// leaves out; undeclared fields already in `table` stay.
fn merge(table: &mut dyn TableLike, fields: &[&str], value: &Value) {
    let given = value.as_inline_table().expect("checked by typed_value");
    for field in fields {
        match given.get(field) {
            Some(value) => {
                let mut value = value.clone();
                value.decor_mut().clear();
                set_value(table.entry(field).or_insert(Item::None), value);
            }
            None => {
                table.remove(field);
            }
        }
    }
}

/// Replace a value and keep the comment and spacing around the old one.
fn set_value(item: &mut Item, mut value: Value) {
    if let Some(existing) = item.as_value() {
        *value.decor_mut() = existing.decor().clone();
    }
    *item = Item::Value(value);
}

/// Every edit reads the latest document before changing it. A document that is
/// not TOML is refused; a wrongly typed value elsewhere is the loader's finding
/// to report, and the edit keeps it byte for byte. The new text is published
/// with one rename, so readers never see a partially written document.
fn save(
    path: &Path,
    edit: impl FnOnce(&mut toml_edit::DocumentMut) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    use std::io::Write;

    // A dangling link is not a missing document: never replace the link itself.
    let entry = match std::fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("inspect settings document"),
    };
    // Follow an existing symlink rather than replacing the user's link.
    let path = match std::fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && entry.is_none() => {
            path.to_path_buf()
        }
        Err(error) => return Err(error).context("locate settings document"),
    };
    let permissions = match std::fs::metadata(&path) {
        Ok(metadata) => {
            let permissions = metadata.permissions();
            if permissions.readonly() {
                bail!("settings document {} is read-only", path.display());
            }
            Some(permissions)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("inspect settings document permissions"),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("read settings document"),
    };
    let mut document: toml_edit::DocumentMut = contents.parse().with_context(|| {
        format!(
            "settings document {} is not valid TOML; editing is off until it is fixed by hand",
            path.display()
        )
    })?;
    edit(&mut document)?;
    let contents = document.to_string();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context("create settings directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("create temporary settings document")?;
    temporary
        .write_all(contents.as_bytes())
        .context("write settings document")?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .context("preserve settings document permissions")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("sync settings document")?;
    temporary
        .persist(&path)
        // PersistError owns the temporary file; discard it even if the caller retains the error.
        .map_err(|error| error.error)
        .context("replace settings document")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{
        AgentOverride, AutomaticLocalTerminals, LaunchChoice, ReadySoundChoice, Settings,
    };
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn reading(path: &Path) -> ovrcr_protocol::SettingsReport {
        parse(path, &std::fs::read_to_string(path).unwrap_or_default())
    }

    fn files(dir: &Path) -> usize {
        std::fs::read_dir(dir).unwrap().count()
    }

    /// The requests the Dashboard sends for `N`, `S`, `L` and a remembered
    /// launch; `crates/ovrcr-tui` asserts it emits exactly these.
    const N_ON: (&str, &str) = ("desktop_notifications", "true");
    const S_ON: (&str, &str) = ("ready_sound", "true");
    const C_CHIME: (&str, &str) = ("ready_sound_choice", "\"chime\"");
    const L_ON: (&str, &str) = ("automatic_local_terminals", "\"on\"");
    const AGENT: &str = "{ kind = \"Agent\", preset = \"new\" }";
    const TERMINAL: &str = "{ kind = \"Terminal\" }";

    const ORIGINAL: &str =
        "# my settings\nbranch_prefix = 'kh/' # mine\n\n[future]\nvalue = 42 # keep\n";

    type Check = fn(&Settings) -> bool;

    /// Every setting type: set through a path, read back by the loader with
    /// source document, and reset to the original bytes.
    #[test]
    fn set_and_reset_round_trip_every_setting_type_and_keep_the_rest() {
        let cases: &[(&str, &str, &str, Check)] = &[
            (
                "desktop_notifications",
                "true",
                "desktop_notifications",
                |s| s.desktop_notifications,
            ),
            ("ready_sound", "true", "ready_sound", |s| s.ready_sound),
            ("ready_sound_choice", "chime", "ready_sound_choice", |s| {
                s.ready_sound_choice == Some(ReadySoundChoice::Chime)
            }),
            ("iterm_focus", "true", "iterm_focus", |s| s.iterm_focus),
            (
                "automatic_local_terminals",
                "off",
                "automatic_local_terminals",
                |s| s.automatic_local_terminals == AutomaticLocalTerminals::Off,
            ),
            (
                "automatic_local_terminals",
                "\"on\"",
                "automatic_local_terminals",
                |s| s.automatic_local_terminals == AutomaticLocalTerminals::On,
            ),
            ("title_model", "pi/test", "title_model", |s| {
                s.title_model.as_deref() == Some("pi/test")
            }),
            (
                "picker_roots",
                "[\"/tmp/a\", \"/tmp/b\"]",
                "picker_roots",
                |s| s.picker_roots == [PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
            ),
            ("picker_roots[0]", "/tmp/c", "picker_roots", |s| {
                s.picker_roots == [PathBuf::from("/tmp/c")]
            }),
            (
                "agents",
                "[{ name = \"a\", argv = [\"a\", \"-v\"] }]",
                "agents",
                |s| {
                    s.agents
                        == [AgentOverride {
                            name: "a".into(),
                            argv: vec!["a".into(), "-v".into()],
                        }]
                },
            ),
            (
                "agents[0]",
                "{ name = \"b\", argv = [\"b\"] }",
                "agents",
                |s| {
                    s.agents
                        == [AgentOverride {
                            name: "b".into(),
                            argv: vec!["b".into()],
                        }]
                },
            ),
            (
                "launch_choices",
                "{ p = { kind = \"Terminal\" } }",
                "launch_choices",
                |s| s.launch_choices["p"] == LaunchChoice::Terminal,
            ),
            ("launch_choices.\"p.q\"", AGENT, "launch_choices", |s| {
                s.launch_choices["p.q"] == LaunchChoice::Agent("new".into())
            }),
            ("quota.enabled", "true", "quota.enabled", |s| {
                s.quota.enabled
            }),
            ("quota.claude.probe", "true", "quota.claude.probe", |s| {
                s.quota.claude_probe
            }),
            (
                "quota.codex.command",
                "/opt/codex",
                "quota.codex.command",
                |s| s.quota.codex.command == Path::new("/opt/codex"),
            ),
            ("quota.codex.home", "\"/tmp/h\"", "quota.codex.home", |s| {
                s.quota.codex.home.as_deref() == Some(Path::new("/tmp/h"))
            }),
            (
                "quota.grok.command",
                "grok-cli",
                "quota.grok.command",
                |s| s.quota.grok.command == Path::new("grok-cli"),
            ),
            ("quota.grok.home", "/tmp/g", "quota.grok.home", |s| {
                s.quota.grok.home.as_deref() == Some(Path::new("/tmp/g"))
            }),
        ];
        for &(path, value, reset, check) in cases {
            let root = tempfile::tempdir().unwrap();
            let document = root.path().join("dashboard.toml");
            std::fs::write(&document, ORIGINAL).unwrap();
            set(&document, path, Some(value)).unwrap_or_else(|e| panic!("{path}: {e:#}"));
            let report = reading(&document);
            assert!(
                check(&report.settings),
                "{path} = {value}:\n{}",
                read(&document)
            );
            // `[future]` is the only finding: an unknown table the edit keeps.
            let keys: Vec<_> = report.findings.iter().map(|f| f.key.as_deref()).collect();
            assert_eq!(keys, [Some("future")], "{path}");
            let key = path.split(['.', '[']).next().unwrap();
            let key = if key == "quota" { path } else { key };
            let row = report.rows.iter().find(|row| row.key == key).unwrap();
            assert_eq!(
                row.source,
                ovrcr_protocol::SettingSource::Document,
                "{path}"
            );
            let saved = read(&document);
            for line in ORIGINAL.lines() {
                assert!(saved.contains(line), "{path}: lost {line:?} in\n{saved}");
            }
            assert_eq!(files(root.path()), 1, "no temporary file remains");
            set(&document, reset, None).unwrap();
            assert_eq!(read(&document), ORIGINAL, "reset {reset} after {path}");
        }
        // An existing value keeps its comment; a reset removes the key.
        let root = tempfile::tempdir().unwrap();
        let document = root.path().join("dashboard.toml");
        std::fs::write(&document, ORIGINAL).unwrap();
        set(&document, "branch_prefix", Some("fix/")).unwrap();
        assert_eq!(read(&document), ORIGINAL.replace("'kh/'", "\"fix/\""));
        set(&document, "branch_prefix", None).unwrap();
        assert_eq!(
            read(&document),
            ORIGINAL.replace("branch_prefix = 'kh/' # mine\n", "")
        );
        assert_eq!(reading(&document).settings.branch_prefix, "feature/");
    }

    #[test]
    fn collection_elements_set_append_and_remove_in_place() {
        let root = tempfile::tempdir().unwrap();
        let document = root.path().join("dashboard.toml");
        let original = "picker_roots = [\"/a\", \"/b\"] # roots\n\n[[agents]]\nname = \"x\" # agent\nargv = [\"x\"]\nextra = 1\n";
        std::fs::write(&document, original).unwrap();
        set(&document, "picker_roots[1]", Some("/c")).unwrap();
        set(&document, "picker_roots[2]", Some("\"/d\"")).unwrap();
        assert!(read(&document).starts_with("picker_roots = [\"/a\", \"/c\", \"/d\"] # roots\n"));
        let error = set(&document, "picker_roots[4]", Some("/e")).unwrap_err();
        assert!(
            format!("{error:#}").contains("no such element"),
            "{error:#}"
        );
        set(&document, "picker_roots[0]", None).unwrap();
        assert_eq!(
            reading(&document).settings.picker_roots,
            [PathBuf::from("/c"), PathBuf::from("/d")]
        );

        set(&document, "agents[0].argv", Some("[\"x\", \"-v\"]")).unwrap();
        set(
            &document,
            "agents[1]",
            Some("{ name = \"y\", argv = [\"y\"] }"),
        )
        .unwrap();
        let saved = read(&document);
        assert!(
            saved.contains("name = \"x\" # agent\nargv = [\"x\", \"-v\"]\nextra = 1\n"),
            "{saved}"
        );
        assert!(
            saved.ends_with("[[agents]]\nname = \"y\"\nargv = [\"y\"]\n"),
            "{saved}"
        );
        // Removing a required field would turn the element into a finding.
        let before = read(&document);
        let error = set(&document, "agents[0].name", None).unwrap_err();
        assert!(format!("{error:#}").contains("agents[0].name"), "{error:#}");
        assert_eq!(read(&document), before);
        set(&document, "agents[1]", None).unwrap();
        let agents = reading(&document).settings.agents;
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].argv, ["x", "-v"]);

        set(&document, "launch_choices.p.kind", Some("Terminal")).unwrap();
        assert!(read(&document).ends_with("[launch_choices.p]\nkind = \"Terminal\"\n"));
        set(&document, "launch_choices.p.preset", Some("claude")).unwrap_err();
        set(
            &document,
            "launch_choices.p",
            Some("{ kind = \"Agent\", preset = \"claude\" }"),
        )
        .unwrap();
        assert_eq!(
            reading(&document).settings.launch_choices["p"],
            LaunchChoice::Agent("claude".into())
        );
        set(&document, "launch_choices.p", None).unwrap();
        assert!(reading(&document).settings.launch_choices.is_empty());
    }

    /// Moving a root is one `SetSetting` of the whole list, the write
    /// `picker_roots` already accepts. The comment on the list and the
    /// unrelated key stay as that writer leaves them.
    #[test]
    fn moving_a_picker_root_rewrites_the_list_and_keeps_its_comment() {
        let original = "\
# kept
picker_roots = [\"/a\", \"/b\", \"/c\"] # roots
branch_prefix = 'kh/' # mine
";
        let root = tempfile::tempdir().unwrap();
        let down = root.path().join("down.toml");
        std::fs::write(&down, original).unwrap();
        set(&down, "picker_roots", Some("[\"/b\", \"/a\", \"/c\"]")).unwrap();
        assert_eq!(
            read(&down),
            "\
# kept
picker_roots = [\"/b\", \"/a\", \"/c\"] # roots
branch_prefix = 'kh/' # mine
"
        );
        assert_eq!(
            reading(&down).settings.picker_roots,
            [
                PathBuf::from("/b"),
                PathBuf::from("/a"),
                PathBuf::from("/c")
            ]
        );

        let up = root.path().join("up.toml");
        std::fs::write(&up, original).unwrap();
        set(&up, "picker_roots", Some("[\"/a\", \"/c\", \"/b\"]")).unwrap();
        assert_eq!(
            read(&up),
            "\
# kept
picker_roots = [\"/a\", \"/c\", \"/b\"] # roots
branch_prefix = 'kh/' # mine
"
        );
        assert_eq!(
            reading(&up).settings.picker_roots,
            [
                PathBuf::from("/a"),
                PathBuf::from("/c"),
                PathBuf::from("/b")
            ]
        );
    }

    #[test]
    fn invalid_values_are_rejected_naming_the_expected_type_and_leave_the_document() {
        let original = "# keep\n[[agents]]\nname = \"x\"\nargv = [\"x\"]\n";
        for (path, value, expected) in [
            ("ready_sound", "yes", "expected a boolean"),
            ("ready_sound", "\"yes\"", "expected a boolean"),
            (
                "ready_sound_choice",
                "Glass",
                "expected \"default\", \"tap\", \"chime\" or \"rise\"",
            ),
            (
                "ready_sound_choice",
                "1",
                "expected \"default\", \"tap\", \"chime\" or \"rise\"",
            ),
            ("quota.enabled", "1", "expected a boolean"),
            (
                "automatic_local_terminals",
                "always",
                "expected \"on\", \"off\" or \"default_branch_only\"",
            ),
            (
                "automatic_local_terminals",
                "1",
                "expected \"on\", \"off\" or \"default_branch_only\"",
            ),
            ("branch_prefix", "42", "expected a string"),
            ("title_model", "pi", "provider/model"),
            ("picker_roots", "[1]", "expected an array of paths"),
            ("picker_roots", "nope", "expected an array of paths"),
            ("picker_roots[0]", "true", "expected a path"),
            ("quota.codex.command", "7", "expected a path"),
            ("agents[0].argv", "\"x\"", "expected an array of strings"),
            (
                "agents[1]",
                "\"x\"",
                "expected an inline table { name, argv }",
            ),
            ("launch_choices.p", "{ kind = \"Shell\" }", "Shell"),
            (
                "launch_choices.p",
                "{ kind = \"Agent\", what = 1 }",
                "unknown field what",
            ),
            ("launch_choices.p.kind", "Shell", "Shell"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let document = root.path().join("dashboard.toml");
            std::fs::write(&document, original).unwrap();
            let error = format!("{:#}", set(&document, path, Some(value)).unwrap_err());
            assert!(
                error.starts_with(&format!("could not save {path}: ")),
                "{path} = {value}: {error}"
            );
            assert!(error.contains(expected), "{path} = {value}: {error}");
            assert_eq!(read(&document), original, "{path} = {value}");
            assert_eq!(files(root.path()), 1);
        }
    }

    #[test]
    fn sound_choices_save_reload_and_reset_without_changing_saved_opt_ins() {
        for notifications in [false, true] {
            for sound in [false, true] {
                for choice in [
                    ReadySoundChoice::Default,
                    ReadySoundChoice::Tap,
                    ReadySoundChoice::Chime,
                    ReadySoundChoice::Rise,
                ] {
                    let root = tempfile::tempdir().unwrap();
                    let config = root.path().join("config.toml");
                    let document = root.path().join("dashboard.toml");
                    let original = format!(
                        "# Saved preferences\ndesktop_notifications = {notifications} # banner\nready_sound = {sound} # sound\nbranch_prefix = 'keep/' # unrelated\n"
                    );
                    std::fs::write(&document, &original).unwrap();
                    set(&document, ReadySoundChoice::KEY, Some(choice.as_str())).unwrap();
                    let report = crate::settings::load_document(&config, &document);
                    assert_eq!(report.settings.ready_sound_choice, Some(choice));
                    assert_eq!(report.settings.desktop_notifications, notifications);
                    assert_eq!(report.settings.ready_sound, sound);
                    assert_eq!(report.settings.branch_prefix, "keep/");
                    assert!(report.findings.is_empty());
                    let row = report
                        .rows
                        .iter()
                        .find(|row| row.key == ReadySoundChoice::KEY)
                        .unwrap();
                    assert_eq!(row.source, ovrcr_protocol::SettingSource::Document);
                    assert_eq!(row.value.as_deref(), Some(choice.as_str()));
                    assert_eq!(row.default.as_deref(), Some("default"));
                    assert!(read(&document).starts_with(&original));

                    set(&document, ReadySoundChoice::KEY, None).unwrap();
                    assert_eq!(read(&document), original);
                    let report = crate::settings::load_document(&config, &document);
                    assert_eq!(
                        report.settings.ready_sound_choice,
                        Some(ReadySoundChoice::Default)
                    );
                    assert_eq!(report.settings.desktop_notifications, notifications);
                    assert_eq!(report.settings.ready_sound, sound);
                    assert!(report.findings.is_empty());
                }
            }
        }
    }

    #[test]
    fn invalid_sound_choices_survive_unrelated_edits_and_invalid_saves_are_refused() {
        for raw in ["'Glass'", "' tap'", "true", "7", "['tap']", "{ tap = {} }"] {
            let root = tempfile::tempdir().unwrap();
            let config = root.path().join("config.toml");
            let document = root.path().join("dashboard.toml");
            let choice_line = format!("ready_sound_choice = {raw} # preserve raw choice\n");
            let original = format!(
                "desktop_notifications = true\nready_sound = true\n{choice_line}branch_prefix = 'keep/' # unrelated\n"
            );
            std::fs::write(&document, &original).unwrap();
            let before = crate::settings::load_document(&config, &document);
            assert_eq!(before.settings.ready_sound_choice, None);
            assert!(matches!(before.findings.as_slice(), [finding]
                if finding.key.as_deref() == Some(ReadySoundChoice::KEY)
                    && finding.line == Some(3)));

            // Even requesting the identical invalid value must not report a successful save.
            let error = set(&document, ReadySoundChoice::KEY, Some(raw)).unwrap_err();
            assert!(format!("{error:#}").contains("could not save ready_sound_choice"));
            assert_eq!(read(&document), original);
            set(&document, "branch_prefix", Some("edited/")).unwrap();
            assert!(read(&document).contains(&choice_line));
            assert!(read(&document).contains("# unrelated"));
            let after = crate::settings::load_document(&config, &document);
            assert_eq!(after.settings.ready_sound_choice, None);
            assert_eq!(after.findings, before.findings);
            assert!(after.settings.desktop_notifications && after.settings.ready_sound);
            assert_eq!(after.settings.branch_prefix, "edited/");

            set(&document, ReadySoundChoice::KEY, Some("chime")).unwrap();
            let fixed = crate::settings::load_document(&config, &document);
            assert_eq!(
                fixed.settings.ready_sound_choice,
                Some(ReadySoundChoice::Chime)
            );
            assert!(fixed.findings.is_empty());
            assert!(fixed.settings.desktop_notifications && fixed.settings.ready_sound);
            assert_eq!(fixed.settings.branch_prefix, "edited/");
        }
    }

    #[test]
    fn unknown_paths_are_rejected_without_touching_the_document() {
        let root = tempfile::tempdir().unwrap();
        let document = root.path().join("dashboard.toml");
        for path in [
            "ready_sund",
            "quota",
            "quota.codex",
            "quota.codex.hom",
            "picker_roots[x]",
            "picker_roots[0].name",
            "agents[0].extra",
            "launch_choices.a.b.kind",
            "future.value",
            "",
        ] {
            let error = format!("{:#}", set(&document, path, Some("1")).unwrap_err());
            assert!(
                error.contains(&format!("no setting named {path}")),
                "{error}"
            );
            assert!(set(&document, path, None).is_err());
        }
        assert!(!document.exists());
    }

    #[test]
    fn alert_and_automatic_terminal_edits_produce_the_documents_they_always_did() {
        let root = tempfile::tempdir().unwrap();
        let document = root.path().join("dashboard.toml");
        let original = "# My alerts\ndesktop_notifications = false # notification preference\nready_sound = true # sound preference\nbranch_prefix = 'fix/' # keep formatting\n";
        std::fs::write(&document, original).unwrap();
        set(&document, N_ON.0, Some(N_ON.1)).unwrap();
        assert_eq!(
            read(&document),
            original.replace(
                "desktop_notifications = false",
                "desktop_notifications = true"
            )
        );
        set(&document, S_ON.0, Some("false")).unwrap();
        assert!(read(&document).contains("ready_sound = false # sound preference\n"));

        // A wrongly typed value is replaced when it is the one edited.
        std::fs::write(
            &document,
            "automatic_local_terminals = 42\nbranch_prefix = 'fix/'\n",
        )
        .unwrap();
        set(&document, L_ON.0, Some(L_ON.1)).unwrap();
        assert_eq!(
            read(&document),
            "automatic_local_terminals = \"on\"\nbranch_prefix = 'fix/'\n"
        );
        // A missing document and its directories are created, private.
        let nested = root.path().join("missing/nested/dashboard.toml");
        set(&nested, S_ON.0, Some(S_ON.1)).unwrap();
        assert_eq!(read(&nested), "ready_sound = true\n");
        assert_eq!(
            std::fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(files(nested.parent().unwrap()), 1);
    }

    #[test]
    fn edits_keep_wrongly_typed_values_elsewhere_byte_for_byte() {
        for (path, value) in [N_ON, S_ON, L_ON, ("launch_choices.demo", AGENT)] {
            for original in [
                "ready_sound = 'yes'\n",
                "branch_prefix = 42\n",
                "launch_choices = []\n",
            ] {
                if original.starts_with("launch_choices") && path.starts_with("launch_choices") {
                    continue;
                }
                let root = tempfile::tempdir().unwrap();
                let document = root.path().join("dashboard.toml");
                std::fs::write(&document, original).unwrap();
                set(&document, path, Some(value)).unwrap();
                let saved = read(&document);
                if path == "ready_sound" && original.starts_with("ready_sound") {
                    assert_eq!(saved, "ready_sound = true\n");
                } else {
                    assert!(saved.starts_with(original), "{path}: {saved}");
                }
            }
        }
    }

    #[test]
    fn unparseable_documents_and_non_table_launch_choices_are_refused_unchanged() {
        for (path, value) in [
            N_ON,
            S_ON,
            L_ON,
            ("launch_choices.demo", AGENT),
            ("launch_choices.demo", TERMINAL),
        ] {
            let mut refused = vec![("invalid = [", "editing is off until it is fixed by hand")];
            if path.starts_with("launch_choices") {
                refused.push(("launch_choices = []\n", "not a table"));
            }
            for (original, message) in refused {
                let root = tempfile::tempdir().unwrap();
                let document = root.path().join("dashboard.toml");
                std::fs::write(&document, original).unwrap();
                let error = format!("{:#}", set(&document, path, Some(value)).unwrap_err());
                assert!(error.contains(message), "{path}: {error}");
                assert_eq!(read(&document), original);
                assert_eq!(files(root.path()), 1);
            }
        }
    }

    #[test]
    fn edits_refuse_dangling_links_and_read_only_targets_and_keep_links() {
        for (path, value) in [N_ON, S_ON, C_CHIME, ("launch_choices.demo", AGENT)] {
            for read_only in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let document = root.path().join("dashboard.toml");
                let target = root.path().join("actual.toml");
                symlink("actual.toml", &document).unwrap();
                if read_only {
                    std::fs::write(&target, "# keep\nready_sound = false\n").unwrap();
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444))
                        .unwrap();
                }
                assert!(set(&document, path, Some(value)).is_err());
                assert_eq!(
                    std::fs::read_link(&document).unwrap(),
                    Path::new("actual.toml")
                );
                if read_only {
                    assert_eq!(read(&target), "# keep\nready_sound = false\n");
                    assert_eq!(
                        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                        0o444
                    );
                } else {
                    assert!(!target.exists());
                }
                assert_eq!(files(root.path()), if read_only { 2 } else { 1 });
            }
        }
        // A writable link target is edited through the link, keeping its mode.
        let root = tempfile::tempdir().unwrap();
        let document = root.path().join("dashboard.toml");
        let target = root.path().join("actual.toml");
        let original = "# My settings\nready_sound = true # keep\n[launch_choices.'other.project']\nkind = 'Agent'\npreset = 'other'\n";
        std::fs::write(&target, original).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink("actual.toml", &document).unwrap();
        set(&document, "launch_choices.demo", Some(TERMINAL)).unwrap();
        assert!(
            std::fs::symlink_metadata(&document)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(read(&target).starts_with(original));
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let settings = reading(&target).settings;
        assert_eq!(settings.launch_choices["demo"], LaunchChoice::Terminal);
        assert_eq!(
            settings.launch_choices["other.project"],
            LaunchChoice::Agent("other".into())
        );
    }

    #[test]
    fn publication_failure_preserves_the_target_and_leaves_no_temporary_file() {
        for (path, value) in [N_ON, S_ON, C_CHIME, ("launch_choices.demo", AGENT)] {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join("locked");
            std::fs::create_dir(&directory).unwrap();
            let target = directory.join("actual.toml");
            let document = root.path().join("dashboard.toml");
            let original = "# preserved\nready_sound = false\n";
            std::fs::write(&target, original).unwrap();
            symlink(&target, &document).unwrap();
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
            // Real filesystem failure at temporary-file creation (run as an unprivileged user).
            let error = set(&document, path, Some(value)).unwrap_err();
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(
                format!("{error:#}").contains("create temporary settings document"),
                "{error:#}"
            );
            assert_eq!(read(&target), original);
            assert_eq!(std::fs::read_link(&document).unwrap(), target);
            assert_eq!(files(&directory), 1);
        }
    }

    #[test]
    fn failed_replacement_cleans_temporary_file_before_returning_error() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        // Deterministically obstruct replacement after reading, without a race or fault hook.
        let result = save(&path, |document| {
            document["ready_sound"] = toml_edit::value(true);
            std::fs::create_dir(&path)?;
            Ok(())
        });
        assert!(result.is_err());
        assert!(path.is_dir());
        assert_eq!(files(&path), 0);
        assert_eq!(files(root.path()), 1);
        drop(result);
    }

    #[test]
    fn launch_edits_preserve_existing_table_styles_and_unknown_fields() {
        for original in [
            "# config\n[launch_choices.'project.with.dots'] # project\nkind = 'Agent' # kind\npreset = 'old' # preset\nextra = 42 # future\n",
            "# config\nlaunch_choices = { 'project.with.dots' = { kind = 'Agent', preset = 'old', extra = 42 } } # future\n",
            "# config\nlaunch_choices.'project.with.dots'.kind = 'Agent' # kind\nlaunch_choices.'project.with.dots'.preset = 'old' # preset\nlaunch_choices.'project.with.dots'.extra = 42 # future\n",
        ] {
            let root = tempfile::tempdir().unwrap();
            let document = root.path().join("dashboard.toml");
            std::fs::write(&document, original).unwrap();
            for (value, choice) in [
                (AGENT, LaunchChoice::Agent("new".into())),
                (TERMINAL, LaunchChoice::Terminal),
            ] {
                set(
                    &document,
                    "launch_choices.\"project.with.dots\"",
                    Some(value),
                )
                .unwrap();
                let saved = read(&document);
                assert!(saved.contains("# config"));
                assert!(saved.contains("# future"));
                assert!(saved.contains("extra = 42"));
                if matches!(choice, LaunchChoice::Agent(_)) {
                    assert_eq!(
                        saved,
                        original
                            .replace("'old'", "\"new\"")
                            .replace("'Agent'", "\"Agent\"")
                    );
                } else {
                    assert!(!saved.contains("preset"));
                }
                assert_eq!(
                    reading(&document).settings.launch_choices["project.with.dots"],
                    choice
                );
            }
        }
    }

    #[test]
    fn launch_save_preserves_other_projects_unrelated_settings_and_files() {
        let root = tempfile::tempdir().unwrap();
        let document = root.path().join("dashboard.toml");
        let external = "# external edit\nbranch_prefix = 'fix/' # keep\npicker_roots = ['/tmp']\n[future]\nvalue = 42 # unknown\n[launch_choices.'other.project']\nkind = 'Agent'\npreset = 'other'\n";
        std::fs::write(&document, external).unwrap();
        let occupied = document.with_extension("toml.tmp");
        std::fs::write(&occupied, "unrelated").unwrap();
        let project = "project.with-dots_and-punctuation";
        let path = format!("launch_choices.{}", toml_edit::Key::new(project));
        set(&document, &path, Some(AGENT)).unwrap();
        let saved = read(&document);
        for line in external.lines() {
            assert!(saved.contains(line), "missing {line}");
        }
        let settings = reading(&document).settings;
        assert_eq!(
            settings.launch_choices[project],
            LaunchChoice::Agent("new".into())
        );
        assert_eq!(
            settings.launch_choices["other.project"],
            LaunchChoice::Agent("other".into())
        );
        assert_eq!(settings.launch_choices.len(), 2);
        assert_eq!(settings.branch_prefix, "fix/");
        assert_eq!(read(&occupied), "unrelated");
        assert_eq!(files(root.path()), 2, "no disposable writer file remains");
    }
}
