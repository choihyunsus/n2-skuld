//! Protect every foreign-key reference before moving a source observation out of hot storage.
use crate::{database::identifier, Result};
use rusqlite::Connection;

pub(crate) fn unreferenced_predicate(connection: &Connection) -> Result<String> {
    let mut statement = connection.prepare("SELECT name FROM main.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut predicates = Vec::new();
    for table in tables {
        if table == "tags" {
            continue;
        }
        let mut statement = connection.prepare(&format!(
            "PRAGMA main.foreign_key_list({})",
            identifier(&table)
        ))?;
        let keys = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (parent, child_column, parent_column) in keys {
            if !parent.eq_ignore_ascii_case("experiences") {
                continue;
            }
            if parent_column
                .as_deref()
                .is_some_and(|column| column != "id")
            {
                return Err(format!(
                    "Unsupported experience foreign key in {table}; archival stopped"
                )
                .into());
            }
            predicates.push(format!(
                "NOT EXISTS(SELECT 1 FROM main.{} dependency WHERE dependency.{}=e.id)",
                identifier(&table),
                identifier(&child_column)
            ));
        }
    }
    if predicates.is_empty() {
        return Ok("1=1".into());
    }
    Ok(predicates.join(" AND "))
}
