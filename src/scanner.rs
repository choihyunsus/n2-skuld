//! Bounded directory traversal and explicit partial-scan diagnostics.
use crate::{syntax, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use std::{fs, io::Read, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    Todo,
    Fixme,
    Xxx,
    Hack,
    Bug,
    UnimplementedMacro,
    TodoMacro,
    PanicNotYet,
}

impl FindingKind {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Fixme => "fixme",
            Self::Xxx => "xxx",
            Self::Hack => "hack",
            Self::Bug => "bug",
            Self::UnimplementedMacro => "unimplemented",
            Self::TodoMacro => "todo-macro",
            Self::PanicNotYet => "panic-not-yet",
        }
    }
    pub(crate) fn from_marker(marker: &str) -> Option<Self> {
        match marker.to_ascii_uppercase().as_str() {
            "TODO" => Some(Self::Todo),
            "FIXME" => Some(Self::Fixme),
            "XXX" => Some(Self::Xxx),
            "HACK" => Some(Self::Hack),
            "BUG" => Some(Self::Bug),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub path: String,
    pub line: usize,
    pub snippet: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanIssue {
    pub path: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub root: String,
    pub complete: bool,
    pub scanned_files: usize,
    pub visited_files: usize,
    pub read_bytes: u64,
    pub findings: Vec<Finding>,
    pub issues: Vec<ScanIssue>,
}
#[derive(Debug, Clone)]
pub struct ScanLimits {
    pub max_files: usize,
    pub max_bytes: u64,
    pub max_depth: usize,
    pub max_findings: usize,
    pub max_duration: Duration,
}
impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_files: 5000,
            max_bytes: 64 * MIB,
            max_depth: 64,
            max_findings: 10000,
            max_duration: Duration::from_secs(30),
        }
    }
}
const MIB: u64 = 1_048_576;
const MAX_FILE_BYTES: u64 = MIB;
const MAX_ENTRIES: usize = 50000;
const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    "venv",
    "__pycache__",
    "out",
    "coverage",
];

pub fn scan_directory(root: &Path, limits: &ScanLimits) -> Result<ScanReport> {
    if limits.max_files == 0
        || limits.max_bytes == 0
        || limits.max_depth == 0
        || limits.max_findings == 0
        || limits.max_duration.is_zero()
    {
        return Err("Scan limits must be positive".into());
    }
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err("Scan root must be a directory".into());
    }
    let mut report = ScanReport {
        root: root.display().to_string(),
        complete: true,
        scanned_files: 0,
        visited_files: 0,
        read_bytes: 0,
        findings: Vec::new(),
        issues: Vec::new(),
    };
    let mut entries = 0;
    let deadline = Instant::now()
        .checked_add(limits.max_duration)
        .ok_or("Invalid scan duration")?;
    walk(&root, &root, 0, limits, &mut report, &mut entries, deadline)?;
    report.findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.kind.cmp(&right.kind))
    });
    Ok(report)
}

fn walk(
    root: &Path,
    directory: &Path,
    depth: usize,
    limits: &ScanLimits,
    report: &mut ScanReport,
    visited: &mut usize,
    deadline: Instant,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)?
        .take(MAX_ENTRIES + 1)
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *visited += 1;
        if *visited > MAX_ENTRIES
            || report.visited_files >= limits.max_files
            || report.findings.len() >= limits.max_findings
            || report.read_bytes >= limits.max_bytes
            || Instant::now() >= deadline
        {
            issue(report, directory, "Scan limit reached");
            return Ok(());
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if is_link(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if depth >= limits.max_depth {
                issue(report, &path, "Directory depth limit reached");
                continue;
            }
            if let Err(error) = walk(root, &path, depth + 1, limits, report, visited, deadline) {
                issue(report, &path, &error.to_string());
            }
        } else if metadata.is_file() && syntax::language(&path).is_some() {
            if let Err(error) = scan_file(root, &path, limits, report, deadline) {
                issue(report, &path, &error.to_string());
            }
        }
    }
    Ok(())
}

fn scan_file(
    root: &Path,
    path: &Path,
    limits: &ScanLimits,
    report: &mut ScanReport,
    deadline: Instant,
) -> Result<()> {
    report.visited_files += 1;
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(root) {
        return Err("Source resolves outside scan root".into());
    }
    let mut bytes = Vec::new();
    let byte_limit = MAX_FILE_BYTES.min(limits.max_bytes.saturating_sub(report.read_bytes));
    fs::File::open(path)?
        .take(byte_limit + 1)
        .read_to_end(&mut bytes)?;
    report.read_bytes += bytes.len() as u64;
    if bytes.len() as u64 > byte_limit {
        return Err("Source byte limit reached".into());
    }
    let source = String::from_utf8(bytes)?;
    let relative = path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/");
    let (mut findings, syntax_errors) = syntax::extract(
        &relative,
        &source,
        syntax::language(path).ok_or("Unsupported language")?,
        deadline,
        limits.max_findings.saturating_sub(report.findings.len()),
    )?;
    if syntax_errors {
        issue(
            report,
            path,
            "Source has syntax errors; findings may be partial",
        );
    }
    report.scanned_files += 1;
    let available = limits.max_findings.saturating_sub(report.findings.len());
    if findings.len() > available {
        findings.truncate(available);
        issue(report, path, "Finding limit reached");
    }
    report.findings.extend(findings);
    Ok(())
}

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}
fn issue(report: &mut ScanReport, path: &Path, message: &str) {
    report.complete = false;
    report.issues.push(ScanIssue {
        path: path.display().to_string(),
        message: message.to_string(),
    });
}
