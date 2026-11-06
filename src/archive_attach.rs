//! Archive attachment validates ownership, schema and file identity before data transfer.
use crate::{database, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{fs, path::Path};

pub(crate) fn with_archive<T>(
    connection: &mut Connection,
    path: &Path,
    create: bool,
    operation: impl FnOnce(&mut Connection) -> Result<T>,
) -> Result<T> {
    if !connection.is_autocommit() {
        return Err(
            "Archive operations require a dedicated connection outside a transaction".into(),
        );
    }
    database::validate_experiences(connection, "main")?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA main.synchronous=FULL;")?;
    validate_path(connection, path, create)?;
    if create && !path.exists() {
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
        {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        validate_path(connection, path, false)?;
    }
    connection.execute(
        "ATTACH DATABASE ?1 AS archive",
        [path.to_str().ok_or("Archive path must be UTF-8")?],
    )?;
    let result = if create {
        prepare(connection)
    } else {
        validate_existing(connection, "archive")
    }
    .and_then(|()| operation(connection));
    let detached = connection.execute_batch("DETACH DATABASE archive");
    match (result, detached) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(error.into()),
    }
}

fn validate_path(connection: &Connection, path: &Path, create: bool) -> Result<()> {
    let mut databases = connection.prepare("PRAGMA database_list")?;
    let attached = databases
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (name, filename) in attached {
        if name == "archive" {
            return Err("The archive alias is already in use".into());
        }
        if !filename.is_empty()
            && path.exists()
            && same_file::is_same_file(Path::new(&filename), path)?
        {
            return Err("Archive and hot database must be different files".into());
        }
    }
    if !path.exists() && !create {
        return Err("Archive database does not exist".into());
    }
    if create {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn prepare(connection: &mut Connection) -> Result<()> {
    connection.execute_batch("PRAGMA archive.synchronous=FULL;")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let tables: i64 = transaction.query_row("SELECT COUNT(*) FROM archive.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0))?;
    if tables > 0 {
        validate_existing(&transaction, "archive")?;
    }
    let schema = database::SOUL_SCHEMA
        .replace(
            "CREATE TABLE IF NOT EXISTS ",
            "CREATE TABLE IF NOT EXISTS archive.",
        )
        .replace(
            "CREATE VIRTUAL TABLE IF NOT EXISTS ",
            "CREATE VIRTUAL TABLE IF NOT EXISTS archive.",
        )
        .replace(
            "CREATE TRIGGER IF NOT EXISTS ",
            "CREATE TRIGGER IF NOT EXISTS archive.",
        )
        .replace(
            "CREATE INDEX IF NOT EXISTS ",
            "CREATE INDEX IF NOT EXISTS archive.",
        );
    transaction.execute_batch(&schema)?;
    transaction.execute(
        "INSERT OR IGNORE INTO archive.meta(key,value) VALUES('schema_version',?1)",
        [database::SCHEMA_VERSION],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO archive.meta(key,value) VALUES('skuld_archive_schema_version','1')",
        [],
    )?;
    associate_source(&transaction)?;
    database::validate_experiences(&transaction, "archive")?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn open_readonly(source: &Connection, path: &Path) -> Result<Connection> {
    validate_path(source, path, false)?;
    let archive = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    validate_existing(&archive, "main")?;
    let source_id: String = source.query_row(
        "SELECT value FROM meta WHERE key='skuld_database_id'",
        [],
        |row| row.get(0),
    )?;
    let archived_id: String = archive.query_row(
        "SELECT value FROM meta WHERE key='skuld_source_id'",
        [],
        |row| row.get(0),
    )?;
    if source_id != archived_id {
        return Err("Archive belongs to a different source database".into());
    }
    Ok(archive)
}

fn validate_existing(connection: &Connection, schema: &str) -> Result<()> {
    if database::common_version(connection, schema)?.as_deref() != Some(database::SCHEMA_VERSION) {
        return Err("Unsupported archive schema".into());
    }
    let version: Option<String> = connection
        .query_row(
            &format!("SELECT value FROM {schema}.meta WHERE key='skuld_archive_schema_version'"),
            [],
            |row| row.get(0),
        )
        .optional()?;
    if version.as_deref() != Some("1") {
        return Err("File is not a supported Skuld archive".into());
    }
    database::validate_experiences(connection, schema)
}

fn associate_source(connection: &Connection) -> Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO main.meta(key,value) VALUES('skuld_database_id',?1)",
        [uuid::Uuid::new_v4().to_string()],
    )?;
    let source: String = connection.query_row(
        "SELECT value FROM main.meta WHERE key='skuld_database_id'",
        [],
        |row| row.get(0),
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO archive.meta(key,value) VALUES('skuld_source_id',?1)",
        [&source],
    )?;
    let existing: String = connection.query_row(
        "SELECT value FROM archive.meta WHERE key='skuld_source_id'",
        [],
        |row| row.get(0),
    )?;
    if source != existing {
        return Err("Archive belongs to a different source database".into());
    }
    Ok(())
}
