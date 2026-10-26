use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use ovrcr_protocol::{ProjectRecord, Registry, WorkspaceRecord};

/// Directory that names one OVRCR instance (`OVRCR_HOME`).
///
/// Formerly the `config.toml` file path. Callers that still say "registry path"
/// mean this directory: the database, settings document, task store and default
/// socket all live inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryPath(pub PathBuf);

/// Preferred environment variable for the instance directory.
pub const HOME_ENV: &str = "OVRCR_HOME";
/// Deprecated file-path alias kept for one release; maps to the parent directory.
pub const CONFIG_ENV: &str = "OVRCR_CONFIG";

const APPLICATION_ID: i64 = 0x4f565243;
const SCHEMA_VERSION: i64 = 23;

const REGISTRY_DB: &str = "registry.sqlite3";
const LEGACY_IDENTITY: &str = "config.toml";
const LEGACY_DB_SUFFIX: &str = "config.toml.sqlite3";
const LEGACY_TASKS: &str = "config.tasks";
const TASKS_DIR: &str = "tasks";
const SETTINGS_DOCUMENT: &str = "dashboard.toml";
const EVENTS_LOG: &str = "events.jsonl";
const SERVER_SOCK: &str = "server.sock";

/// How [`RegistryPath::resolve`] chose the instance directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolveNotes {
    /// Set when `OVRCR_CONFIG` selected the instance; names the file that was given.
    pub config_alias: Option<PathBuf>,
}

impl RegistryPath {
    pub fn resolve() -> Result<Self> {
        Ok(Self(resolve_home()?.0))
    }

    pub fn resolve_with_notes() -> Result<(Self, ResolveNotes)> {
        let (home, notes) = resolve_home()?;
        Ok((Self(home), notes))
    }
}

fn resolve_home() -> Result<(PathBuf, ResolveNotes)> {
    home_from_env(
        std::env::var_os(HOME_ENV),
        std::env::var_os(CONFIG_ENV),
        default_home_dir()?,
    )
}

fn default_home_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("", "", "ovrcr").context("resolve OVRCR configuration directory")?;
    Ok(dirs.config_dir().to_path_buf())
}

/// Pure resolution used by [`RegistryPath::resolve`] and unit tests.
pub fn home_from_env(
    home: Option<std::ffi::OsString>,
    config: Option<std::ffi::OsString>,
    default_dir: PathBuf,
) -> Result<(PathBuf, ResolveNotes)> {
    let mut notes = ResolveNotes::default();
    if let Some(home) = home {
        return Ok((PathBuf::from(home), notes));
    }
    if let Some(config) = config {
        let config = PathBuf::from(config);
        let home = config
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        notes.config_alias = Some(config);
        return Ok((home, notes));
    }
    Ok((default_dir, notes))
}

/// `registry.sqlite3` inside the instance directory.
pub fn database_path(home: &Path) -> PathBuf {
    home.join(REGISTRY_DB)
}

/// Pre-layout-migration database name beside the old identity file.
pub fn legacy_database_path(home: &Path) -> PathBuf {
    home.join(LEGACY_DB_SUFFIX)
}

/// Preserved `config.toml` identity file from the old layout.
pub fn legacy_identity_path(home: &Path) -> PathBuf {
    home.join(LEGACY_IDENTITY)
}

/// Settings document inside the instance directory.
pub fn settings_document_path(home: &Path) -> PathBuf {
    home.join(SETTINGS_DOCUMENT)
}

/// Scheduled-task store inside the instance directory.
pub fn instance_tasks_dir(home: &Path) -> PathBuf {
    home.join(TASKS_DIR)
}

/// Event log inside the instance directory.
pub fn events_log_path(home: &Path) -> PathBuf {
    home.join(EVENTS_LOG)
}

/// Default server socket inside the instance directory.
pub fn default_socket_path(home: &Path) -> PathBuf {
    home.join(SERVER_SOCK)
}

/// Database path for a read: prefer the new name, else the legacy sibling.
pub fn open_database_path(home: &Path) -> PathBuf {
    let modern = database_path(home);
    if modern.exists() {
        return modern;
    }
    let legacy = legacy_database_path(home);
    if legacy.exists() {
        return legacy;
    }
    modern
}

/// Copy old sibling files to the directory layout once. Never deletes the old names.
pub fn migrate_instance_layout(home: &Path) -> Result<bool> {
    fs::create_dir_all(home)
        .with_context(|| format!("create instance directory {}", home.display()))?;
    let mut migrated = false;

    let modern_db = database_path(home);
    let legacy_db = legacy_database_path(home);
    if !modern_db.exists() && legacy_db.try_exists()? {
        copy_file_private(&legacy_db, &modern_db)?;
        migrated = true;
    }

    let modern_tasks = instance_tasks_dir(home);
    let legacy_tasks = home.join(LEGACY_TASKS);
    if !modern_tasks.exists() && legacy_tasks.try_exists()? {
        copy_tree(&legacy_tasks, &modern_tasks)?;
        migrated = true;
    }

    Ok(migrated)
}

fn copy_file_private(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to)
        .with_context(|| format!("migrate {} to {}", from.display(), to.display()))?;
    let _ = fs::set_permissions(to, fs::Permissions::from_mode(0o600));
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)
        .with_context(|| format!("create migrated directory {}", to.display()))?;
    for entry in
        fs::read_dir(from).with_context(|| format!("read legacy directory {}", from.display()))?
    {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&src, &dst)?;
        } else if file_type.is_file() {
            fs::copy(&src, &dst)
                .with_context(|| format!("migrate {} to {}", src.display(), dst.display()))?;
        }
    }
    Ok(())
}

/// Finding text when `OVRCR_CONFIG` selected the instance.
pub fn config_alias_finding(alias: &Path) -> String {
    let home = alias
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    format!(
        "{CONFIG_ENV}={} is a deprecated file-path alias; set {HOME_ENV}={} instead",
        alias.display(),
        home.display()
    )
}

