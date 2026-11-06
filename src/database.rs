//! Shared Soul database initialization and stable ownership identifiers.
use crate::Result;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

pub const SOUL_SCHEMA: &str = include_str!("soul_schema.sql");
pub const SCHEMA_VERSION: &str = "2";
pub const EXPERIENCE_COLUMNS: &[&str] = &[
    "id",
    "timestamp",
    "session_id",
    "agent",
    "project",
    "type",
    "category",
    "severity",
    "context",
    "action",
    "outcome",
    "correction",
    "source_ref",
    "frequency",
    "token_cost",
    "created_at",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scope {
    pub project: String,
    pub agent: String,
}
impl Scope {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.project, &self.agent] {
            if value.trim().is_empty() || value.len() > 512 {
                return Err("Project and agent must contain 1-512 bytes".into());
            }
        }
        Ok(())
    }
}

pub fn initialize_database(path: &Path) -> Result<Connection> {
    let mut connection = Connection::open(path)?;
    configure(&connection)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version = common_version(&transaction, "main")?;
    if version
        .as_deref()
        .is_some_and(|value| value != "1" && value != SCHEMA_VERSION)
    {
        return Err("Unsupported Soul Mimir schema version".into());
    }
    transaction.execute_batch(SOUL_SCHEMA)?;
    transaction.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('schema_version',?1)",
        [SCHEMA_VERSION],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO meta(key,value) VALUES('skuld_database_id',?1)",
        [uuid::Uuid::new_v4().to_string()],
    )?;
    validate_experiences(&transaction, "main")?;
    transaction.commit()?;
    Ok(connection)
}

pub fn open_database(path: &Path) -> Result<Connection> {
    let connection =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure(&connection)?;
    if common_version(&connection, "main")?
        .as_deref()
        .is_some_and(|version| version != SCHEMA_VERSION)
    {
        return Err("Expected a distribution Soul Mimir schema version 2 database".into());
    }
    validate_experiences(&connection, "main")?;
    Ok(connection)
}

fn configure(connection: &Connection) -> Result<()> {
    if rusqlite::version_number() < 3_051_003 {
        return Err("SQLite 3.51.3 or newer is required".into());
    }
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
    Ok(())
}

pub(crate) fn common_version(connection: &Connection, schema: &str) -> Result<Option<String>> {
    let found: bool = connection.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {schema}.sqlite_master WHERE name='meta' AND type='table')"), [], |row| row.get(0))?;
    if !found {
        return Ok(None);
    }
    Ok(connection
        .query_row(
            &format!("SELECT value FROM {schema}.meta WHERE key='schema_version'"),
            [],
            |row| row.get(0),
        )
        .optional()?)
}

pub(crate) fn validate_experiences(connection: &Connection, schema: &str) -> Result<()> {
    if rusqlite::version_number() < 3_051_003 {
        return Err("SQLite 3.51.3 or newer is required".into());
    }
    if common_version(connection, schema)?
        .as_deref()
        .is_some_and(|version| version != "1" && version != SCHEMA_VERSION)
    {
        return Err("Unsupported Soul Mimir schema version".into());
    }
    let mut statement = connection.prepare(&format!("PRAGMA {schema}.table_info(experiences)"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if columns != EXPERIENCE_COLUMNS {
        return Err("Experience columns differ from the distribution Soul contract".into());
    }
    Ok(())
}

pub(crate) fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
