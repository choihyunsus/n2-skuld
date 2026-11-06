//! Immutable Markdown generations published through one atomic manifest replacement.
use crate::{
    scanner::{is_link, FindingKind, ScanReport},
    Result,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const FORMAT: &str = "n2-skuld-pages/v1";
const CURRENT: &str = "current.json";
const MAX_PAGE_BYTES: usize = 16_777_216;

#[derive(Debug, Serialize, Deserialize)]
pub struct PageManifest {
    pub format: String,
    pub generation: String,
    pub complete: bool,
    pub pages: Vec<String>,
}

pub fn publish_pages(directory: &Path, report: &ScanReport) -> Result<PageManifest> {
    validate_directory(directory)?;
    fs::create_dir_all(directory)?;
    let lock_path = safe_file(directory, ".publish.lock")?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    lock.try_lock_exclusive()?;
    let manifest_path = safe_file(directory, CURRENT)?;
    if manifest_path.exists() {
        read_manifest(directory)?;
    }
    let pages = render_pages(report);
    if pages.values().any(|content| content.len() > MAX_PAGE_BYTES) {
        return Err("Generated page exceeds 16 MiB".into());
    }
    let generation = uuid::Uuid::new_v4().to_string();
    let generation_dir = directory.join(&generation);
    fs::create_dir(&generation_dir)?;
    for (slug, content) in &pages {
        write_new(
            &generation_dir.join(format!("{slug}.md")),
            content.as_bytes(),
        )?;
    }
    sync_directory(&generation_dir)?;
    let manifest = PageManifest {
        format: FORMAT.into(),
        generation,
        complete: report.complete,
        pages: pages.keys().cloned().collect(),
    };
    let pending = directory.join(format!(".manifest-{}.tmp", uuid::Uuid::new_v4()));
    write_new(&pending, &serde_json::to_vec_pretty(&manifest)?)?;
    fs::rename(&pending, manifest_path)?;
    sync_directory(directory)?;
    Ok(manifest)
}

pub fn read_manifest(directory: &Path) -> Result<PageManifest> {
    validate_directory(directory)?;
    let path = safe_file(directory, CURRENT)?;
    if fs::metadata(&path)?.len() > 4096 {
        return Err("Page manifest exceeds 4096 bytes".into());
    }
    let manifest: PageManifest = serde_json::from_slice(&fs::read(path)?)?;
    if manifest.format != FORMAT
        || uuid::Uuid::parse_str(&manifest.generation).is_err()
        || manifest.pages.len() > 9
        || !manifest.pages.iter().all(|slug| valid_slug(slug))
    {
        return Err("Unsupported or invalid Skuld page manifest".into());
    }
    let generation = directory.join(&manifest.generation);
    validate_directory(&generation)?;
    if !generation.is_dir() {
        return Err("Skuld page generation is missing".into());
    }
    Ok(manifest)
}

pub fn read_page(directory: &Path, slug: &str) -> Result<String> {
    if !valid_slug(slug) {
        return Err("Invalid Skuld page slug".into());
    }
    let manifest = read_manifest(directory)?;
    if !manifest.pages.iter().any(|page| page == slug) {
        return Err("Page not found in current generation".into());
    }
    let path = safe_file(&directory.join(manifest.generation), &format!("{slug}.md"))?;
    if fs::metadata(&path)?.len() > MAX_PAGE_BYTES as u64 {
        return Err("Generated page exceeds 16 MiB".into());
    }
    Ok(fs::read_to_string(path)?)
}

fn valid_slug(slug: &str) -> bool {
    [
        "_index",
        "todo",
        "fixme",
        "xxx",
        "hack",
        "bug",
        "unimplemented",
        "todo-macro",
        "panic-not-yet",
    ]
    .contains(&slug)
}

fn validate_directory(directory: &Path) -> Result<()> {
    if directory
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Page directory cannot contain parent traversal".into());
    }
    for path in directory.ancestors() {
        match fs::symlink_metadata(path) {
            Ok(metadata) if is_link(&metadata) => {
                return Err("Page directory cannot contain links or reparse points".into())
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("Page parent must be a directory".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn safe_file(directory: &Path, name: &str) -> Result<PathBuf> {
    let path = directory.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if is_link(&metadata) || !metadata.is_file() => {
            return Err("Expected a regular Skuld output file".into())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(path)
}

fn write_new(path: &Path, content: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(content)?;
    file.sync_all()?;
    Ok(())
}

fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(directory)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

fn render_pages(report: &ScanReport) -> BTreeMap<String, String> {
    let mut groups: BTreeMap<FindingKind, Vec<_>> = BTreeMap::new();
    for finding in &report.findings {
        groups.entry(finding.kind).or_default().push(finding);
    }
    let mut pages = BTreeMap::new();
    let status = if report.complete {
        "Complete scan"
    } else {
        "Partial scan"
    };
    let mut index = format!(
        "# Skuld\n\n{status}. {} findings across {} scanned files.\n\n",
        report.findings.len(),
        report.scanned_files
    );
    for (kind, findings) in groups {
        let slug = kind.slug();
        let mut content = format!("# {slug}\n\n{status}. {} findings.\n\n", findings.len());
        for finding in &findings {
            content.push_str(&format!(
                "- `{}` line {} — `{}`\n",
                escape(&finding.path),
                finding.line,
                escape(&finding.snippet)
            ));
        }
        pages.insert(slug.into(), content);
        index.push_str(&format!(
            "- [{slug}]({slug}.md) — {} findings\n",
            findings.len()
        ));
    }
    for issue in &report.issues {
        index.push_str(&format!(
            "\n- `{}` — {}\n",
            escape(&issue.path),
            escape(&issue.message)
        ));
    }
    pages.insert("_index".into(), index);
    pages
}

fn escape(value: &str) -> String {
    value
        .replace('`', "'")
        .replace(['\r', '\n'], " ")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
