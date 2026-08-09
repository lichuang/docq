use std::ops::Range;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
  /// File-relative path; the primary identifier.
  pub id: String,
  pub file_path: std::path::PathBuf,
  /// SHA-256 of file content; drives incremental reindex.
  pub content_hash: String,
  pub content_size: usize,
  pub indexed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
  /// SHA-256 of `text`; enables content-addressed dedup.
  pub id: String,
  /// `Document.id` this chunk belongs to.
  pub doc_id: String,
  /// Full original text actually embedded.
  pub text: String,
  pub byte_range: Range<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkCandidate {
  pub text: String,
  pub byte_range: Range<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
  pub chunk: Chunk,
  pub score: f32,
  pub explain: ScoreExplain,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreExplain {
  pub bm25_score: Option<f32>,
  pub vector_score: Option<f32>,
  pub rrf_score: Option<f32>,
  pub rerank_score: Option<f32>,
  pub final_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredChunk {
  pub chunk: Chunk,
  pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
  pub text: String,
  pub citations: Vec<Citation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
  /// Marker as printed in `text`, e.g. `[1]`.
  pub marker: String,
  /// Human-readable source, e.g. `docs/a.txt (bytes 120-512)`.
  pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
  /// `embedding` / `reranker` / `chat`.
  pub role: String,
  pub repo_id: String,
  pub filename: String,
  pub revision: String,
  pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
  pub n_ctx: u32,
  pub temperature: f32,
  pub top_p: f32,
  pub max_tokens: usize,
  pub seed: u32,
}

impl Default for LlmConfig {
  fn default() -> Self {
    Self {
      n_ctx: 4096,
      temperature: 0.7,
      top_p: 0.9,
      max_tokens: 512,
      seed: 0,
    }
  }
}
