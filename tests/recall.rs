use n2_skuld::{
    archive::archive_experiences,
    database::{initialize_database, Scope},
    recall::deep_recall,
};
use rusqlite::Connection;

fn scope() -> Scope {
    Scope {
        agent: "worker".into(),
        project: "demo".into(),
    }
}
fn insert(db: &Connection, id: &str, agent: &str, created: &str) {
    db.execute(
        "INSERT INTO experiences(id,agent,project,type,category,context,action,outcome,created_at)
        VALUES(?1,?2,'demo','success','testing','fixture','shared keyword','passed',?3)",
        [id, agent, created],
    )
    .unwrap();
}

#[test]
fn recalls_hot_and_archive_with_scope_provenance_and_no_source_mutation() {
    let root = tempfile::tempdir().unwrap();
    let mut db = initialize_database(&root.path().join("hot.db")).unwrap();
    let cold = root.path().join("archive.db");
    insert(&db, "cold", "worker", "2020-01-01");
    insert(&db, "foreign", "other-worker", "2020-01-01");
    archive_experiences(&mut db, &cold, &scope(), 30, 10).unwrap();
    insert(&db, "hot", "worker", "2099-01-01");
    let before = std::fs::read(&cold).unwrap();
    let report = deep_recall(&db, &scope(), "keyword", "testing", 10, Some(&cold)).unwrap();
    assert_eq!(report.results.len(), 2);
    assert!(!report.truncated);
    assert_eq!(report.results[0].source_id, "hot");
    assert_eq!(report.results[1].source_tier, 3);
    assert!(report.results.iter().all(|item| item.agent == "worker"));
    assert_eq!(before, std::fs::read(&cold).unwrap());
    assert!(deep_recall(
        &db,
        &scope(),
        "keyword",
        "",
        10,
        Some(&root.path().join("missing.db"))
    )
    .is_err());
    assert!(!root.path().join("missing.db").exists());
}

#[test]
fn hot_record_overrides_stale_archived_copy_even_when_it_no_longer_matches() {
    let root = tempfile::tempdir().unwrap();
    let mut db = initialize_database(&root.path().join("hot.db")).unwrap();
    let cold = root.path().join("archive.db");
    insert(&db, "duplicate", "worker", "2020-01-01");
    db.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON experiences BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(archive_experiences(&mut db, &cold, &scope(), 30, 10).is_err());
    db.execute(
        "UPDATE experiences SET action='superseded' WHERE id='duplicate'",
        [],
    )
    .unwrap();
    let report = deep_recall(&db, &scope(), "keyword", "", 10, Some(&cold)).unwrap();
    assert!(report.results.is_empty());
    assert_eq!(
        deep_recall(&db, &scope(), "superseded", "", 10, Some(&cold))
            .unwrap()
            .results
            .len(),
        1
    );
}

#[test]
fn project_insights_require_matching_source_ownership() {
    let root = tempfile::tempdir().unwrap();
    let db = initialize_database(&root.path().join("hot.db")).unwrap();
    insert(&db, "own", "worker", "2020-01-01");
    insert(&db, "foreign", "other-worker", "2020-01-01");
    for id in ["valid", "mixed", "unlinked"] {
        db.execute("INSERT INTO insights(id,agent,description,category) VALUES(?1,'worker','lesson keyword','testing')",[id]).unwrap();
    }
    db.execute_batch(
        "INSERT INTO experience_insight_links VALUES('own','valid','supports');
        INSERT INTO experience_insight_links VALUES('own','mixed','supports');
        INSERT INTO experience_insight_links VALUES('foreign','mixed','supports');",
    )
    .unwrap();
    let report = deep_recall(&db, &scope(), "lesson keyword", "", 10, None).unwrap();
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].source_id, "valid");
    assert_eq!(report.results[0].project.as_deref(), Some("demo"));
    assert!(deep_recall(&db, &scope(), "%_\"'", "", 10, None)
        .unwrap()
        .results
        .is_empty());
}
