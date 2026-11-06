//! Bounded, owner-scoped recall across the active store and its paired archive.
use crate::{archive_attach, database::Scope, temporal, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{collections::HashSet, path::Path};

const CANDIDATE_LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct RecallItem {
    pub source_id: String,
    pub source_type: String,
    pub source_tier: u8,
    pub agent: String,
    pub project: Option<String>,
    pub content: String,
    pub category: String,
    pub created_at: String,
    pub status: Option<String>,
    pub relevance: f64,
    pub freshness: f64,
    pub final_score: f64,
}

#[derive(Debug, Serialize)]
pub struct RecallReport {
    pub results: Vec<RecallItem>,
    pub candidate_limit_per_source: usize,
    pub truncated: bool,
}

pub fn deep_recall(
    connection: &Connection,
    scope: &Scope,
    query: &str,
    category: &str,
    top_k: usize,
    archive_path: Option<&Path>,
) -> Result<RecallReport> {
    scope.validate()?;
    if query.trim().is_empty() || query.chars().count() > 1000 || top_k > CANDIDATE_LIMIT {
        return Err("Query must contain 1-1000 characters and top_k must be at most 200".into());
    }
    let fts = fts_query(query);
    let mut report = RecallReport {
        results: Vec::new(),
        candidate_limit_per_source: CANDIDATE_LIMIT,
        truncated: false,
    };
    if top_k == 0 || fts.is_empty() {
        return Ok(report);
    }
    append(
        &mut report,
        experiences(connection, scope, &fts, category, 1)?,
    );
    append(&mut report, insights(connection, scope, query, category)?);
    if let Some(path) = archive_path {
        append_archive(&mut report, connection, path, scope, &fts, category)?;
    }
    let mut seen = HashSet::new();
    report
        .results
        .retain(|item| seen.insert((item.source_type.clone(), item.source_id.clone())));
    report.results.sort_by(|left, right| {
        right
            .final_score
            .total_cmp(&left.final_score)
            .then(left.source_tier.cmp(&right.source_tier))
            .then(left.source_id.cmp(&right.source_id))
    });
    report.truncated |= report.results.len() > top_k;
    report.results.truncate(top_k);
    Ok(report)
}

fn append_archive(
    report: &mut RecallReport,
    connection: &Connection,
    path: &Path,
    scope: &Scope,
    fts: &str,
    category: &str,
) -> Result<()> {
    let archive = archive_attach::open_readonly(connection, path)?;
    let mut archived = experiences(&archive, scope, fts, category, 3)?;
    report.truncated |= archived.len() > CANDIDATE_LIMIT;
    let mut retained = Vec::new();
    for item in archived.drain(..) {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM experiences WHERE id=?1)",
            [&item.source_id],
            |row| row.get(0),
        )?;
        if !exists {
            retained.push(item);
        }
    }
    append(report, retained);
    Ok(())
}

fn append(report: &mut RecallReport, mut items: Vec<RecallItem>) {
    report.truncated |= items.len() > CANDIDATE_LIMIT;
    items.truncate(CANDIDATE_LIMIT);
    for (index, item) in items.iter_mut().enumerate() {
        item.relevance = 1.0 / (index + 1) as f64;
        item.freshness = temporal::freshness_weight(&item.created_at, &item.category);
        item.final_score = item.relevance * item.freshness;
    }
    report.results.extend(items);
}

fn experiences(
    connection: &Connection,
    scope: &Scope,
    query: &str,
    category: &str,
    tier: u8,
) -> Result<Vec<RecallItem>> {
    let mut statement = connection.prepare(
        "SELECT e.id,e.agent,e.project,e.context,e.action,e.outcome,e.category,e.created_at
        FROM experiences_fts f JOIN experiences e ON e.rowid=f.rowid
        WHERE experiences_fts MATCH ?1 AND e.agent=?2 AND e.project=?3 AND (?4='' OR e.category=?4)
        ORDER BY f.rank,e.id LIMIT ?5",
    )?;
    let values = statement
        .query_map(
            params![
                query,
                scope.agent,
                scope.project,
                category,
                (CANDIDATE_LIMIT + 1) as i64
            ],
            |row| {
                let content = format!(
                    "{}\n{}\n{}",
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?
                );
                Ok(RecallItem {
                    source_id: row.get(0)?,
                    agent: row.get(1)?,
                    project: row.get(2)?,
                    content: content.chars().take(4096).collect(),
                    category: row.get(6)?,
                    created_at: row.get(7)?,
                    source_type: "experience".into(),
                    source_tier: tier,
                    status: None,
                    relevance: 0.0,
                    freshness: 0.0,
                    final_score: 0.0,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(values)
}

fn insights(
    connection: &Connection,
    scope: &Scope,
    query: &str,
    category: &str,
) -> Result<Vec<RecallItem>> {
    let mut statement = connection.prepare("SELECT i.id,i.agent,i.description,i.category,i.updated_at,i.status,i.scope FROM insights i
        WHERE i.agent=?1 AND i.status IN ('active','graduated') AND instr(lower(i.description),lower(?2))>0
        AND (?3='' OR i.category=?3) AND (i.scope IN ('global','agent') OR (
          EXISTS(SELECT 1 FROM experience_insight_links l JOIN experiences e ON e.id=l.experience_id
            WHERE l.insight_id=i.id AND e.project=?4 AND e.agent=i.agent)
          AND NOT EXISTS(SELECT 1 FROM experience_insight_links l JOIN experiences e ON e.id=l.experience_id
            WHERE l.insight_id=i.id AND (e.project<>?4 OR e.agent<>i.agent))))
        ORDER BY i.importance DESC,i.updated_at DESC,i.id LIMIT ?5")?;
    let values = statement
        .query_map(
            params![
                scope.agent,
                query,
                category,
                scope.project,
                (CANDIDATE_LIMIT + 1) as i64
            ],
            |row| {
                let project =
                    (row.get::<_, String>(6)? == "project").then(|| scope.project.clone());
                Ok(RecallItem {
                    source_id: row.get(0)?,
                    agent: row.get(1)?,
                    project,
                    content: row.get::<_, String>(2)?.chars().take(4096).collect(),
                    category: row.get(3)?,
                    created_at: row.get(4)?,
                    status: row.get(5)?,
                    source_type: "insight".into(),
                    source_tier: 1,
                    relevance: 0.0,
                    freshness: 0.0,
                    final_score: 0.0,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(values)
}

fn fts_query(query: &str) -> String {
    query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(" ")
}
