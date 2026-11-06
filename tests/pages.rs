use n2_skuld::{
    pages::{publish_pages, read_manifest, read_page},
    scanner::{scan_directory, ScanLimits},
};

#[test]
fn publishes_consistent_generations_and_preserves_unrelated_notes() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("pages");
    std::fs::write(root.path().join("test.rs"), "// TODO implement").unwrap();
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    publish_pages(&output, &report).unwrap();
    assert!(read_page(&output, "todo").unwrap().contains("implement"));
    std::fs::write(output.join("notes.md"), "user notes").unwrap();
    let mut empty = report.clone();
    empty.findings.clear();
    empty.complete = false;
    let manifest = publish_pages(&output, &empty).unwrap();
    assert!(!manifest.complete);
    assert!(read_page(&output, "todo").is_err());
    assert!(read_page(&output, "_index")
        .unwrap()
        .contains("Partial scan"));
    assert_eq!(
        std::fs::read_to_string(output.join("notes.md")).unwrap(),
        "user notes"
    );
    assert!(read_page(&output, "../notes").is_err());
}

#[test]
fn corrupt_manifest_and_foreign_output_are_preserved_on_error() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("pages");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("current.json"), "user record").unwrap();
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    assert!(publish_pages(&output, &report).is_err());
    assert!(read_manifest(&output).is_err());
    assert_eq!(
        std::fs::read_to_string(output.join("current.json")).unwrap(),
        "user record"
    );
}

#[test]
fn concurrent_publisher_lock_preserves_the_current_generation() {
    use fs2::FileExt;
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("pages");
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    let original = publish_pages(&output, &report).unwrap();
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .open(output.join(".publish.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    assert!(publish_pages(&output, &report).is_err());
    assert_eq!(
        read_manifest(&output).unwrap().generation,
        original.generation
    );
}
