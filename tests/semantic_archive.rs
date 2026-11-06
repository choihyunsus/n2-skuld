use n2_skuld::{
    database::{initialize_database, Scope},
    semantic_archive::{initialize_semantic_edges, prune_semantic_edges},
};
use rusqlite::Connection;

#[test]
fn keeps_top_edges_per_source_with_deterministic_ties_and_owner_isolation() {
    let root = tempfile::tempdir().unwrap();
    let cold = root.path().join("archive.db");
    let mut db = initialize_database(&root.path().join("hot.db")).unwrap();
    initialize_semantic_edges(&mut db).unwrap();
    for agent in ["worker", "other"] {
        for target in ["a", "b", "c", "d"] {
            db.execute("INSERT INTO verdandi_semantic_edges(project,agent,source_path,target_path,similarity,model,computed_at)
                VALUES('demo',?1,'source',?2,0.8,'model',100)",[agent,target]).unwrap();
        }
    }
    let scope = Scope {
        project: "demo".into(),
        agent: "worker".into(),
    };
    assert_eq!(
        prune_semantic_edges(&mut db, &cold, &scope, 2, 100)
            .unwrap()
            .moved,
        2
    );
    let targets:Vec<String>=db.prepare("SELECT target_path FROM verdandi_semantic_edges WHERE agent='worker' ORDER BY target_path").unwrap()
        .query_map([],|row|row.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    assert_eq!(targets, vec!["a", "b"]);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM verdandi_semantic_edges WHERE agent='other'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
    let archive = Connection::open(&cold).unwrap();
    assert_eq!(
        archive
            .query_row(
                "SELECT count(*) FROM verdandi_semantic_edges WHERE model='model'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(
        prune_semantic_edges(&mut db, &cold, &scope, 2, 100)
            .unwrap()
            .moved,
        0
    );
}

#[test]
fn rejects_legacy_unscoped_tables_without_modifying_them() {
    let root = tempfile::tempdir().unwrap();
    let mut db = initialize_database(&root.path().join("hot.db")).unwrap();
    db.execute_batch("CREATE TABLE verdandi_semantic_edges(source_path TEXT,target_path TEXT,similarity REAL,computed_at INTEGER);
        INSERT INTO verdandi_semantic_edges VALUES('a','b',0.8,1);").unwrap();
    assert!(initialize_semantic_edges(&mut db).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM verdandi_semantic_edges", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
}
