mod config;
mod engine;

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use config::DocqConfig;

use clap::{Parser, Subcommand};
use docq_core::{EngineStatus, Storage, Verbose};
use docq_model::BGE_SMALL_ZH_V1_5_DIMENSION;
use docq_storage::SqliteStorage;
use serde::Serialize;

pub use engine::{Engine, EngineComponents, EngineConfig};

#[derive(Parser)]
#[command(
  name = "docq",
  version,
  about = "Local-first RAG: hybrid search and cited answers over your documents (downloads models on first use)"
)]
struct Cli {
  /// Workspace directory path (default: ~/.config/docq on Unix, %LOCALAPPDATA%\docq on Windows).
  #[arg(long, global = true)]
  workspace: Option<PathBuf>,

  /// Model cache directory (default: ~/.cache/docq/models).
  #[arg(long, global = true)]
  model_cache: Option<PathBuf>,

  /// Path to a custom configuration file (default: ~/.config/docq/config.toml).
  #[arg(short = 'c', long, global = true)]
  config: Option<PathBuf>,

  /// Enable verbose progress output (use -vv for even more detail).
  #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
  verbose: u8,

  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Initialize a new workspace.
  Init,
  /// Add a collection (a directory to be indexed).
  Add {
    /// Directory path to index.
    path: PathBuf,
    /// Name for this collection.
    #[arg(long)]
    name: String,
  },
  /// Build or update the index.
  Index {
    /// Only index the specified collection.
    #[arg(long)]
    collection: Option<String>,
  },
  /// Search for passages (zero LLM cost).
  Search {
    /// Query string.
    query: String,
    /// Number of results to return.
    #[arg(long, default_value_t = 5)]
    top_k: usize,
    /// Show score breakdown.
    #[arg(long)]
    explain: bool,
    /// Output JSON to stdout.
    #[arg(long)]
    json: bool,
  },
  /// Ask a question and get a cited answer.
  Ask {
    /// Question string.
    query: String,
    /// Output JSON to stdout.
    #[arg(long)]
    json: bool,
  },
  /// Show workspace status.
  Status {
    /// Output JSON to stdout.
    #[arg(long)]
    json: bool,
  },
}

fn default_workspace() -> PathBuf {
  // Use XDG-style config directories on Unix and the standard local app data
  // directory on Windows. This keeps the workspace (config.toml + docq.db) in
  // the conventional per-platform config location.
  #[cfg(target_os = "macos")]
  {
    dirs::home_dir().unwrap_or_default().join(".config").join("docq")
  }
  #[cfg(target_os = "linux")]
  {
    dirs::config_dir().unwrap_or_default().join("docq")
  }
  #[cfg(target_os = "windows")]
  {
    dirs::config_local_dir().unwrap_or_default().join("docq")
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    dirs::home_dir().unwrap_or_default().join(".docq")
  }
}

/// Default directory for the global `config.toml`.
/// This is independent from the workspace (data) directory: configuration is
/// always loaded from here, even when `--workspace` points somewhere else.
fn default_config_dir() -> PathBuf {
  // Keep the global config in the conventional per-platform config location.
  #[cfg(target_os = "macos")]
  {
    dirs::home_dir().unwrap_or_default().join(".config").join("docq")
  }
  #[cfg(target_os = "linux")]
  {
    dirs::config_dir().unwrap_or_default().join("docq")
  }
  #[cfg(target_os = "windows")]
  {
    dirs::config_local_dir().unwrap_or_default().join("docq")
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    dirs::home_dir().unwrap_or_default().join(".docq")
  }
}

fn default_model_cache() -> PathBuf {
  dirs::home_dir().unwrap_or_default().join(".cache").join("docq").join("models")
}

#[derive(Serialize)]
struct ErrorResponse {
  error: String,
}

fn print_json<T: Serialize>(value: &T) {
  println!(
    "{}",
    serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("{{\"error\": \"json: {e}\"}}"))
  );
}

fn print_error_json(msg: &str) {
  eprintln!(
    "{}",
    serde_json::to_string(&ErrorResponse { error: msg.to_string() }).unwrap_or_default()
  );
}

#[tokio::main]
async fn main() {
  let cli = Cli::parse();

  let workspace = cli.workspace.unwrap_or_else(default_workspace);
  let model_cache = cli.model_cache.unwrap_or_else(default_model_cache);
  let config_result = match cli.config {
    Some(ref path) => DocqConfig::load_from_file(path),
    None => ensure_config(),
  };
  let config = match config_result {
    Ok(cfg) => cfg,
    Err(e) => {
      print_error_json(&e.to_string());
      process::exit(1);
    }
  };
  let verbose = Verbose(cli.verbose > 0);

  if let Err(e) = run_command(&cli.command, &workspace, &model_cache, config, verbose).await {
    print_error_json(&e.to_string());
    process::exit(1);
  }
}

