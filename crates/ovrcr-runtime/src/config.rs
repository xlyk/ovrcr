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
const SCHEMA_VERSION: i64 = 1;

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
    let database = database_path(path);
    if !database.try_exists()? {
        return load_legacy_registry(path);
    }
    let mut connection =
        Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("open registry database {}", database.display()))?;
    connection.busy_timeout(Duration::from_secs(1))?;
    let transaction = connection.transaction()?;
    if is_uninitialized(&transaction)? {
        return load_legacy_registry(path);
    }
    check_schema(&transaction)?;
    read_registry(&transaction)
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

fn open_writable_registry(path: &Path) -> Result<Connection> {
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
                name TEXT NOT NULL,
                path BLOB NOT NULL,
                branch TEXT NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY (project, name)
            );",
        )?;
        write_registry(&transaction, &registry)?;
        transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    } else {
        check_schema(&transaction)?;
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

fn check_schema(connection: &Connection) -> Result<()> {
    let (application, version) = schema_identity(connection)?;
    if application != APPLICATION_ID || version != SCHEMA_VERSION {
        bail!(
            "incompatible registry database (application {application}, schema {version}); \
             expected OVRCR schema {SCHEMA_VERSION}; original storage was not replaced"
        );
    }
    Ok(())
}

fn read_registry(connection: &Connection) -> Result<Registry> {
    let mut projects =
        connection.prepare("SELECT name, repo, workspace_root FROM projects ORDER BY position")?;
    let mut workspaces = connection.prepare(
        "SELECT name, path, branch FROM workspaces WHERE project = ?1 ORDER BY position",
    )?;
    let mut registry = Registry::default();
    let mut rows = projects.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let records = workspaces
            .query_map([&name], |row| {
                Ok(WorkspaceRecord {
                    name: row.get(0)?,
                    path: PathBuf::from(OsString::from_vec(row.get(1)?)),
                    branch: row.get(2)?,
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

fn write_registry(transaction: &Transaction<'_>, registry: &Registry) -> Result<()> {
    transaction.execute_batch("DELETE FROM workspaces; DELETE FROM projects;")?;
    let mut projects = transaction.prepare(
        "INSERT INTO projects (name, repo, workspace_root, position) VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut workspaces = transaction.prepare(
        "INSERT INTO workspaces (project, name, path, branch, position)
         VALUES (?1, ?2, ?3, ?4, ?5)",
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
                workspace.name,
                workspace.path.as_os_str().as_bytes(),
                workspace.branch,
                position as i64,
            ])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
                    name: "cleanup".into(),
                    path: PathBuf::from("/worktrees/consigint/cleanup"),
                    branch: "feature/cleanup".into(),
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
}
