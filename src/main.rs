//! Explicit-path JSON CLI for local scanning and shared-memory maintenance.
use clap::{Args, Parser, Subcommand};
use n2_skuld::{
    archive,
    database::{self, Scope},
    pages, recall, scan_store, scanner, temporal, Result,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Source debt scanning and Soul-compatible memory archival"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Owner {
    #[arg(long)]
    project: String,
    #[arg(long)]
    agent: String,
}
impl From<Owner> for Scope {
    fn from(owner: Owner) -> Self {
        Self {
            project: owner.project,
            agent: owner.agent,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Create or initialize the shared Soul schema.
    Init {
        #[arg(long)]
        db: PathBuf,
    },
    /// Scan supported source files and optionally store or publish the report.
    Scan {
        root: PathBuf,
        #[command(flatten)]
        owner: Owner,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        pages: Option<PathBuf>,
        #[arg(long, default_value_t = 5000)]
        max_files: usize,
    },
    /// Archive old, unreferenced observations for exactly one project and agent.
    Archive(ArchiveArgs),
    /// Recall historical material from active and optional archive storage.
    Recall(RecallArgs),
    /// Archive excess semantic edges while retaining the best links per source.
    Prune(PruneArgs),
    /// Read a generated Markdown page through the current manifest.
    Page {
        directory: PathBuf,
        #[arg(default_value = "_index")]
        slug: String,
    },
    /// Calculate UTC knowledge freshness.
    Freshness {
        timestamp: String,
        #[arg(long, default_value = "general")]
        category: String,
    },
}

#[derive(Args)]
struct ArchiveArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    archive: PathBuf,
    #[command(flatten)]
    owner: Owner,
    #[arg(long, default_value_t = 90)]
    retention_days: u32,
    #[arg(long, default_value_t = 1000)]
    limit: usize,
}
#[derive(Args)]
struct RecallArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    archive: Option<PathBuf>,
    #[command(flatten)]
    owner: Owner,
    #[arg(long)]
    query: String,
    #[arg(long, default_value = "")]
    category: String,
    #[arg(long, default_value_t = 20)]
    limit: usize,
}
#[derive(Args)]
struct PruneArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    archive: PathBuf,
    #[command(flatten)]
    owner: Owner,
    #[arg(long, default_value_t = 10)]
    keep: usize,
    #[arg(long, default_value_t = 1000)]
    limit: usize,
}

fn run_archive(args: ArchiveArgs) -> Result<()> {
    let mut connection = database::open_database(&args.db)?;
    output(&archive::archive_experiences(
        &mut connection,
        &args.archive,
        &args.owner.into(),
        args.retention_days,
        args.limit,
    )?)
}
fn run_recall(args: RecallArgs) -> Result<()> {
    let connection = database::open_database(&args.db)?;
    output(&recall::deep_recall(
        &connection,
        &args.owner.into(),
        &args.query,
        &args.category,
        args.limit,
        args.archive.as_deref(),
    )?)
}
fn run_prune(args: PruneArgs) -> Result<()> {
    let mut connection = database::open_database(&args.db)?;
    output(&n2_skuld::semantic_archive::prune_semantic_edges(
        &mut connection,
        &args.archive,
        &args.owner.into(),
        args.keep,
        args.limit,
    )?)
}

fn main() {
    match run(Cli::parse().command) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run(command: Command) -> Result<i32> {
    match command {
        Command::Init { db } => {
            database::initialize_database(&db)?;
            output(
                &serde_json::json!({"schema_version":database::SCHEMA_VERSION,"sqlite_version":rusqlite::version()}),
            )?;
        }
        Command::Scan {
            root,
            owner,
            db,
            pages,
            max_files,
        } => return scan(root, owner.into(), db, pages, max_files),
        Command::Archive(args) => run_archive(args)?,
        Command::Recall(args) => run_recall(args)?,
        Command::Prune(args) => run_prune(args)?,
        Command::Page { directory, slug } => println!("{}", pages::read_page(&directory, &slug)?),
        Command::Freshness {
            timestamp,
            category,
        } => output(
            &serde_json::json!({"freshness":temporal::freshness_weight(&timestamp,&category)}),
        )?,
    }
    Ok(0)
}

fn scan(
    root: PathBuf,
    scope: Scope,
    db: Option<PathBuf>,
    directory: Option<PathBuf>,
    max_files: usize,
) -> Result<i32> {
    scope.validate()?;
    let limits = scanner::ScanLimits {
        max_files,
        ..Default::default()
    };
    let report = scanner::scan_directory(&root, &limits)?;
    let scan_id = if let Some(path) = db {
        let mut connection = database::initialize_database(&path)?;
        Some(scan_store::save_scan(&mut connection, &scope, &report)?)
    } else {
        None
    };
    let manifest = directory
        .map(|path| pages::publish_pages(&path, &report))
        .transpose()?;
    output(&serde_json::json!({"report":report,"scan_id":scan_id,"pages":manifest}))?;
    Ok(if report.complete { 0 } else { 2 })
}

fn output(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