/// Offline inspection never creates storage or imports legacy records.
pub fn load_registry(path: &Path) -> Result<Registry> {
    let Some(mut connection) = open_readonly_registry(path)? else {
        return load_legacy_registry(path);
    };
    let transaction = connection.transaction()?;
    check_schema(&transaction)?;
    read_registry(&transaction)
}

pub(crate) fn open_readonly_registry(path: &Path) -> Result<Option<Connection>> {
    let database = open_database_path(path);
    if !database.try_exists()? {
        return Ok(None);
    }
    let mut connection =
        Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open registry database {}", database.display()))?;
    connection.busy_timeout(Duration::from_secs(1))?;
    let transaction = connection.transaction()?;
    if is_uninitialized(&transaction)? {
        return Ok(None);
    }
    check_schema(&transaction)?;
    drop(transaction);
    Ok(Some(connection))
}

/// Called by the owning server before publishing inventory.
pub fn initialize_registry(path: &Path) -> Result<Registry> {
    let mut connection = open_writable_registry(path)?;
    let transaction = connection.transaction()?;
    read_registry(&transaction)
}

fn load_legacy_registry(home: &Path) -> Result<Registry> {
    let path = legacy_identity_path(home);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("read registry {}", path.display()));
        }
    };

    let registry: Registry = toml::from_str(&contents).context("parse registry")?;
    registry.validate().context("validate registry")?;
    Ok(registry)
}

pub fn save_registry_atomic(registry: &Registry, path: &Path) -> Result<()> {
    registry.validate().context("validate registry")?;
    let mut connection = open_writable_registry(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check_schema(&transaction)?;
    write_registry(&transaction, registry)?;
    transaction.commit().context("commit registry")
}

pub(crate) fn open_writable_registry(path: &Path) -> Result<Connection> {
    migrate_instance_layout(path)?;
    let database = database_path(path);
    let parent = database
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create registry directory {}", parent.display()))?;
    // SQLite inherits this private mode for its rollback journal.
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&database)
    {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("create registry database {}", database.display()));
        }
    }
    let mut connection = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| format!("open registry database {}", database.display()))?;
    connection.busy_timeout(Duration::from_secs(1))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if is_uninitialized(&transaction)? {
        let registry = load_legacy_registry(path)?;
        transaction.execute_batch(
            "CREATE TABLE projects (
                name TEXT PRIMARY KEY NOT NULL,
                repo BLOB NOT NULL,
                workspace_root BLOB NOT NULL,
                position INTEGER NOT NULL
            );
            CREATE TABLE workspaces (
                project TEXT NOT NULL REFERENCES projects(name),
                id TEXT NOT NULL,
                path BLOB NOT NULL,
                branch TEXT NOT NULL,
                git_identity TEXT,
                setup_pending INTEGER NOT NULL DEFAULT 0 CHECK (setup_pending IN (0, 1)),
                position INTEGER NOT NULL,
                PRIMARY KEY (project, id)
            );",
        )?;
        write_registry(&transaction, &registry)?;
        transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
        transaction.pragma_update(None, "user_version", 1)?;
    } else {
        check_schema(&transaction)?;
    }
    if schema_identity(&transaction)?.1 == 1 {
        crate::retained::create_schema(&transaction)?;
        transaction.pragma_update(None, "user_version", 3)?;
    }
    if schema_identity(&transaction)?.1 < 5 {
        // Archive mainline and the earlier recovery draft both used schema 3.
        // Inspect their structural markers and preserve either history atomically.
        if !crate::retained::has_disposition(&transaction)? {
            transaction.execute_batch("ALTER TABLE retained_sessions ADD COLUMN disposition INTEGER NOT NULL DEFAULT 0 CHECK (disposition IN (0, 1, 2));")?;
        }
        if crate::retained::has_table(&transaction, "claude_conversations")? {
            crate::retained::migrate_claude_conversations(&transaction)?;
        } else if !crate::retained::has_table(&transaction, "agent_conversations")? {
            crate::retained::create_conversation_schema(&transaction)?;
        }
        transaction.pragma_update(None, "user_version", 5)?;
    }
    if schema_identity(&transaction)?.1 < 6 {
        migrate_workspace_identity(&transaction)?;
        transaction.pragma_update(None, "user_version", 6)?;
    }
    if schema_identity(&transaction)?.1 < 7 {
        crate::retained::create_conversation_subject_schema(&transaction)?;
        transaction.pragma_update(None, "user_version", 7)?;
    }
    if schema_identity(&transaction)?.1 < 8 {
        crate::retained::create_delegation_schema(&transaction)?;
        transaction.pragma_update(None, "user_version", 8)?;
    }
    if schema_identity(&transaction)?.1 < 9 {
        // Older writers would discard versioned receipt/result/acceptance metadata.
        // The JSON records and private evidence remain untouched by this upgrade.
        transaction.pragma_update(None, "user_version", 9)?;
    }
    if schema_identity(&transaction)?.1 < 10 {
        // Schema9 writers would drop durable per-child authority and control receipts.
        transaction.pragma_update(None, "user_version", 10)?;
    }
    if schema_identity(&transaction)?.1 < 11 {
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS delegation_workflows (workflow TEXT PRIMARY KEY, metadata TEXT NOT NULL);")?;
        transaction.pragma_update(None, "user_version", 11)?;
    }
    if schema_identity(&transaction)?.1 < 12 {
        // Older writers discard workflow cancellation and native interrupt journals.
        transaction.pragma_update(None, "user_version", 12)?;
    }
    if schema_identity(&transaction)?.1 < 13 {
        // Older writers would discard authenticated question and answer obligations.
        transaction.pragma_update(None, "user_version", 13)?;
    }
    if schema_identity(&transaction)?.1 < 14 {
        // Older writers discard immutable delivery and protected archive journals.
        transaction.pragma_update(None, "user_version", 14)?;
    }
    if schema_identity(&transaction)?.1 < 15 {
        // Older writers discard coding workspace effects and immutable candidate attribution.
        transaction.pragma_update(None, "user_version", 15)?;
    }
    if schema_identity(&transaction)?.1 < 16 {
        // Older writers discard trusted fallback evidence and typed acceptance.
        transaction.pragma_update(None, "user_version", 16)?;
    }
    if schema_identity(&transaction)?.1 < 17 {
        // Older writers discard bounded dependency input witnesses and review contracts.
        transaction.pragma_update(None, "user_version", 17)?;
    }
    if schema_identity(&transaction)?.1 < 18 {
        // Older writers discard independent review policy and finite corrective receipts.
        transaction.pragma_update(None, "user_version", 18)?;
    }
    if schema_identity(&transaction)?.1 < 19 {
        // Older writers discard authenticated readable review-view references.
        transaction.pragma_update(None, "user_version", 19)?;
    }
    if schema_identity(&transaction)?.1 < 20 {
        // Older writers discard native resolved-model/profile/policy authority.
        transaction.pragma_update(None, "user_version", 20)?;
    }
    if schema_identity(&transaction)?.1 < 21 {
        crate::retained::cloud_creation::create_schema(&transaction)?;
        transaction.pragma_update(None, "user_version", 21)?;
    }
    if schema_identity(&transaction)?.1 < 22 {
        // Older writers discard distinct Send claims and exact run receipts.
        transaction.pragma_update(None, "user_version", 22)?;
    }
    transaction.commit().context("commit registry migration")?;
    Ok(connection)
}