/// Ensure the global config directory and `config.toml` exist.
/// If the config file is missing, write a default one and return it.
fn ensure_config() -> anyhow::Result<DocqConfig> {
  let config_dir = default_config_dir();
  fs::create_dir_all(&config_dir)?;
  let config_path = DocqConfig::path(&config_dir);
  if config_path.exists() {
    DocqConfig::load(&config_dir)
  } else {
    let cfg = DocqConfig::default();
    fs::write(&config_path, cfg.to_toml()?)
      .map_err(|e| anyhow::anyhow!("write default config {}: {e}", config_path.display()))?;
    Ok(cfg)
  }
}

async fn run_command(
  cmd: &Commands,
  workspace: &Path,
  model_cache: &Path,
  config: DocqConfig,
  verbose: Verbose,
) -> anyhow::Result<()> {
  // Make sure the workspace (data) directory exists for commands that need storage.
  fs::create_dir_all(workspace)?;

  match cmd {
    Commands::Init => {
      fs::create_dir_all(workspace)?;
      let storage = SqliteStorage::open_workspace(workspace)?;
      storage.init(BGE_SMALL_ZH_V1_5_DIMENSION)?;
      println!("Initialized workspace at {}", workspace.display());
    }

    Commands::Add { path, name } => {
      // add only needs storage, not models — avoid Engine::open to skip model downloads
      let storage = open_storage(workspace)?;
      let canonical = fs::canonicalize(path)?;
      let path_str = canonical.to_string_lossy().to_string();
      let mut tx = storage.begin_tx()?;
      tx.add_collection(name, &path_str)?;
      tx.commit()?;
      println!("Added collection '{}' -> {}", name, canonical.display());
    }

    Commands::Status { json } => {
      // status only reads metadata, not models
      let storage = open_storage(workspace)?;
      let docs = storage.list_documents()?;
      let collections = storage.list_collections()?;
      let chunks = storage.count_chunks()?;
      let status = EngineStatus {
        documents: docs.len(),
        chunks,
        collections,
      };
      if *json {
        print_json(&status);
      } else {
        println!("Workspace: {}", workspace.display());
        println!("Documents: {}", status.documents);
        println!("Chunks: {}", status.chunks);
        println!("Collections: {}", status.collections.len());
        for c in &status.collections {
          println!("  {} -> {}", c.name, c.path.display());
        }
      }
    }

    Commands::Index { collection } => {
      let engine = Engine::open_for_index(engine_config(workspace, model_cache, config.clone(), verbose)).await?;
      let stats = if let Some(name) = collection {
        engine.index_one(name).await?
      } else {
        engine.index().await?
      };
      println!(
        "Indexed {} files ({} chunks, {} skipped)",
        stats.files_indexed, stats.chunks_indexed, stats.files_skipped
      );
    }

    Commands::Search {
      query,
      top_k,
      explain,
      json,
    } => {
      let engine = Engine::open_for_search(engine_config(workspace, model_cache, config.clone(), verbose)).await?;
      let hits = engine.search(query, *top_k).await?;

      if *json {
        let output: Vec<serde_json::Value> = hits
          .iter()
          .map(|h| {
            if *explain {
              serde_json::to_value(h).unwrap_or_default()
            } else {
              serde_json::json!({
                "chunk": h.chunk,
                "score": h.score,
              })
            }
          })
          .collect();
        print_json(&serde_json::json!({ "hits": output }));
      } else {
        for (i, hit) in hits.iter().enumerate() {
          println!("[{}] {:.4} {}", i + 1, hit.score, hit.chunk.text.trim());
          if *explain {
            let e = &hit.explain;
            println!(
              "    bm25={:?} vec={:?} rrf={:?} rerank={:?}",
              e.bm25_score, e.vector_score, e.rrf_score, e.rerank_score
            );
          }
        }
        if hits.is_empty() {
          println!("No results found.");
        }
      }
    }

    Commands::Ask { query, json } => {
      let engine = Engine::open_for_ask(engine_config(workspace, model_cache, config.clone(), verbose)).await?;
      let answer = engine.ask(query).await?;

      if *json {
        print_json(&answer);
      } else {
        println!("{}", answer.text);
        if !answer.citations.is_empty() {
          println!("\nSources:");
          for c in &answer.citations {
            println!("  {} {}", c.marker, c.source);
          }
        }
      }
    }
  }
  Ok(())
}

fn open_storage(workspace: &Path) -> anyhow::Result<SqliteStorage> {
  let storage = SqliteStorage::open_workspace(workspace)?;
  storage.init(BGE_SMALL_ZH_V1_5_DIMENSION)?;
  Ok(storage)
}

fn engine_config(workspace: &Path, model_cache: &Path, config: DocqConfig, verbose: Verbose) -> EngineConfig {
  EngineConfig {
    workspace_path: workspace.to_path_buf(),
    model_cache_dir: model_cache.to_path_buf(),
    config,
    verbose,
  }
}
