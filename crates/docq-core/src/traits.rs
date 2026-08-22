//! Async boundaries:
//! - [`Storage`] is sync (SQLite is local blocking IO).
//! - [`Embedder`] / [`Reranker`] / [`Llm`] are async (heavy inference).
//!
//! [`Storage`]: crate::traits::Storage
//! [`Embedder`]: crate::traits::Embedder
//! [`Reranker`]: crate::traits::Reranker
//! [`Llm`]: crate::traits::Llm

use std::collections::HashMap;

use async_trait::async_trait;

use crate::error::Result;
use crate::models::{Chunk, ChunkCandidate, Collection, Document, DocumentSource, ModelRole, ModelSpec};

#[async_trait]
pub trait Embedder: Send + Sync {
  fn dimension(&self) -> usize;
  fn model_name(&self) -> &str;
  async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[async_trait]
pub trait Reranker: Send + Sync {
  async fn rerank(&self, query: &str, chunks: &[Chunk]) -> Result<Vec<crate::models::ScoredChunk>>;
}

#[async_trait]
pub trait Llm: Send + Sync {
  async fn complete(&self, prompt: &str) -> Result<String>;
}

pub trait Chunker: Send + Sync {
  fn chunk(&self, text: &str) -> Vec<ChunkCandidate>;
}

pub trait FileReader: Send + Sync {
  /// File extensions this reader handles, without the leading dot
  /// (e.g. `["txt", "md"]` or `["pdf"]`).
  fn extensions(&self) -> &[&str];

  /// Read a single file and return its content as a `DocumentSource`.
  /// Return `Ok(None)` to silently skip the file (e.g. empty or
  /// non-UTF-8).
  fn read(&self, path: &std::path::Path) -> Result<Option<DocumentSource>>;
}

pub trait WordSegmenter: Send + Sync {
  fn segment(&self, text: &str) -> String;
}

pub trait Storage: Send + Sync {
  /// Initialize all storage tables. Pass `0` to skip creating the vector
  /// table when no embedder is available yet.
  fn init(&self, vector_dimension: usize) -> Result<()>;

  // ---- reads / queries ----

  fn get_document(&self, doc_id: &str) -> Result<Option<Document>>;
  fn list_documents(&self) -> Result<Vec<Document>>;
  fn get_document_paths(&self, doc_ids: &[String]) -> Result<HashMap<String, String>>;
  fn get_chunks(&self, chunk_ids: &[String]) -> Result<Vec<Chunk>>;
  fn search_vectors(&self, embedding: &[f32], top_k: usize) -> Result<Vec<(String, f32)>>;
  fn search_text(&self, query: &str, top_k: usize) -> Result<Vec<(String, f32)>>;
  fn get_model_version(&self, role: ModelRole) -> Result<Option<ModelSpec>>;
  fn list_collections(&self) -> Result<Vec<Collection>>;

  // ---- counts ----

  fn count_chunks(&self) -> Result<usize>;

  // ---- transactions ----

  /// Begin a write transaction covering `documents` / `chunks` / `vec_chunks`
  /// / `fts_chunks` / `model_versions`. Operations on the returned [`StorageTx`]
  /// are atomic: they either all commit via [`StorageTx::commit`] or all roll
  /// back when the transaction is dropped without committing.
  fn begin_tx(&self) -> Result<Box<dyn StorageTx + '_>>;
}

/// Atomic write transaction over the indexed tables. All mutations flow
/// through this trait so a re-index / re-embed failure cannot leave the
/// store half-written.
pub trait StorageTx {
  fn add_document(&mut self, doc: &Document) -> Result<()>;
  fn set_document_path(&mut self, doc_id: &str, path: &str) -> Result<()>;
  fn delete_document(&mut self, doc_id: &str) -> Result<()>;
  fn add_chunks(&mut self, chunks: &[Chunk]) -> Result<()>;
  fn add_chunk_documents(&mut self, chunk_ids: &[String], doc_id: &str) -> Result<()>;
  fn delete_chunks_by_doc(&mut self, doc_id: &str) -> Result<()>;
  fn add_vectors(&mut self, chunk_ids: &[String], embeddings: &[Vec<f32>]) -> Result<()>;
  fn add_fts_chunks(&mut self, chunk_ids: &[String], tokenized_texts: &[String]) -> Result<()>;
  fn set_model_version(&mut self, role: ModelRole, version: &ModelSpec) -> Result<()>;
  fn add_collection(&mut self, name: &str, path: &str) -> Result<()>;
  fn commit(&mut self) -> Result<()>;
}
