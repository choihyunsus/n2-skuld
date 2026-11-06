use std::process::Command;

#[test]
fn executable_scans_saves_publishes_and_reports_partial_failure() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("a.rs"), "// TODO implement").unwrap();
    std::fs::write(source.join("b.ts"), "// FIXME second").unwrap();
    let database = root.path().join("shared.db");
    let pages = root.path().join("pages");
    let output = Command::new(env!("CARGO_BIN_EXE_n2-skuld"))
        .arg("scan")
        .arg(&source)
        .args(["--project", "demo", "--agent", "worker", "--db"])
        .arg(&database)
        .arg("--pages")
        .arg(&pages)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["report"]["findings"].as_array().unwrap().len(), 2);
    assert!(json["scan_id"].is_string());
    let output = Command::new(env!("CARGO_BIN_EXE_n2-skuld"))
        .arg("page")
        .arg(&pages)
        .arg("todo")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("implement"));
    let output = Command::new(env!("CARGO_BIN_EXE_n2-skuld"))
        .arg("scan")
        .arg(&source)
        .args(["--project", "demo", "--agent", "worker", "--max-files", "1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["report"]["complete"], false);
}

#[test]
fn read_command_does_not_create_missing_database() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("absent.db");
    let output = Command::new(env!("CARGO_BIN_EXE_n2-skuld"))
        .args(["recall", "--db"])
        .arg(&path)
        .args([
            "--project",
            "demo",
            "--agent",
            "worker",
            "--query",
            "keyword",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!path.exists());
}
