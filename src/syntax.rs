//! Parser-backed debt markers avoid treating source string literals as comments.
use crate::scanner::{Finding, FindingKind};
use crate::Result;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tree_sitter::{Language, Node, ParseOptions, ParseState, Parser};

static MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?://|#|/\*|\*)\s*(TODO|FIXME|XXX|HACK|BUG)\b").expect("marker pattern")
});
static PANIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)panic\s*!\s*\(\s*"(?:not\s+yet|unimplemented|TODO|FIXME)"#)
        .expect("panic pattern")
});

pub fn language(path: &Path) -> Option<Language> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let grammar = match extension.as_str() {
        "rs" => tree_sitter_rust::LANGUAGE,
        "ts" | "mts" | "cts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX,
        "js" | "jsx" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE,
        "py" | "pyi" => tree_sitter_python::LANGUAGE,
        "go" => tree_sitter_go::LANGUAGE,
        "java" => tree_sitter_java::LANGUAGE,
        "c" | "h" => tree_sitter_c::LANGUAGE,
        "cc" | "cpp" | "hpp" => tree_sitter_cpp::LANGUAGE,
        "sh" | "bash" | "zsh" => tree_sitter_bash::LANGUAGE,
        _ => return None,
    };
    Some(grammar.into())
}

pub fn extract(
    path: &str,
    source: &str,
    grammar: Language,
    deadline: Instant,
    max_findings: usize,
) -> Result<(Vec<Finding>, bool)> {
    let mut parser = Parser::new();
    parser.set_language(&grammar)?;
    let deadline = deadline.min(Instant::now() + Duration::from_secs(1));
    let mut cancelled = |_: &ParseState| Instant::now() >= deadline;
    let options = ParseOptions::new().progress_callback(&mut cancelled);
    let tree = parser
        .parse_with_options(
            &mut |offset, _| source.as_bytes().get(offset..).unwrap_or(&[]),
            None,
            Some(options),
        )
        .ok_or("Source parse time budget exceeded")?;
    let mut findings = Vec::new();
    let mut cursor = tree.walk();
    loop {
        if Instant::now() >= deadline {
            return Err("Source traversal time budget exceeded".into());
        }
        inspect(cursor.node(), path, source, &mut findings, max_findings)?;
        if findings.len() > max_findings {
            return Ok((findings, tree.root_node().has_error()));
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                findings.sort_by(|left, right| {
                    left.line.cmp(&right.line).then(left.kind.cmp(&right.kind))
                });
                findings.dedup_by(|left, right| left.line == right.line && left.kind == right.kind);
                return Ok((findings, tree.root_node().has_error()));
            }
        }
    }
}

fn inspect(
    node: Node<'_>,
    path: &str,
    source: &str,
    findings: &mut Vec<Finding>,
    max_findings: usize,
) -> Result<()> {
    if node.kind().contains("comment") {
        for (offset, line) in node.utf8_text(source.as_bytes())?.lines().enumerate() {
            if findings.len() > max_findings {
                break;
            }
            if let Some(marker) = MARKER.captures(line).and_then(|captures| captures.get(1)) {
                if let Some(kind) = FindingKind::from_marker(marker.as_str()) {
                    findings.push(finding(
                        kind,
                        path,
                        node.start_position().row + offset + 1,
                        line,
                    ));
                }
            }
        }
    }
    if node.kind() == "macro_invocation" {
        let name = node
            .child_by_field_name("macro")
            .and_then(|name| name.utf8_text(source.as_bytes()).ok())
            .unwrap_or("");
        let text = node.utf8_text(source.as_bytes())?;
        let kind = match name.rsplit("::").next().unwrap_or("") {
            "todo" => Some(FindingKind::TodoMacro),
            "unimplemented" => Some(FindingKind::UnimplementedMacro),
            "panic" if PANIC.is_match(text) => Some(FindingKind::PanicNotYet),
            _ => None,
        };
        if let Some(kind) = kind {
            findings.push(finding(kind, path, node.start_position().row + 1, text));
        }
    }
    Ok(())
}

fn finding(kind: FindingKind, path: &str, line: usize, text: &str) -> Finding {
    Finding {
        kind,
        path: path.to_string(),
        line,
        snippet: text
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(200)
            .collect(),
    }
}