fn schema_identity(connection: &Connection) -> Result<(i64, i64)> {
    Ok((
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?,
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?,
    ))
}

fn is_uninitialized(connection: &Connection) -> Result<bool> {
    if schema_identity(connection)? != (0, 0) {
        return Ok(false);
    }
    let objects: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'",
        [],
        |row| row.get(0),
    )?;
    Ok(objects == 0)
}

pub(crate) fn check_schema(connection: &Connection) -> Result<()> {
    let (application, version) = schema_identity(connection)?;
    if application != APPLICATION_ID
        || !matches!(
            version,
            1 | 2
                | 3
                | 4
                | 5
                | 6
                | 7
                | 8
                | 9
                | 10
                | 11
                | 12
                | 13
                | 14
                | 15
                | 16
                | 17
                | 18
                | 19
                | 20
                | 21
                | 22
                | SCHEMA_VERSION
        )
    {
        bail!(
            "incompatible registry database (application {application}, schema {version}); \
             expected OVRCR schema {SCHEMA_VERSION}; original storage was not replaced"
        );
    }
    Ok(())
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(connection.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"),
        [column],
        |row| row.get(0),
    )?)
}

fn migrate_workspace_identity(transaction: &Transaction<'_>) -> Result<()> {
    if table_has_column(transaction, "workspaces", "name")?
        && !table_has_column(transaction, "workspaces", "id")?
    {
        transaction.execute_batch("ALTER TABLE workspaces RENAME COLUMN name TO id;")?;
    }
    if !table_has_column(transaction, "workspaces", "git_identity")? {
        transaction.execute_batch("ALTER TABLE workspaces ADD COLUMN git_identity TEXT;")?;
    }
    if !table_has_column(transaction, "workspaces", "setup_pending")? {
        transaction.execute_batch(
            "ALTER TABLE workspaces ADD COLUMN setup_pending INTEGER NOT NULL DEFAULT 0 CHECK (setup_pending IN (0, 1));",
        )?;
    }
    let mut query = transaction.prepare(
        "SELECT w.project, w.id, w.path, w.branch, p.repo
         FROM workspaces w JOIN projects p ON p.name = w.project",
    )?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                PathBuf::from(OsString::from_vec(row.get(2)?)),
                row.get::<_, String>(3)?,
                PathBuf::from(OsString::from_vec(row.get(4)?)),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(query);
    for (project, id, path, branch, repo) in rows {
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let identity = crate::git::capture_worktree_identity(&path).with_context(|| {
                    format!(
                        "capture Git identity for existing workspace {project}/{id} at {}",
                        path.display()
                    )
                })?;
                transaction.execute(
                    "UPDATE workspaces SET git_identity = ?1 WHERE project = ?2 AND id = ?3 AND git_identity IS NULL",
                    params![identity, project, id],
                )?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(identity) = crate::git::recover_worktree_identity(&repo, &path)? {
                    transaction.execute(
                        "UPDATE workspaces SET git_identity = ?1 WHERE project = ?2 AND id = ?3 AND git_identity IS NULL",
                        params![identity, project, id],
                    )?;
                }
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("inspect workspace {project}/{id} at {}", path.display())
                });
            }
        }
        let root = match (fs::canonicalize(&repo), fs::canonicalize(&path)) {
            (Ok(repo), Ok(path)) => repo == path,
            _ => path == repo,
        };
        if root {
            let expected = crate::git::default_branch(&repo).unwrap_or(branch);
            transaction.execute(
                "UPDATE workspaces SET branch = ?1, setup_pending = 1 WHERE project = ?2 AND id = ?3",
                params![expected, project, id],
            )?;
            continue;
        }
        if let Ok(name) = crate::git::checkout_name(&path)
            && name != branch
        {
            transaction.execute(
                "UPDATE workspaces SET branch = ?1 WHERE project = ?2 AND id = ?3",
                params![name, project, id],
            )?;
        }
    }
    Ok(())
}

