//! Durable-copy-first archival preserves observations and their tag/vector metadata.
use crate::{
    archive_attach::with_archive,
    archive_references::unreferenced_predicate,
    database::{Scope, EXPERIENCE_COLUMNS},
    Result,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveResult {
    pub copied: usize,
    pub moved: usize,
    pub retained_after_copy: usize,
}

pub fn archive_experiences(
    connection: &mut Connection,
    archive_path: &Path,
    scope: &Scope,
    retention_days: u32,
    limit: usize,
) -> Result<ArchiveResult> {
    scope.validate()?;
    if limit == 0 || limit > 1000 || retention_days > 365000 {
        return Err("Invalid archive limit or retention window".into());
    }
    crate::database::validate_experiences(connection, "main")?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA main.synchronous=FULL;")?;
    with_archive(connection, archive_path, true, |connection| {
        let ids = copy_candidates(connection, scope, retention_days, limit)?;
        let moved = delete_copied(connection, &ids)?;
        Ok(ArchiveResult {
            copied: ids.len(),
            moved,
            retained_after_copy: ids.len() - moved,
        })
    })
}

fn copy_candidates(
    connection: &mut Connection,
    scope: &Scope,
    days: u32,
    limit: usize,
) -> Result<Vec<String>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let protected = unreferenced_predicate(&transaction)?;
    let sql = format!("SELECT id FROM main.experiences e WHERE agent=?1 AND project=?2
        AND julianday(created_at)<julianday('now',?3) AND {protected} ORDER BY created_at,id LIMIT ?4");
    let ids = transaction
        .prepare(&sql)?
        .query_map(
            rusqlite::params![
                scope.agent,
                scope.project,
                format!("-{days} days"),
                limit as i64
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for id in &ids {
        copy_observation(&transaction, id)?;
    }
    transaction.commit()?;
    Ok(ids)
}

fn copy_observation(connection: &Connection, id: &str) -> Result<()> {
    let archived: Option<(String, String)> = connection
        .query_row(
            "SELECT agent,project FROM archive.experiences WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let current: (String, String) = connection.query_row(
        "SELECT agent,project FROM main.experiences WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if archived.is_some_and(|owner| owner != current) {
        return Err("Archived ID has a different owner".into());
    }
    let columns = EXPERIENCE_COLUMNS.join(",");
    let updates = EXPERIENCE_COLUMNS
        .iter()
        .filter(|column| **column != "id")
        .map(|column| format!("{column}=excluded.{column}"))
        .collect::<Vec<_>>()
        .join(",");
    connection.execute(&format!("INSERT INTO archive.experiences({columns}) SELECT {columns} FROM main.experiences WHERE id=?1
        ON CONFLICT(id) DO UPDATE SET {updates}"), [id])?;
    connection.execute("DELETE FROM archive.tags WHERE experience_id=?1", [id])?;
    connection.execute("INSERT INTO archive.tags(id,experience_id,level,tag) SELECT id,experience_id,level,tag FROM main.tags WHERE experience_id=?1", [id])?;
    connection.execute(
        "DELETE FROM archive.embeddings WHERE source_type='experience' AND source_id=?1",
        [id],
    )?;
    connection.execute("INSERT INTO archive.embeddings(id,source_type,source_id,vector,model,created_at)
        SELECT id,source_type,source_id,vector,model,created_at FROM main.embeddings WHERE source_type='experience' AND source_id=?1", [id])?;
    Ok(())
}

fn delete_copied(connection: &mut Connection, ids: &[String]) -> Result<usize> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let unreferenced = unreferenced_predicate(&transaction)?;
    let mut moved = 0;
    for id in ids {
        let eligible: bool = transaction.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM main.experiences e WHERE id=?1 AND {unreferenced})"
            ),
            [id],
            |row| row.get(0),
        )?;
        if !eligible || !same_snapshot(&transaction, id)? {
            continue;
        }
        transaction.execute(
            "DELETE FROM main.embeddings WHERE source_type='experience' AND source_id=?1",
            [id],
        )?;
        moved += transaction.execute("DELETE FROM main.experiences WHERE id=?1", [id])?;
    }
    transaction.commit()?;
    Ok(moved)
}

fn same_snapshot(connection: &Connection, id: &str) -> Result<bool> {
    for (table, columns, predicate) in [
        ("experiences", EXPERIENCE_COLUMNS.join(","), "id=?1"),
        (
            "tags",
            "id,experience_id,level,tag".into(),
            "experience_id=?1",
        ),
        (
            "embeddings",
            "id,source_type,source_id,vector,model,created_at".into(),
            "source_type='experience' AND source_id=?1",
        ),
    ] {
        let left = format!("SELECT {columns} FROM main.{table} WHERE {predicate}");
        let right = format!("SELECT {columns} FROM archive.{table} WHERE {predicate}");
        let differs: bool = connection.query_row(
            &format!("SELECT EXISTS({left} EXCEPT {right}) OR EXISTS({right} EXCEPT {left})"),
            [id],
            |row| row.get(0),
        )?;
        if differs {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_and_new_references_between_commits_retain_hot_rows() {
        let root = tempfile::tempdir().unwrap();
        let mut db = crate::database::initialize_database(&root.path().join("hot.db")).unwrap();
        let scope = Scope {
            project: "demo".into(),
            agent: "worker".into(),
        };
        for id in ["changed", "referenced", "unchanged"] {
            db.execute("INSERT INTO experiences(id,agent,project,type,category,context,action,outcome,created_at)
                VALUES(?1,'worker','demo','success','testing','context','action','passed','2020-01-01')",[id]).unwrap();
        }
        with_archive(&mut db, &root.path().join("cold.db"), true, |db| {
            let ids = copy_candidates(db, &scope, 30, 10)?;
            db.execute(
                "UPDATE experiences SET outcome='updated' WHERE id='changed'",
                [],
            )?;
            db.execute_batch(
                "CREATE TABLE new_references(source TEXT REFERENCES experiences(id));
                INSERT INTO new_references VALUES('referenced');",
            )?;
            assert_eq!(delete_copied(db, &ids)?, 1);
            assert_eq!(
                db.query_row("SELECT count(*) FROM main.experiences", [], |row| row
                    .get::<_, i64>(0))?,
                2
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM archive.experiences", [], |row| row
                    .get::<_, i64>(0))?,
                3
            );
            Ok(())
        })
        .unwrap();
    }
}
