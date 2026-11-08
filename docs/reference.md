[Back to the getting-started guide](../README.md)

[한국어](reference.ko.md)

# n2-skuld

Source-debt scanning, knowledge freshness, Markdown reports and durable archival for the shared Soul SQLite database. Skuld runs as a Rust library or a local JSON CLI.

## Install

Build with Rust 1.98 or newer and a C/C++ toolchain for the bundled SQLite and Tree-sitter grammars.

```sh
cargo install --git https://github.com/choihyunsus/n2-skuld --tag v1.0.0 --locked
n2-skuld --help
```

From a source checkout, run `cargo build --locked --release`. The executable is under `target/release/`.

## Scan source code

```sh
n2-skuld scan ./src --project demo --agent worker --db ./mimir.db --pages ./skuld-pages
n2-skuld page ./skuld-pages todo
```

`--db` and `--pages` are optional. The JSON result includes the scan report, saved report ID and page manifest when requested. Scan rows use explicit `project` and `agent` identifiers.

Supported grammars cover Rust, TypeScript/TSX, JavaScript/JSX, Python, Go, Java, C, C++ and Bash. Shell files, including `.zsh`, use the Bash grammar. Findings include TODO, FIXME, XXX, HACK and BUG comments, plus Rust `todo!()`, `unimplemented!()` and placeholder `panic!()` calls. Tree-sitter identifies comments and macro calls. The scanner skips marker-like source string literals.

Default limits are 5,000 source files, 64 MiB total reads, 1 MiB per file, 64 directory levels, 10,000 findings and 30 seconds overall. Each parser and syntax traversal has a one-second budget. Traversal also stops at 50,000 directory entries. Hidden/build directories, symlinks and Windows reparse points are skipped. Files must be UTF-8.

Syntax errors, unreadable files and reached budgets appear in `issues` and set `complete` to false. The CLI exits with 2 after printing a partial scan. Operational errors exit with 1. Resource limits count attempted reads, including oversized or invalid files.

Markdown publication writes immutable UUID generations, flushes file contents and replaces `current.json` once the complete generation is ready. Readers resolve pages through that manifest. Concurrent publication uses a filesystem lock. Partial scans remain visibly marked. Existing generations and unrelated notes are retained. The host controls retention of older generations. Directory metadata is synced on Unix, while Windows uses its filesystem rename behavior.

## Share the Soul database

Point `--db` at the same `mimir.db` used by distribution Soul and Mimir. Back up that database before maintenance. Initialization preserves the nine common tables and their field names, adds versioned Skuld metadata and rejects unsupported future schemas. A compatible Soul database without a version marker can also be opened.

```sh
n2-skuld init --db ./mimir.db
```

Common tables are `experiences`, `insights`, `tags`, `embeddings`, `effect_tracking`, `experience_insight_links`, `insight_links`, `meta` and `tag_similarity`. Skuld stores scan results in `skuld_scans`, with `project`, `agent`, `root`, `complete`, `report_json` and `created_at` fields. `meta` contains `skuld_scan_schema_version`, `skuld_database_id` and the common `schema_version`.

Experience archival preserves every common experience column, all tags and vector/model metadata. All declared foreign-key references to experiences are inspected before copying and again before deletion. Referenced evidence remains in the active database. Hosts using additional logical references must declare corresponding foreign keys for that protection to apply.

## Archive and recall

```sh
n2-skuld archive --db ./mimir.db --archive ./archive.db --project demo --agent worker --retention-days 90 --limit 1000
n2-skuld recall --db ./mimir.db --archive ./archive.db --project demo --agent worker --query "database retry" --limit 20
```

Archival is an explicit maintenance operation. It selects old observations by UTC `created_at`, commits the full archive copy, then deletes active rows only when the payload still matches and references remain absent. An interruption can leave copies in both stores. Retries preserve current content. Changed or newly referenced rows remain active. A UUID associates each archive with its source database. Same-file aliases, hard links and archives belonging to another source are rejected.

Soul and Mimir ordinarily query their active database. Use Skuld's deep recall when archived observations are needed. Recall reads the archive with a read-only connection and returns source IDs, tiers, ownership and temporal scores. Active records take precedence over older archived copies, including when the updated active content no longer matches a query.

Experience search uses literal Unicode FTS tokens. Active-insight search uses a literal case-insensitive substring, with same-owner global/agent scopes or source-validated project scope. Candidate retrieval is bounded at 200 per source and reports truncation. Relevance uses reciprocal rank within each source, multiplied by category freshness. Results represent historical material for the caller to evaluate.

The default batch holds at most 1,000 observations. SQLite 3.51.3 or newer is required, and the locked build bundles 3.54.1. It includes the WAL-reset durability fix. All programs writing the shared database should use a patched SQLite engine. Keep the active and archive databases together in backups. [SQLite fix](https://www.sqlite.org/releaselog/3_51_3.html)

## Knowledge freshness

```sh
n2-skuld freshness "2026-01-01T00:00:00Z" --category library_api
```

The score is `1 / (1 + decay_rate * age_days)`. Rates are 0.002 for architecture, 0.005 for code patterns, 0.008 for security, 0.01 for documentation/general knowledge and 0.02 for library APIs. Naive timestamps are interpreted as UTC. Future dates clamp to 1 and invalid dates produce 0. The library also provides checked SQL expressions for allowed timestamp and importance columns.

## Semantic edge maintenance

The library exposes `semantic_archive::initialize_semantic_edges` and `prune_semantic_edges`. The scoped shared table has `project`, `agent`, `source_path`, `target_path`, `similarity`, `edge_type`, `model` and `computed_at`, with a composite ownership/path key. Its metadata version is `verdandi_semantic_schema_version = 1`.

```sh
n2-skuld prune --db ./mimir.db --archive ./archive.db --project demo --agent worker --keep 10 --limit 1000
```

The command expects the scoped table to exist. It ranks each source file's edges by similarity, breaking ties by target path, and archives excess rows with copy-before-delete durability. Both rank and payload are rechecked before deletion. The older unscoped Daebung table requires an explicit ownership migration. Future Verdandi consumers should adopt this published table contract.

## Rust API

```rust,no_run
use n2_skuld::{database::{initialize_database, Scope}, scanner::{scan_directory, ScanLimits}, scan_store::save_scan};
use std::path::Path;

fn main() -> n2_skuld::Result<()> {
    let mut database = initialize_database(Path::new("mimir.db"))?;
    let scope = Scope { project: "demo".into(), agent: "worker".into() };
    let report = scan_directory(Path::new("src"), &ScanLimits::default())?;
    let id = save_scan(&mut database, &scope, &report)?;
    println!("Saved scan {id}");
    Ok(())
}
```

Use a dedicated connection for archive operations. The caller supplies scheduling, MCP/tool registration and authentication. Project and agent names identify data scopes within a trusted local store.

## Validation and license

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo package --locked
```

Tests exercise parser grammars, string-literal exclusion, budgets, partial reports, CLI execution, scoped storage, metadata preservation, copy/delete failures, changes between commits, foreign-key protection, archive pairing, temporal expressions and page publication. Integration verification also runs the CLI against a database created by Soul 9.2.1 while preserving common fields and Mimir 3.0.1 compatibility.

The release is verified on Windows with Rust 1.98. Platform-specific native builds require validation in the target environment. MIT license, with origins and dependency notices in `THIRD-PARTY-NOTICES`.
