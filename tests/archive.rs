use n2_skuld::{
    archive::archive_experiences,
    database::{initialize_database, Scope},
};
use rusqlite::Connection;

fn scope() -> Scope {
    Scope {
        agent: "worker".into(),
        project: "demo".into(),
    }
}
fn insert(db: &Connection, id: &str, agent: &str) {
    db.execute("INSERT INTO experiences(id,agent,project,type,category,context,action,outcome,source_ref,created_at)
        VALUES(?1,?2,'demo','success','testing','context','archive check','passed','fixture','2020-01-01')", [id, agent]).unwrap();
}

#[test]
fn archive_preserves_all_metadata_and_scope_across_restart() {
    let root = tempfile::tempdir().unwrap();
    let hot = root.path().join("hot.db");
    let cold = root.path().join("archive.db");
    let mut db = initialize_database(&hot).unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    insert(&db, "one", "worker");
    insert(&db, "other", "other-worker");
    db.execute(
        "INSERT INTO tags(experience_id,level,tag) VALUES('one',2,'portable')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO embeddings(id,source_type,source_id,vector,model) VALUES('v','experience','one',x'0000803F','model')", []).unwrap();
    let moved = archive_experiences(&mut db, &cold, &scope(), 30, 10).unwrap();
    assert_eq!(moved.moved, 1);
    assert_eq!(
        db.query_row("SELECT id FROM experiences", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "other"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM tags", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);
    let archive = Connection::open(&cold).unwrap();
    assert_eq!(
        archive
            .query_row("SELECT source_ref FROM experiences", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "fixture"
    );
    assert_eq!(
        archive
            .query_row("SELECT tag FROM tags", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "portable"
    );
    assert_eq!(
        archive
            .query_row("SELECT hex(vector) FROM embeddings", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "0000803F"
    );
    assert_eq!(
        archive
            .query_row(
                "SELECT count(*) FROM experiences_fts WHERE experiences_fts MATCH 'archive'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        archive
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn evidence_foreign_keys_protect_observations_even_with_cascading_delete() {
    let root = tempfile::tempdir().unwrap();
    let mut db = initialize_database(&root.path().join("hot.db")).unwrap();
    insert(&db, "protected", "worker");
    insert(&db, "move", "worker");
    db.execute_batch(
        "CREATE TABLE evidence(id TEXT, source TEXT REFERENCES experiences(id) ON DELETE CASCADE);
        INSERT INTO evidence VALUES('evidence','protected');",
    )
    .unwrap();
    let moved =
        archive_experiences(&mut db, &root.path().join("archive.db"), &scope(), 30, 10).unwrap();
    assert_eq!(moved.moved, 1);
    assert_eq!(
        db.query_row("SELECT id FROM experiences", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "protected"
    );
}

#[test]
fn failed_hot_delete_retains_committed_archive_and_retry_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let mut db = initialize_database(&root.path().join("hot.db")).unwrap();
    let cold = root.path().join("archive.db");
    insert(&db, "retry", "worker");
    db.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON experiences BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(archive_experiences(&mut db, &cold, &scope(), 30, 10).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM experiences", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let archive = Connection::open(&cold).unwrap();
    assert_eq!(
        archive
            .query_row("SELECT count(*) FROM experiences", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(archive);
    db.execute_batch("DROP TRIGGER fail_delete;").unwrap();
    assert_eq!(
        archive_experiences(&mut db, &cold, &scope(), 30, 10)
            .unwrap()
            .moved,
        1
    );
    assert_eq!(
        archive_experiences(&mut db, &cold, &scope(), 30, 10)
            .unwrap()
            .moved,
        0
    );
}

#[test]
fn rejects_same_file_hardlink_foreign_archive_and_future_schema() {
    let root = tempfile::tempdir().unwrap();
    let hot = root.path().join("hot.db");
    let cold = root.path().join("archive.db");
    let mut db = initialize_database(&hot).unwrap();
    insert(&db, "one", "worker");
    assert!(archive_experiences(&mut db, &hot, &scope(), 30, 10).is_err());
    let link = root.path().join("link.db");
    std::fs::hard_link(&hot, &link).unwrap();
    assert!(archive_experiences(&mut db, &link, &scope(), 30, 10).is_err());
    archive_experiences(&mut db, &cold, &scope(), 30, 10).unwrap();
    let mut other = initialize_database(&root.path().join("other.db")).unwrap();
    insert(&other, "two", "worker");
    assert!(archive_experiences(&mut other, &cold, &scope(), 30, 10).is_err());
    other
        .execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
        .unwrap();
    drop(other);
    assert!(initialize_database(&root.path().join("other.db")).is_err());
}

#[test]
fn archive_creation_works_when_source_was_opened_without_create_permission() {
    let root = tempfile::tempdir().unwrap();
    let hot = root.path().join("hot.db");
    let cold = root.path().join("cold.db");
    let db = initialize_database(&hot).unwrap();
    insert(&db, "one", "worker");
    drop(db);
    let mut db = n2_skuld::database::open_database(&hot).unwrap();
    assert_eq!(
        archive_experiences(&mut db, &cold, &scope(), 30, 10)
            .unwrap()
            .moved,
        1
    );
    db.execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
        .unwrap();
    assert!(archive_experiences(&mut db, &cold, &scope(), 30, 10).is_err());
}
