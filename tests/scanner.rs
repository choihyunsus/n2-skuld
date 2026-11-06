use n2_skuld::scanner::{scan_directory, FindingKind, ScanLimits};
use std::fs;

#[test]
fn scans_all_marker_kinds_without_string_literal_false_positives() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("sample.rs"),
        r#"
// TODO implement
// FIXME boundary
// XXX debt
// HACK workaround
// BUG state
fn f() {
 let text = "// TODO ignored";
 let raw = r"// FIXME ignored";
 todo!();
 unimplemented!();
 panic!("not yet ready");
}
"#,
    )
    .unwrap();
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    assert!(report.complete, "{:?}", report.issues);
    assert_eq!(report.findings.len(), 8);
    assert!(report
        .findings
        .iter()
        .any(|item| item.kind == FindingKind::PanicNotYet));
    assert!(report.findings.iter().all(|item| item.path == "sample.rs"));
}

#[test]
fn all_supported_language_grammars_read_real_comments() {
    let root = tempfile::tempdir().unwrap();
    for extension in ["rs", "ts", "tsx", "js", "jsx", "go", "java", "c", "cpp"] {
        fs::write(
            root.path().join(format!("sample.{extension}")),
            "// TODO implement\n",
        )
        .unwrap();
    }
    for extension in ["py", "sh"] {
        fs::write(
            root.path().join(format!("sample.{extension}")),
            "# TODO implement\n",
        )
        .unwrap();
    }
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    assert!(report.complete, "{:?}", report.issues);
    assert_eq!(report.scanned_files, 11);
    assert_eq!(report.findings.len(), 11);
}

#[test]
fn scan_limits_and_malformed_sources_report_partial_work() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.rs"), "// TODO a\nfn broken( {").unwrap();
    fs::write(root.path().join("b.rs"), "// TODO b").unwrap();
    let limits = ScanLimits {
        max_files: 1,
        ..Default::default()
    };
    let report = scan_directory(root.path(), &limits).unwrap();
    assert!(!report.complete);
    assert_eq!(report.visited_files, 1);
    assert_eq!(report.findings.len(), 1);
    assert!(report.issues.len() >= 2);
    let limits = ScanLimits {
        max_bytes: 4,
        ..Default::default()
    };
    assert!(!scan_directory(root.path(), &limits).unwrap().complete);
}

#[test]
fn ignored_build_and_hidden_directories_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["target", "node_modules", ".git"] {
        fs::create_dir(root.path().join(directory)).unwrap();
        fs::write(
            root.path().join(directory).join("fake.rs"),
            "// TODO ignored",
        )
        .unwrap();
    }
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    assert!(report.complete);
    assert_eq!(report.visited_files, 0);
}

#[test]
fn oversized_sources_and_dense_comments_respect_memory_budgets() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("dense.rs"),
        format!("/*\n{}*/", " * TODO implement\n".repeat(1000)),
    )
    .unwrap();
    let limits = ScanLimits {
        max_findings: 3,
        ..Default::default()
    };
    let report = scan_directory(root.path(), &limits).unwrap();
    assert_eq!(report.findings.len(), 3);
    assert!(!report.complete);
    std::fs::write(root.path().join("dense.rs"), vec![b'a'; 1_048_577]).unwrap();
    let report = scan_directory(root.path(), &ScanLimits::default()).unwrap();
    assert!(!report.complete);
    assert_eq!(report.scanned_files, 0);
}
