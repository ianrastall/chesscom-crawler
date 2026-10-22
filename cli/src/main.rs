//! Command-line shell over the crawler core. Shares its data folder with the desktop app.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use chesscom_crawler_core::fide::{FideCatalog, FideIndex, ProfileFacts, Resolution};
use chesscom_crawler_core::settings::Settings;
use chesscom_crawler_core::status::{Level, RunState};
use chesscom_crawler_core::store::Store;
use chesscom_crawler_core::{Core, CoreConfig};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chesscom-crawler", version, about = "Collects Chess.com games into FIDE-rated PGN collections.")]
struct Cli {
    /// Data folder holding settings.json and state.sqlite.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the crawl in the foreground until it completes (Ctrl+C stops it; progress is kept).
    Run,
    /// Show collection and identification counts.
    Status,
    /// Print the settings as JSON, or replace them from a JSON file.
    Settings {
        #[arg(long)]
        import: Option<PathBuf>,
    },
    /// Show how a Chess.com profile name resolves against the newest FIDE list (no requests).
    Resolve {
        name: String,
        #[arg(long, default_value = "")]
        title: String,
        /// Chess.com country code, such as US or XE.
        #[arg(long, default_value = "")]
        country: String,
    },
    /// Map an account to a FIDE ID, block it from resolving, or clear the override.
    Override {
        #[command(subcommand)]
        action: OverrideAction,
    },
    /// Rewrite every collection file from collected games under the current settings.
    Export,
    /// Forget month progress so months are crawled again. Collected games are kept.
    ResetProgress,
}

#[derive(Subcommand)]
enum OverrideAction {
    Set { username: String, fide_id: i64 },
    Block { username: String },
    Clear { username: String },
    List,
}

fn default_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Jupiter").join("ChesscomCrawler")
}

fn default_output_dir() -> PathBuf {
    let base = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Documents").join("Chess.com collections")
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.unwrap_or_else(default_data_dir);
    match run(cli.command, data_dir) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn open_core(data_dir: PathBuf) -> Result<Core, String> {
    Core::open(CoreConfig { data_dir, default_output_dir: default_output_dir() }).map_err(|e| e.to_string())
}

fn print_events(core: &Core) {
    for e in core.poll_events(1000) {
        let tag = match e.level {
            Level::Info => "",
            Level::Warning => "warning: ",
            Level::Error => "error: ",
        };
        eprintln!("{tag}{}", e.message);
    }
}

fn run(command: Command, data_dir: PathBuf) -> Result<ExitCode, String> {
    match command {
        Command::Run => {
            let core = open_core(data_dir)?;
            core.start().map_err(|e| e.to_string())?;
            let mut last_phase = String::new();
            loop {
                std::thread::sleep(Duration::from_millis(500));
                print_events(&core);
                let s = core.status();
                if s.phase != last_phase {
                    eprintln!("[{}] {}", s.month.map(|m| m.to_string()).unwrap_or_default(), s.phase);
                    last_phase = s.phase.clone();
                }
                if !s.state.is_active() {
                    print_events(&core);
                    return Ok(if s.state == RunState::Failed { ExitCode::FAILURE } else { ExitCode::SUCCESS });
                }
            }
        }
        Command::Status => {
            let core = open_core(data_dir)?;
            let s = core.status();
            println!("FIDE lists: {} to {}", opt(s.fide_first), opt(s.fide_last));
            let store = Store::open(&core.state_path()).map_err(|e| e.to_string())?;
            let source = core.settings().rating_source.code();
            println!(
                "Games collected ({source} ratings): {}",
                store.game_count(None, source).map_err(|e| e.to_string())?
            );
            println!("Accounts:");
            for (code, n) in store.resolution_counts().map_err(|e| e.to_string())? {
                println!("  {code}: {n}");
            }
            for m in store.months_with_games(source).map_err(|e| e.to_string())?.iter().rev().take(24) {
                println!("  {m}: {} games", store.game_count(Some(*m), source).map_err(|e| e.to_string())?);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Settings { import } => {
            let core = open_core(data_dir)?;
            if let Some(path) = import {
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let settings: Settings = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                core.update_settings(settings).map_err(|e| e.to_string())?;
            }
            println!("{}", serde_json::to_string_pretty(&core.settings()).map_err(|e| e.to_string())?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Resolve { name, title, country } => {
            let core = open_core(data_dir)?;
            let (catalog, warnings) = FideCatalog::scan(&core.settings().fide_folders);
            for w in warnings {
                eprintln!("warning: {w}");
            }
            let latest = catalog.latest().ok_or("No FIDE standard lists were found.")?;
            let index = FideIndex::load(catalog.get(latest).ok_or("Catalog error")?).map_err(|e| e.to_string())?;
            match index.resolve(&ProfileFacts { name: &name, title: &title, country: &country }) {
                Resolution::Resolved(p) => {
                    println!("{} ({}, {}, {}) on the {latest} list", p.name, p.fide_id, p.fed, p.display_title())
                }
                other => println!("Not resolved: {}", other.code()),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Override { action } => {
            let core = open_core(data_dir)?;
            let store = Store::open(&core.state_path()).map_err(|e| e.to_string())?;
            let r = match action {
                OverrideAction::Set { username, fide_id } => store.set_override(&username, Some(fide_id)),
                OverrideAction::Block { username } => store.set_override(&username, None),
                OverrideAction::Clear { username } => store.clear_override(&username).map(|_| ()),
                OverrideAction::List => store.overrides().map(|list| {
                    for (user, id) in list {
                        println!("{user}\t{}", id.map(|i| i.to_string()).unwrap_or_else(|| "blocked".into()));
                    }
                }),
            };
            r.map_err(|e| e.to_string())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Export => {
            let core = open_core(data_dir)?;
            let n = core.export_all().map_err(|e| e.to_string())?;
            println!("Rewrote collections for {n} months in {}.", core.settings().output_folder);
            Ok(ExitCode::SUCCESS)
        }
        Command::ResetProgress => {
            let core = open_core(data_dir)?;
            Store::open(&core.state_path()).and_then(|s| s.reset_progress()).map_err(|e| e.to_string())?;
            println!("Month progress cleared.");
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn opt(m: Option<chesscom_crawler_core::Month>) -> String {
    m.map(|m| m.to_string()).unwrap_or_else(|| "none".into())
}
