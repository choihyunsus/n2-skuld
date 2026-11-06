use chrono::{TimeZone, Utc};
use n2_skuld::{
    database::{initialize_database, open_database, Scope, SOUL_SCHEMA},
    scan_store::{latest_scan, save_scan},
    scanner::{scan_directory, ScanLimits},
    temporal::freshness_at,
};
use rusqlite::Connection;

#[test]
fn compatible_soul_database_without_version_marker_is_readable() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("soul.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(SOUL_SCHEMA).unwrap();
    drop(db);
    open_database(&path).unwrap();
    initialize_database(&path).unwrap();
}

#[test]
fn scoped_scan_storage_restores_latest_report_and_rejects_future_versions() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store.db");
    std::fs::write(root.path().join("demo.rs"), "// TODO implement").unwrap();
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    let scope = Scope {
        project: "demo".into(),
        agent: "worker".into(),
    };
    let mut db = initialize_database(&path).unwrap();
    save_scan(&mut db, &scope, &report).unwrap();
    drop(db);
    let mut db = open_database(&path).unwrap();
    assert_eq!(
        latest_scan(&db, &scope, &report.root)
            .unwrap()
            .unwrap()
            .findings
            .len(),
        1
    );
    let other = Scope {
        project: "demo".into(),
        agent: "another".into(),
    };
    assert!(latest_scan(&db, &other, &report.root).unwrap().is_none());
    db.execute(
        "UPDATE meta SET value='999' WHERE key='skuld_scan_schema_version'",
        [],
    )
    .unwrap();
    assert!(save_scan(&mut db, &scope, &report).is_err());
    assert!(latest_scan(&db, &scope, &report.root).is_err());
}

#[test]
fn temporal_priority_uses_utc_categories_and_rejects_invalid_dates() {
    let now = Utc.with_ymd_and_hms(2026, 1, 31, 0, 0, 0).unwrap();
    assert!((freshness_at("2026-01-01", "library_api", now) - 0.625).abs() < 1e-9);
    assert!(freshness_at("2026-01-01", "architecture", now) > 0.94);
    assert_eq!(
        freshness_at("2026-01-01T09:00:00+09:00", "library_api", now),
        freshness_at("2026-01-01 00:00:00", "library_api", now)
    );
    assert_eq!(freshness_at("invalid", "general", now), 0.0);
    assert_eq!(freshness_at("2099-01-01", "general", now), 1.0);
}

#[test]
fn sql_temporal_expressions_reject_injection_and_handle_invalid_dates() {
    use n2_skuld::temporal::{freshness_importance_sql, freshness_sql_expr};
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE sample(created_at TEXT,importance INTEGER); INSERT INTO sample VALUES('invalid',10);").unwrap();
    let expression = freshness_importance_sql("created_at", "importance", "general").unwrap();
    assert_eq!(
        db.query_row(&format!("SELECT {expression} FROM sample"), [], |row| row
            .get::<_, f64>(
            0
        ))
        .unwrap(),
        0.0
    );
    assert!(freshness_sql_expr("created_at); DROP TABLE sample;--", "general").is_err());
    assert!(freshness_importance_sql("created_at", "1); DROP TABLE sample;--", "general").is_err());
}
