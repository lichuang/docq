//! Async boundaries:
//! - [`Storage`] is sync (SQLite is local blocking IO).
//! - [`Embedder`] / [`Reranker`] / [`Llm`] are async (heavy inference).
//!
//! [`Storage`]: crate::traits::Storage
//! [`Embedder`]: crate::traits::Embedder
//! [`Reranker`]: crate::traits::Reranker
//! [`Llm`]: crate::traits::Llm

use async_trait::async_trait;

use crate::error::Result;
use crate::models::{Chunk, ChunkCandidate, Document, ModelSpec};

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

pub trait Storage: Send + Sync {
  fn init(&self) -> Result<()>;

  // ---- documents ----

  fn add_document(&self, doc: &Document) -> Result<()>;
  fn get_document(&self, doc_id: &str) -> Result<Option<Document>>;
  fn list_documents(&self) -> Result<Vec<Document>>;
  fn delete_document(&self, doc_id: &str) -> Result<()>;

  // ---- chunks ----

  fn add_chunks(&self, chunks: &[Chunk]) -> Result<()>;
  fn get_chunks(&self, chunk_ids: &[String]) -> Result<Vec<Chunk>>;
  fn delete_chunks_by_doc(&self, doc_id: &str) -> Result<()>;

  // ---- vectors ----

  fn add_vectors(&self, chunk_ids: &[String], embeddings: &[Vec<f32>]) -> Result<()>;
  fn search_vectors(&self, embedding: &[f32], top_k: usize) -> Result<Vec<(String, f32)>>;

  // ---- full-text search ----

  fn search_text(&self, query: &str, top_k: usize) -> Result<Vec<(String, f32)>>;

  // ---- model versions ----

  fn set_model_version(&self, role: &str, version: &ModelSpec) -> Result<()>;
  fn get_model_version(&self, role: &str) -> Result<Option<ModelSpec>>;
}
