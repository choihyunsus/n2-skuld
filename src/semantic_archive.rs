//! Preserve excess scoped semantic edges in the archive before removing hot copies.
use crate::{archive::ArchiveResult, archive_attach::with_archive, database::Scope, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;

pub const SEMANTIC_SCHEMA: &str = include_str!("semantic_edges.sql");
const COLUMNS: &[&str] = &[
    "project",
    "agent",
    "source_path",
    "target_path",
    "similarity",
    "edge_type",
    "model",
    "computed_at",
];

pub fn initialize_semantic_edges(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    validate_version(&transaction, "main")?;
    transaction.execute_batch(SEMANTIC_SCHEMA)?;
    validate_columns(&transaction, "main")?;
    transaction.execute(
        "INSERT OR IGNORE INTO meta(key,value) VALUES('verdandi_semantic_schema_version','1')",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn prune_semantic_edges(
    connection: &mut Connection,
    archive_path: &Path,
    scope: &Scope,
    keep_per_source: usize,
    limit: usize,
) -> Result<ArchiveResult> {
    scope.validate()?;
    if keep_per_source > 10000 || limit == 0 || limit > 1000 {
        return Err("Invalid semantic edge retention limit".into());
    }
    validate_version(connection, "main")?;
    validate_columns(connection, "main")?;
    with_archive(connection, archive_path, true, |connection| {
        prepare_archive(connection)?;
        let edges = copy_edges(connection, scope, keep_per_source, limit)?;
        let moved = delete_copied(connection, &edges, keep_per_source)?;
        Ok(ArchiveResult {
            copied: edges.len(),
            moved,
            retained_after_copy: edges.len() - moved,
        })
    })
}

fn validate_version(connection: &Connection, schema: &str) -> Result<()> {
    let value: Option<String> = connection
        .query_row(
            &format!(
                "SELECT value FROM {schema}.meta WHERE key='verdandi_semantic_schema_version'"
            ),
            [],
            |row| row.get(0),
        )
        .optional()?;
    if value.as_deref().is_some_and(|value| value != "1") {
        return Err("Unsupported Verdandi semantic edge schema".into());
    }
    Ok(())
}

fn validate_columns(connection: &Connection, schema: &str) -> Result<()> {
    let mut statement = connection.prepare(&format!(
        "PRAGMA {schema}.table_info(verdandi_semantic_edges)"
    ))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if columns != COLUMNS {
        return Err(
            "Semantic edges require scoped fields; migrate legacy tables explicitly".into(),
        );
    }
    Ok(())
}

fn prepare_archive(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    validate_version(&transaction, "archive")?;
    transaction.execute_batch(
        &SEMANTIC_SCHEMA
            .replace(
                "CREATE TABLE IF NOT EXISTS ",
                "CREATE TABLE IF NOT EXISTS archive.",
            )
            .replace(
                "CREATE INDEX IF NOT EXISTS ",
                "CREATE INDEX IF NOT EXISTS archive.",
            ),
    )?;
    validate_columns(&transaction, "archive")?;
    transaction.execute("INSERT OR IGNORE INTO archive.meta(key,value) VALUES('verdandi_semantic_schema_version','1')",[])?;
    transaction.commit()?;
    Ok(())
}

#[derive(Debug)]
struct Edge {
    project: String,
    agent: String,
    source: String,
    target: String,
    similarity: f64,
    edge_type: String,
    model: String,
    computed_at: i64,
}

fn copy_edges(
    connection: &mut Connection,
    scope: &Scope,
    keep: usize,
    limit: usize,
) -> Result<Vec<Edge>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let edges=transaction.prepare("SELECT project,agent,source_path,target_path,similarity,edge_type,model,computed_at
        FROM (SELECT *,row_number() OVER(PARTITION BY source_path ORDER BY similarity DESC,target_path) AS position
          FROM main.verdandi_semantic_edges WHERE project=?1 AND agent=?2)
        WHERE position>?3 ORDER BY source_path,position LIMIT ?4")?
        .query_map(params![scope.project,scope.agent,keep as i64,limit as i64],|row|Ok(Edge {
            project:row.get(0)?,agent:row.get(1)?,source:row.get(2)?,target:row.get(3)?,similarity:row.get(4)?,
            edge_type:row.get(5)?,model:row.get(6)?,computed_at:row.get(7)? }))?
        .collect::<std::result::Result<Vec<_>,_>>()?;
    for edge in &edges {
        transaction.execute("INSERT INTO archive.verdandi_semantic_edges(project,agent,source_path,target_path,similarity,edge_type,model,computed_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(project,agent,source_path,target_path)
            DO UPDATE SET similarity=excluded.similarity,edge_type=excluded.edge_type,model=excluded.model,computed_at=excluded.computed_at",
            params![edge.project,edge.agent,edge.source,edge.target,edge.similarity,edge.edge_type,edge.model,edge.computed_at])?;
    }
    transaction.commit()?;
    Ok(edges)
}

fn delete_copied(connection: &mut Connection, edges: &[Edge], keep: usize) -> Result<usize> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut moved = 0;
    for edge in edges {
        moved+=transaction.execute("DELETE FROM main.verdandi_semantic_edges AS e WHERE project=?1 AND agent=?2
            AND source_path=?3 AND target_path=?4 AND similarity=?5 AND edge_type=?6 AND model=?7 AND computed_at=?8
            AND (SELECT COUNT(*) FROM main.verdandi_semantic_edges other WHERE other.project=e.project AND other.agent=e.agent
              AND other.source_path=e.source_path AND (other.similarity>e.similarity OR (other.similarity=e.similarity AND other.target_path<e.target_path)))>=?9
            AND EXISTS(SELECT 1 FROM archive.verdandi_semantic_edges a WHERE
              (a.project,a.agent,a.source_path,a.target_path,a.similarity,a.edge_type,a.model,a.computed_at)=
              (e.project,e.agent,e.source_path,e.target_path,e.similarity,e.edge_type,e.model,e.computed_at))",
            params![edge.project,edge.agent,edge.source,edge.target,edge.similarity,edge.edge_type,edge.model,edge.computed_at,keep as i64])?;
    }
    transaction.commit()?;
    Ok(moved)
}
