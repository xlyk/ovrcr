use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use ovrcr_protocol::{ProjectRecord, Registry, WorkspaceRecord};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryPath(pub PathBuf);

const APPLICATION_ID: i64 = 0x4f565243;
const SCHEMA_VERSION: i64 = 6;

impl RegistryPath {
    pub fn resolve() -> Result<Self> {
        if let Some(path) = std::env::var_os("OVRCR_CONFIG") {
            return Ok(Self(PathBuf::from(path)));
        }

        let dirs =
            ProjectDirs::from("", "", "ovrcr").context("resolve OVRCR configuration directory")?;
        Ok(Self(dirs.config_dir().join("config.toml")))
    }
}

/// The full config filename remains the instance identity for settings and tasks.
pub fn database_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".sqlite3");
    name.into()
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
    let database = database_path(path);
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

fn load_legacy_registry(path: &Path) -> Result<Registry> {
    let contents = match fs::read_to_string(path) {
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
    if schema_identity(&transaction)?.1 < SCHEMA_VERSION {
        migrate_workspace_identity(&transaction)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
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
    if application != APPLICATION_ID || !matches!(version, 1 | 2 | 3 | 4 | 5 | SCHEMA_VERSION) {
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
    fn registry_round_trip_preserves_projects_and_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
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

        let empty_path = dir.path().join("empty-config.toml");
        save_registry_atomic(&Registry::default(), &empty_path).unwrap();
        assert_eq!(load_registry(&empty_path).unwrap(), Registry::default());
    }

    #[test]
    fn corrupt_registry_is_reported_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[[projects]\n").unwrap();
        let error = load_registry(&path).unwrap_err();
        assert!(
            error.downcast_ref::<toml::de::Error>().is_some(),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "[[projects]\n");
    }

    #[test]
    fn present_empty_registry_is_reported_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let error = load_registry(&path).unwrap_err();
        assert!(
            error.downcast_ref::<toml::de::Error>().is_some(),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "");
    }

    #[test]
    fn offline_v5_reads_name_as_id_without_migrating() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
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
        let path = dir.path().join("config.toml");
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
        let path = dir.path().join("config.toml");
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
        let path = dir.path().join("config.toml");
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

        let path = dir.path().join("config.toml");
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
        let path = dir.path().join("config.toml");
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
        crate::git::remove_worktree(project, workspace).unwrap();
        assert!(
            !worktree.exists(),
            "restored original checkout must remain removable"
        );
    }
}
