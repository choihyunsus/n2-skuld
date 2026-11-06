//! Scoped debt reports share the host database without altering memory observations.
use crate::{database::Scope, scanner::ScanReport, Result};
use rusqlite::{Connection, TransactionBehavior};

pub fn save_scan(
    connection: &mut Connection,
    scope: &Scope,
    report: &ScanReport,
) -> Result<String> {
    scope.validate()?;
    crate::database::validate_experiences(connection, "main")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    validate_version(&transaction)?;
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS skuld_scans (
        id TEXT PRIMARY KEY, project TEXT NOT NULL, agent TEXT NOT NULL, root TEXT NOT NULL,
        complete INTEGER NOT NULL, report_json TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    ); CREATE INDEX IF NOT EXISTS idx_skuld_scans_scope ON skuld_scans(project,agent,root,created_at);")?;
    transaction.execute(
        "INSERT OR IGNORE INTO meta(key,value) VALUES('skuld_scan_schema_version','1')",
        [],
    )?;
    let id = uuid::Uuid::new_v4().to_string();
    transaction.execute("INSERT INTO skuld_scans(id,project,agent,root,complete,report_json) VALUES(?1,?2,?3,?4,?5,?6)",
        rusqlite::params![id, scope.project, scope.agent, report.root, report.complete, serde_json::to_string(report)?])?;
    transaction.commit()?;
    Ok(id)
}

pub fn latest_scan(
    connection: &Connection,
    scope: &Scope,
    root: &str,
) -> Result<Option<ScanReport>> {
    use rusqlite::OptionalExtension;
    scope.validate()?;
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='skuld_scans')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    validate_version(connection)?;
    let value: Option<String> = connection
        .query_row(
            "SELECT report_json FROM skuld_scans WHERE project=?1 AND agent=?2 AND root=?3
        ORDER BY created_at DESC,rowid DESC LIMIT 1",
            rusqlite::params![scope.project, scope.agent, root],
            |row| row.get(0),
        )
        .optional()?;
    value
        .map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
}

fn validate_version(connection: &Connection) -> Result<()> {
    use rusqlite::OptionalExtension;
    let version: Option<String> = connection
        .query_row(
            "SELECT value FROM meta WHERE key='skuld_scan_schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if version.as_deref().is_some_and(|version| version != "1") {
        return Err("Unsupported Skuld scan schema version".into());
    }
    Ok(())
}
