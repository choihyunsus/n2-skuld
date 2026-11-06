//! UTC category-specific knowledge freshness compatible with the distribution contract.
use chrono::{DateTime, Utc};
const DECAY_RATES: &[(&str, f64)] = &[
    ("architecture", 0.002),
    ("code_pattern", 0.005),
    ("security", 0.008),
    ("doc_learn", 0.01),
    ("general", 0.01),
    ("library_api", 0.02),
];

pub fn decay_rate_for(category: &str) -> f64 {
    let lower = category.to_lowercase();
    DECAY_RATES
        .iter()
        .find(|(name, _)| lower.contains(name))
        .map(|(_, rate)| *rate)
        .unwrap_or(0.01)
}

pub fn freshness_weight(timestamp: &str, category: &str) -> f64 {
    freshness_at(timestamp, category, Utc::now())
}

pub fn freshness_sql_expr(timestamp_column: &str, category: &str) -> crate::Result<String> {
    let allowed = [
        "timestamp",
        "updated_at",
        "created_at",
        "computed_at",
        "i.updated_at",
        "e.timestamp",
        "i.created_at",
        "datetime(computed_at, 'unixepoch')",
    ];
    if !allowed.contains(&timestamp_column) {
        return Err("Unsupported timestamp column".into());
    }
    let rate = decay_rate_for(category);
    Ok(format!(
        "CASE WHEN julianday({timestamp_column}) IS NULL THEN 0.0 ELSE
        (1.0/(1.0+{rate}*MAX(0,julianday('now')-julianday({timestamp_column})))) END"
    ))
}

pub fn freshness_importance_sql(
    timestamp_column: &str,
    importance_column: &str,
    category: &str,
) -> crate::Result<String> {
    if !["importance", "i.importance"].contains(&importance_column) {
        return Err("Unsupported importance column".into());
    }
    let freshness = freshness_sql_expr(timestamp_column, category)?;
    Ok(format!(
        "(({freshness})*MAX(1,COALESCE({importance_column},1)))"
    ))
}

pub fn freshness_at(timestamp: &str, category: &str, now: DateTime<Utc>) -> f64 {
    let parsed = DateTime::parse_from_rfc3339(timestamp)
        .map(|value| value.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S%.f")
                .map(|value| value.and_utc())
        })
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|value| value.and_utc())
        })
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(timestamp, "%Y-%m-%d")
                .map(|value| value.and_hms_opt(0, 0, 0).expect("midnight").and_utc())
        });
    let Ok(created) = parsed else {
        return 0.0;
    };
    let days = (now - created).num_milliseconds().max(0) as f64 / 86_400_000.0;
    1.0 / (1.0 + decay_rate_for(category) * days)
}