fn read_registry(connection: &Connection) -> Result<Registry> {
    let id_column = if table_has_column(connection, "workspaces", "id")? {
        "id"
    } else {
        "name"
    };
    let has_identity = table_has_column(connection, "workspaces", "git_identity")?;
    let extra = if has_identity {
        ", git_identity, setup_pending"
    } else {
        ""
    };
    let mut projects =
        connection.prepare("SELECT name, repo, workspace_root FROM projects ORDER BY position")?;
    let mut workspaces = connection.prepare(&format!(
        "SELECT {id_column}, path, branch{extra} FROM workspaces WHERE project = ?1 ORDER BY position"
    ))?;
    let mut registry = Registry::default();
    let mut rows = projects.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let records = workspaces
            .query_map([&name], |row| {
                let git_identity = if has_identity { row.get(3)? } else { None };
                let setup_pending = if has_identity {
                    row.get::<_, i64>(4)? != 0
                } else {
                    false
                };
                Ok(WorkspaceRecord {
                    id: row.get(0)?,
                    path: PathBuf::from(OsString::from_vec(row.get(1)?)),
                    branch: row.get(2)?,
                    git_identity,
                    setup_pending,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        registry.projects.push(ProjectRecord {
            name,
            repo: PathBuf::from(OsString::from_vec(row.get(1)?)),
            workspace_root: PathBuf::from(OsString::from_vec(row.get(2)?)),
            workspaces: records,
        });
    }
    registry.validate().context("validate registry database")?;
    Ok(registry)
}

pub(crate) fn write_registry(transaction: &Transaction<'_>, registry: &Registry) -> Result<()> {
    transaction.execute_batch("DELETE FROM workspaces; DELETE FROM projects;")?;
    let mut projects = transaction.prepare(
        "INSERT INTO projects (name, repo, workspace_root, position) VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut workspaces = transaction.prepare(
        "INSERT INTO workspaces (project, id, path, branch, git_identity, setup_pending, position)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for (position, project) in registry.projects.iter().enumerate() {
        projects.execute(params![
            project.name,
            project.repo.as_os_str().as_bytes(),
            project.workspace_root.as_os_str().as_bytes(),
            position as i64,
        ])?;
        for (position, workspace) in project.workspaces.iter().enumerate() {
            workspaces.execute(params![
                project.name,
                workspace.id,
                workspace.path.as_os_str().as_bytes(),
                workspace.branch,
                workspace.git_identity,
                i64::from(workspace.setup_pending),
                position as i64,
            ])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn home_prefers_ovrcr_home_over_config_alias() {
        let (home, notes) = home_from_env(
            Some("/inst".into()),
            Some("/other/config.toml".into()),
            PathBuf::from("/default"),
        )
        .unwrap();
        assert_eq!(home, Path::new("/inst"));
        assert!(notes.config_alias.is_none());
    }

    #[test]
    fn home_maps_config_alias_to_parent_directory() {
        let (home, notes) = home_from_env(
            None,
            Some("/x/config.toml".into()),
            PathBuf::from("/default"),
        )
        .unwrap();
        assert_eq!(home, Path::new("/x"));
        assert_eq!(
            notes.config_alias.as_deref(),
            Some(Path::new("/x/config.toml"))
        );
        assert!(config_alias_finding(Path::new("/x/config.toml")).contains("OVRCR_HOME=/x"));
    }

    #[test]
    fn home_defaults_to_platform_config_directory() {
        let (home, notes) = home_from_env(None, None, PathBuf::from("/default/ovrcr")).unwrap();
        assert_eq!(home, Path::new("/default/ovrcr"));
        assert!(notes.config_alias.is_none());
    }

    #[test]
    fn fresh_home_uses_registry_sqlite3_not_legacy_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        save_registry_atomic(&Registry::default(), &home).unwrap();
        assert!(database_path(&home).is_file());
        assert!(!legacy_database_path(&home).exists());
        assert!(!legacy_identity_path(&home).exists());
        assert!(!home.join("config.tasks").exists());
        assert_eq!(load_registry(&home).unwrap(), Registry::default());
    }

    #[test]
    fn migrate_copies_legacy_db_and_tasks_once_without_deleting() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let legacy_db = legacy_database_path(&home);
        std::fs::write(&legacy_db, b"legacy-db").unwrap();
        let legacy_tasks = home.join("config.tasks");
        std::fs::create_dir(&legacy_tasks).unwrap();
        std::fs::write(legacy_tasks.join("state.toml"), "x = 1\n").unwrap();

        assert!(migrate_instance_layout(&home).unwrap());
        assert_eq!(std::fs::read(database_path(&home)).unwrap(), b"legacy-db");
        assert!(home.join("tasks/state.toml").is_file());
        assert!(legacy_db.is_file(), "legacy database must remain");
        assert!(
            legacy_tasks.join("state.toml").is_file(),
            "legacy tasks must remain"
        );

        // Second pass is a no-op once modern names exist.
        std::fs::write(&legacy_db, b"changed-legacy").unwrap();
        assert!(!migrate_instance_layout(&home).unwrap());
        assert_eq!(std::fs::read(database_path(&home)).unwrap(), b"legacy-db");
    }

    #[test]
    fn registry_round_trip_preserves_projects_and_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inst");
        std::fs::create_dir(&path).unwrap();
        let registry = Registry {
            projects: vec![ProjectRecord {
                name: "consigint".into(),
                repo: PathBuf::from("/repo/consigint"),
                workspace_root: PathBuf::from("/worktrees/consigint"),
                workspaces: vec![WorkspaceRecord {
                    id: "cleanup".into(),
                    path: PathBuf::from("/worktrees/consigint/cleanup"),
                    branch: "feature/cleanup".into(),
                    git_identity: Some("1:2".into()),
                    setup_pending: true,
                }],
            }],
        };
        save_registry_atomic(&registry, &path).unwrap();
        assert_eq!(load_registry(&path).unwrap(), registry);

        let empty_path = dir.path().join("empty-inst");
        std::fs::create_dir(&empty_path).unwrap();
        save_registry_atomic(&Registry::default(), &empty_path).unwrap();
        assert_eq!(load_registry(&empty_path).unwrap(), Registry::default());
    }

    #[test]
    fn corrupt_registry_is_reported_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        std::fs::write(legacy_identity_path(&path), "[[projects]\n").unwrap();
        let error = load_registry(&path).unwrap_err();
        assert!(
            error.downcast_ref::<toml::de::Error>().is_some(),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(legacy_identity_path(&path)).unwrap(),
            "[[projects]\n"
        );
    }

    #[test]
    fn present_empty_registry_is_reported_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        std::fs::write(legacy_identity_path(&path), "").unwrap();
        let error = load_registry(&path).unwrap_err();
        assert!(
            error.downcast_ref::<toml::de::Error>().is_some(),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(legacy_identity_path(&path)).unwrap(),
            ""
        );
    }

    #[test]
    fn offline_v5_reads_name_as_id_without_migrating() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf() /* instance home */;
        let database = database_path(&path);
        let connection = Connection::open(&database).unwrap();
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        connection
            .pragma_update(None, "user_version", 5i64)
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    name TEXT PRIMARY KEY NOT NULL,
                    repo BLOB NOT NULL,
                    workspace_root BLOB NOT NULL,
                    position INTEGER NOT NULL
                );
                CREATE TABLE workspaces (
                    project TEXT NOT NULL REFERENCES projects(name),
                    name TEXT NOT NULL,
                    path BLOB NOT NULL,
                    branch TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    PRIMARY KEY (project, name)
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO projects (name, repo, workspace_root, position) VALUES (?1, ?2, ?3, 0)",
                params![
                    "demo",
                    PathBuf::from("/repo/demo").as_os_str().as_bytes(),
                    PathBuf::from("/worktrees/demo").as_os_str().as_bytes(),
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO workspaces (project, name, path, branch, position) VALUES (?1, ?2, ?3, ?4, 0)",
                params![
                    "demo",
                    "legacy",
                    PathBuf::from("/worktrees/demo/legacy").as_os_str().as_bytes(),
                    "feature/legacy",
                ],
            )
            .unwrap();
        drop(connection);

        let registry = load_registry(&path).unwrap();
        assert_eq!(registry.projects[0].workspaces[0].id, "legacy");
        assert_eq!(registry.projects[0].workspaces[0].branch, "feature/legacy");
        assert_eq!(registry.projects[0].workspaces[0].git_identity, None);
        assert!(!registry.projects[0].workspaces[0].setup_pending);
        let version: i64 = Connection::open(&database)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5, "offline load must not migrate");
        assert!(
            table_has_column(&Connection::open(&database).unwrap(), "workspaces", "name").unwrap()
        );
    }

    #[test]
    fn schema_eight_upgrade_preserves_delegation_authority_and_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("instance");
        initialize_registry(&config).unwrap();
        let database = database_path(&config);
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 8).unwrap();
        let instance: String = connection
            .query_row("SELECT id FROM delegation_instance", [], |row| row.get(0))
            .unwrap();
        // An upgrade must preserve raw legacy metadata, including unknown outcomes.
        let metadata = r#"{"handle":"original","identity":{"instance":"i","workflow":"wf","controller":"parent","controller_epoch":1,"task":"t","attempt":1,"contract":1,"request":"original","session":42,"run":7},"receipts":[{"request":"original","operation":"InitialInput","outcome":"Unknown","input":{"written_bytes":7,"total_bytes":100,"submit_written":false}}],"contract_digest":"original-contract","result_digest":"original-result"}"#;
        connection.execute("INSERT INTO delegations(handle,workflow,session,metadata) VALUES('original','wf',42,?1)", [metadata]).unwrap();
        let evidence = config.join("workflows/original");
        std::fs::create_dir_all(&evidence).unwrap();
        for name in ["contract-v1.json", "brief-v1.json", "submission-v1.json"] {
            std::fs::write(evidence.join(name), format!("immutable {name}")).unwrap();
        }
        drop(connection);
        load_registry(&config).unwrap();
        assert_eq!(
            schema_version(&database),
            8,
            "offline reads must not migrate"
        );
        initialize_registry(&config).unwrap();
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
        let connection = Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT id FROM delegation_instance", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            instance
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle='original' AND session=42",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            metadata
        );
        for name in ["contract-v1.json", "brief-v1.json", "submission-v1.json"] {
            assert_eq!(
                std::fs::read_to_string(evidence.join(name)).unwrap(),
                format!("immutable {name}")
            );
        }
        initialize_registry(&config).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM delegations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1,
            "reopening must not recreate delegation tables"
        );
    }

    #[test]
    fn schema_eleven_upgrade_keeps_question_intents_and_unknown_answer_receipts_exact() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("instance");
        initialize_registry(&config).unwrap();
        let database = database_path(&config);
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 11).unwrap();
        // Migration preserves bytes, including incomplete obligations it cannot
        // interpret; only the owning delegation validator can grant authority.
        let metadata = r#"{"handle":"original","questions":[{"reference":{"question_id":"choice","digest":"original-question"},"handled":false}],"answers":[{"request":"answer-original","expected_parent_epoch":3,"control_epoch":7,"digest":"original-answer"}],"receipts":[{"request":"answer-original","operation":"AnswerQuestion","outcome":"Unknown"}]}"#;
        connection.execute("INSERT INTO delegations(handle,workflow,metadata) VALUES('original','workflow',?1)", [metadata]).unwrap();
        drop(connection);
        load_registry(&config).unwrap();
        assert_eq!(schema_version(&database), 11, "offline read never migrates");
        initialize_registry(&config).unwrap();
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
        let stored: String = Connection::open(&database)
            .unwrap()
            .query_row(
                "SELECT metadata FROM delegations WHERE handle='original'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stored, metadata,
            "migration must not drop unknown/partial answer obligations"
        );
    }

    #[test]
    fn schema_fifteen_upgrade_preserves_coding_delivery_and_uncertain_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("instance");
        initialize_registry(&config).unwrap();
        let database = database_path(&config);
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 15).unwrap();
        // Migration preserves opaque metadata; only the owning validators grant authority.
        let assignment = r#"{"coding":{"base":"original-base","git_identity":"original-generation"},"obligation_revision":7,"acceptance":{"result_digest":"original-child-digest"},"archive_receipts":[{"outcome":"StopUnknown"}]}"#;
        let workflow = r#"{"deliveries":[{"reference":{"request":"original-delivery","digest":"original-declaration-digest"}}],"cancellation":null}"#;
        connection.execute("INSERT INTO delegations(handle,workflow,metadata) VALUES('original','workflow',?1)", [assignment]).unwrap();
        connection
            .execute(
                "INSERT INTO delegation_workflows(workflow,metadata) VALUES('workflow',?1)",
                [workflow],
            )
            .unwrap();
        let evidence = config.join("workflows/original");
        std::fs::create_dir_all(&evidence).unwrap();
        for name in [
            "contract-v1.json",
            "brief-v1.json",
            "submission-v1.json",
            "candidate-v1.json",
            "acceptance-v1.json",
        ] {
            std::fs::write(evidence.join(name), format!("immutable original {name}")).unwrap();
        }
        drop(connection);
        load_registry(&config).unwrap();
        assert_eq!(
            schema_version(&database),
            15,
            "offline reads do not migrate the accepted coding schema"
        );
        initialize_registry(&config).unwrap();
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
        let connection = Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle='original'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            assignment
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegation_workflows WHERE workflow='workflow'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            workflow
        );
        for name in [
            "contract-v1.json",
            "brief-v1.json",
            "submission-v1.json",
            "candidate-v1.json",
            "acceptance-v1.json",
        ] {
            assert_eq!(
                std::fs::read_to_string(evidence.join(name)).unwrap(),
                format!("immutable original {name}")
            );
        }
    }

    #[test]
    fn schema_sixteen_upgrade_preserves_typed_fallback_and_immutable_history() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("instance");
        initialize_registry(&home).unwrap();
        let database = database_path(&home);
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 16).unwrap();
        let assignment = r#"{"captures":[{"id":"old-capture"}],"recoveries":[{"digest":"old-recovery"}],"acceptance":{"target":{"ParentRecovery":{"recovery_version":2,"digest":"old-recovery"}}},"previous_versions":[{"result_version":1,"acceptance":{"result_digest":"old-child"}}],"obligation_revision":9}"#;
        connection.execute("INSERT INTO delegations(handle,workflow,metadata) VALUES('original','workflow',?1)", [assignment]).unwrap();
        let evidence = home.join("workflows/original");
        std::fs::create_dir_all(&evidence).unwrap();
        let originals = [
            ("acceptance-v1.json", "immutable legacy child decision"),
            ("acceptance-v2.json", "immutable typed recovery decision"),
            ("parent-fallback-r2-v2.json", "immutable parent recovery"),
        ];
        for (name, bytes) in originals {
            std::fs::write(evidence.join(name), bytes).unwrap();
        }
        drop(connection);
        load_registry(&home).unwrap();
        assert_eq!(
            schema_version(&database),
            16,
            "offline reader must preserve accepted331 schema"
        );
        initialize_registry(&home).unwrap();
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
        let connection = Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle='original'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            assignment
        );
        for (name, bytes) in originals {
            assert_eq!(
                std::fs::read(evidence.join(name)).unwrap(),
                bytes.as_bytes()
            );
        }
    }

    #[test]
    fn schema_seventeen_upgrade_preserves_dependency_witnesses_and_review_publications() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("instance");
        initialize_registry(&home).unwrap();
        let database = database_path(&home);
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 17).unwrap();
        // Opaque accepted metadata must survive migration; owning validators grant authority.
        let assignment = r#"{"dependencies":{"version":4,"lineage":["accepted-input"]},"acceptance":{"result_digest":"accepted-result"},"obligation_revision":9}"#;
        let workflow = r#"{"dependencies":{"version":4,"tasks":["predecessor","successor"],"edges":["accepted-edge"]}}"#;
        connection.execute("INSERT INTO delegations(handle,workflow,metadata) VALUES('original','workflow',?1)", [assignment]).unwrap();
        connection
            .execute(
                "INSERT INTO delegation_workflows(workflow,metadata) VALUES('workflow',?1)",
                [workflow],
            )
            .unwrap();
        let evidence = home.join("workflows/original");
        std::fs::create_dir_all(&evidence).unwrap();
        let originals = [
            ("input-baseline-v1.json", "immutable inherited input bytes"),
            (
                "submission-v1.json",
                "immutable typed review with coding:null",
            ),
            (
                "acceptance-v1.json",
                "immutable accepted dependency decision",
            ),
        ];
        for (name, bytes) in originals {
            std::fs::write(evidence.join(name), bytes).unwrap();
        }
        drop(connection);
        load_registry(&home).unwrap();
        assert_eq!(
            schema_version(&database),
            17,
            "offline read never migrates accepted332 storage"
        );
        initialize_registry(&home).unwrap();
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
        let connection = Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle='original'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            assignment
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegation_workflows WHERE workflow='workflow'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            workflow
        );
        for (name, bytes) in originals {
            assert_eq!(
                std::fs::read(evidence.join(name)).unwrap(),
                bytes.as_bytes()
            );
        }
    }

    #[test]
    fn schema_eighteen_upgrade_preserves_native_proof_policy_and_dependency_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("instance");
        initialize_registry(&config).unwrap();
        let database = database_path(&config);
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 18).unwrap();
        // Opaque preservation is not validation or proof of native availability.
        let metadata = r#"{"native_model":{"requested":"alias","resolved":"native-model","profile":"private-profile","binary_digest":"original-binary"},"native_policy":{"recipe":"original-recipe","settings":"original-settings","mcp":"original-mcp"},"dependency_witnesses":[{"digest":"original-input"}],"review":{"base":"original-review-base"},"receipts":[{"outcome":"Unknown"}]}"#;
        connection
            .execute(
                "INSERT INTO delegations(handle,workflow,metadata) VALUES('native','workflow',?1)",
                [metadata],
            )
            .unwrap();
        let evidence = config.join("workflows/native");
        std::fs::create_dir_all(&evidence).unwrap();
        let names = [
            "claude-settings.json",
            "claude-mcp.json",
            "claude-policy.json",
            "submission-v1.json",
        ];
        for name in names {
            std::fs::write(evidence.join(name), format!("original {name}")).unwrap();
        }
        drop(connection);
        load_registry(&config).unwrap();
        assert_eq!(schema_version(&database), 18, "offline reads never migrate");
        initialize_registry(&config).unwrap();
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
        let stored: String = Connection::open(&database)
            .unwrap()
            .query_row(
                "SELECT metadata FROM delegations WHERE handle='native'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, metadata);
        for name in names {
            assert_eq!(
                std::fs::read_to_string(evidence.join(name)).unwrap(),
                format!("original {name}")
            );
        }
    }

    #[test]
    fn schema_eighteen_upgrade_preserves_original_review_records_and_private_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("instance");
        initialize_registry(&home).unwrap();
        let database = database_path(&home);
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 18).unwrap();
        let original = r#"{"review":{"rounds":[{"declaration":"old-round","evidence":[]}],"repairs":[]},"previous_versions":[{"contract_digest":"original"}]}"#;
        connection.execute("INSERT INTO delegations(handle,workflow,metadata) VALUES('original','workflow',?1)", [original]).unwrap();
        let evidence = home.join("workflows/original");
        std::fs::create_dir_all(&evidence).unwrap();
        for name in [
            "contract-v1.json",
            "candidate-v1.json",
            "review-old-round.json",
            "submission-v1.json",
            "acceptance-v1.json",
        ] {
            std::fs::write(evidence.join(name), format!("immutable original {name}")).unwrap();
        }
        drop(connection);
        load_registry(&home).unwrap();
        assert_eq!(
            schema_version(&database),
            18,
            "offline read never migrates accepted335 storage"
        );
        initialize_registry(&home).unwrap();
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
        let connection = Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle='original'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            original
        );
        for name in [
            "contract-v1.json",
            "candidate-v1.json",
            "review-old-round.json",
            "submission-v1.json",
            "acceptance-v1.json",
        ] {
            assert_eq!(
                std::fs::read_to_string(evidence.join(name)).unwrap(),
                format!("immutable original {name}")
            );
        }
    }

    #[test]
    fn newer_schema_refusal_preserves_delegation_metadata_and_private_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("instance");
        initialize_registry(&config).unwrap();
        let database = database_path(&config);
        let connection = Connection::open(&database).unwrap();
        let newer = SCHEMA_VERSION + 1;
        connection
            .pragma_update(None, "user_version", newer)
            .unwrap();
        let metadata = "newer version authority must survive";
        connection
            .execute(
                "INSERT INTO delegations(handle,workflow,metadata) VALUES('newer','wf',?1)",
                [metadata],
            )
            .unwrap();
        let evidence = config.join("evidence.json");
        std::fs::write(&evidence, "immutable evidence").unwrap();
        drop(connection);
        assert!(
            load_registry(&config)
                .unwrap_err()
                .to_string()
                .contains("incompatible registry database")
        );
        assert!(
            initialize_registry(&config)
                .unwrap_err()
                .to_string()
                .contains("incompatible registry database")
        );
        assert_eq!(schema_version(&database), newer);
        let connection = Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle='newer'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            metadata
        );
        assert_eq!(
            std::fs::read_to_string(evidence).unwrap(),
            "immutable evidence"
        );
    }

    #[test]
    fn writable_v5_migrates_id_and_assigns_git_identity() {
        use std::os::unix::ffi::OsStrExt;
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        for args in [
            ["init", "-b", "main"].as_slice(),
            ["config", "user.name", "OVRCR Tests"].as_slice(),
            ["config", "user.email", "tests@example.invalid"].as_slice(),
            ["commit", "--allow-empty", "-m", "initial"].as_slice(),
        ] {
            let output = Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env_remove("GIT_DIR")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let missing = dir.path().join("gone");
        let path = dir.path().to_path_buf() /* instance home */;
        let database = database_path(&path);
        let connection = Connection::open(&database).unwrap();
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        connection
            .pragma_update(None, "user_version", 5i64)
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    name TEXT PRIMARY KEY NOT NULL,
                    repo BLOB NOT NULL,
                    workspace_root BLOB NOT NULL,
                    position INTEGER NOT NULL
                );
                CREATE TABLE workspaces (
                    project TEXT NOT NULL REFERENCES projects(name),
                    name TEXT NOT NULL,
                    path BLOB NOT NULL,
                    branch TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    PRIMARY KEY (project, name)
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO projects (name, repo, workspace_root, position) VALUES (?1, ?2, ?3, 0)",
                params![
                    "demo",
                    repo.as_os_str().as_bytes(),
                    dir.path().as_os_str().as_bytes(),
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO workspaces (project, name, path, branch, position) VALUES (?1, ?2, ?3, ?4, 0)",
                params!["demo", "root", repo.as_os_str().as_bytes(), "main"],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO workspaces (project, name, path, branch, position) VALUES (?1, ?2, ?3, ?4, 1)",
                params![
                    "demo",
                    "archived",
                    missing.as_os_str().as_bytes(),
                    "feature/gone",
                ],
            )
            .unwrap();
        drop(connection);

        let registry = initialize_registry(&path).unwrap();
        let root = registry.projects[0]
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "root")
            .unwrap();
        assert_eq!(root.path, repo);
        assert_eq!(root.branch, "main");
        assert!(root.git_identity.is_some());
        assert_eq!(
            root.git_identity.as_deref(),
            Some(crate::git::worktree_identity(&repo).unwrap()).as_deref()
        );
        let archived = registry.projects[0]
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "archived")
            .unwrap();
        assert_eq!(archived.branch, "feature/gone");
        assert_eq!(archived.git_identity, None);
        assert!(!archived.setup_pending);
        let connection = Connection::open(&database).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert!(table_has_column(&connection, "workspaces", "id").unwrap());
        assert!(!table_has_column(&connection, "workspaces", "name").unwrap());
    }

    fn init_git_repo(repo: &Path) {
        use std::process::Command;
        std::fs::create_dir(repo).unwrap();
        for args in [
            ["init", "-b", "main"].as_slice(),
            ["config", "user.name", "OVRCR Tests"].as_slice(),
            ["config", "user.email", "tests@example.invalid"].as_slice(),
            ["commit", "--allow-empty", "-m", "initial"].as_slice(),
        ] {
            let output = Command::new("git")
                .args(args)
                .current_dir(repo)
                .env_remove("GIT_DIR")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    fn seed_v5(
        database: &Path,
        repo: &Path,
        workspace_root: &Path,
        workspaces: &[(&str, &Path, &str)],
    ) {
        use std::os::unix::ffi::OsStrExt;
        let connection = Connection::open(database).unwrap();
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        connection
            .pragma_update(None, "user_version", 5i64)
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    name TEXT PRIMARY KEY NOT NULL,
                    repo BLOB NOT NULL,
                    workspace_root BLOB NOT NULL,
                    position INTEGER NOT NULL
                );
                CREATE TABLE workspaces (
                    project TEXT NOT NULL REFERENCES projects(name),
                    name TEXT NOT NULL,
                    path BLOB NOT NULL,
                    branch TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    PRIMARY KEY (project, name)
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO projects (name, repo, workspace_root, position) VALUES (?1, ?2, ?3, 0)",
                params![
                    "demo",
                    repo.as_os_str().as_bytes(),
                    workspace_root.as_os_str().as_bytes(),
                ],
            )
            .unwrap();
        for (position, (name, path, branch)) in workspaces.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO workspaces (project, name, path, branch, position) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        "demo",
                        name,
                        path.as_os_str().as_bytes(),
                        branch,
                        position as i64,
                    ],
                )
                .unwrap();
        }
    }

    fn schema_version(database: &Path) -> i64 {
        Connection::open(database)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn writable_v5_rolls_back_present_identity_failure_then_retries() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_git_repo(&repo);
        let path = dir.path().to_path_buf() /* instance home */;
        let database = database_path(&path);
        seed_v5(
            &database,
            &repo,
            dir.path(),
            &[("root", repo.as_path(), "main")],
        );

        let git_dir = repo.join(".git");
        let hidden = dir.path().join("hidden-git");
        std::fs::rename(&git_dir, &hidden).unwrap();
        let failed = initialize_registry(&path);
        std::fs::rename(&hidden, &git_dir).unwrap();
        assert!(failed.is_err(), "{failed:?}");
        assert_eq!(
            schema_version(&database),
            5,
            "present checkout identity failure must not commit schema 6"
        );

        let registry = initialize_registry(&path).unwrap();
        assert_eq!(
            registry.projects[0].workspaces[0].git_identity.as_deref(),
            Some(crate::git::worktree_identity(&repo).unwrap()).as_deref()
        );
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
    }

    #[test]
    fn writable_v5_inaccessible_path_does_not_commit_null_identity() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_git_repo(&repo);
        let hidden_root = dir.path().join("hidden");
        std::fs::create_dir(&hidden_root).unwrap();
        let feature = hidden_root.join("feature");
        std::fs::create_dir(&feature).unwrap();
        let path = dir.path().to_path_buf() /* instance home */;
        let database = database_path(&path);
        seed_v5(
            &database,
            &repo,
            dir.path(),
            &[
                ("root", repo.as_path(), "main"),
                ("feature", feature.as_path(), "feature/x"),
            ],
        );

        let original = std::fs::metadata(&hidden_root).unwrap().permissions();
        let mut denied = original.clone();
        denied.set_mode(0o000);
        std::fs::set_permissions(&hidden_root, denied).unwrap();
        let failed = initialize_registry(&path);
        std::fs::set_permissions(&hidden_root, original).unwrap();
        assert!(failed.is_err(), "{failed:?}");
        assert_eq!(
            schema_version(&database),
            5,
            "inaccessible path must not be treated as missing and committed with null identity"
        );
    }

    #[test]
    fn writable_v5_recovers_missing_worktree_identity_from_admin_metadata() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        init_git_repo(&repo);
        let work = dir.path().join("feature");
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                "feature/kept",
                work.to_str().unwrap(),
                "main",
            ])
            .current_dir(&repo)
            .env_remove("GIT_DIR")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let canonical = work.canonicalize().unwrap();
        std::fs::remove_dir_all(&canonical).unwrap();

        let path = dir.path().to_path_buf() /* instance home */;
        let database = database_path(&path);
        seed_v5(
            &database,
            &repo,
            dir.path(),
            &[
                ("root", repo.as_path(), "main"),
                ("feature", canonical.as_path(), "feature/kept"),
            ],
        );

        let registry = initialize_registry(&path).unwrap();
        let feature = registry.projects[0]
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "feature")
            .unwrap();
        let recovered = crate::git::recover_worktree_identity(&repo, &canonical).unwrap();
        assert_eq!(feature.git_identity, recovered);
        assert!(
            feature.git_identity.is_some(),
            "surviving v5 admin must receive a generation marker"
        );
        assert_eq!(schema_version(&database), SCHEMA_VERSION);
    }

    #[test]
    fn writable_v5_unavailable_repository_defers_migration_until_it_returns() {
        let dir = tempfile::tempdir().unwrap();
        let volume = dir.path().join("volume");
        std::fs::create_dir(&volume).unwrap();
        let volume = volume.canonicalize().unwrap();
        let repo = volume.join("repo");
        init_git_repo(&repo);
        let worktree = volume.join("work");
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["worktree", "add", "-b", "feature/work"])
            .arg(&worktree)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let path = dir.path().to_path_buf() /* instance home */;
        let database = database_path(&path);
        seed_v5(
            &database,
            &repo,
            &volume,
            &[("work", &worktree, "feature/work")],
        );
        let unavailable = dir.path().join("unavailable-volume");
        std::fs::rename(&volume, &unavailable).unwrap();
        let failed = initialize_registry(&path);
        std::fs::rename(&unavailable, &volume).unwrap();
        assert!(
            failed.is_err(),
            "an unavailable volume must leave migration retryable"
        );
        assert_eq!(schema_version(&database), 5);
        let registry = initialize_registry(&path).unwrap();
        let project = &registry.projects[0];
        let workspace = &project.workspaces[0];
        let inspection = crate::git::inspect_worktree(project, workspace).unwrap();
        assert_eq!(inspection.branch, "feature/work");
        crate::git::remove_worktree(project, workspace, false).unwrap();
        assert!(
            !worktree.exists(),
            "restored original checkout must remain removable"
        );
    }
}
