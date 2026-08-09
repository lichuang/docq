//! File reading, chunking, and indexing pipeline.

pub mod chunker;
pub mod reader;
pub mod tokenizer;

pub use chunker::SentenceSplitter;
pub use reader::{DocumentSource, TextReader};
pub use tokenizer::{jieba_tokenize, JiebaSegmenter};

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use docq_core::{Chunk, Chunker, Document, Embedder, ParseError, Result, Storage, WordSegmenter};
use sha2::{Digest, Sha256};

pub struct IndexerConfig {
  pub chunker: Arc<dyn Chunker>,
  pub embedder: Arc<dyn Embedder>,
  pub segmenter: Arc<dyn WordSegmenter>,
  pub storage: Arc<dyn Storage>,
  pub reader: TextReader,
}

pub struct Indexer {
  config: IndexerConfig,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexStats {
  pub files_indexed: usize,
  pub files_skipped: usize,
  pub chunks_indexed: usize,
}

impl IndexStats {
  pub fn merge(&mut self, other: &IndexStats) {
    self.files_indexed += other.files_indexed;
    self.files_skipped += other.files_skipped;
    self.chunks_indexed += other.chunks_indexed;
  }
}

impl Indexer {
  pub fn new(config: IndexerConfig) -> Self {
    Self { config }
  }

  pub async fn index_file(&self, path: &Path) -> Result<IndexStats> {
    let content = std::fs::read_to_string(path).map_err(|e| ParseError::Other(format!("{}: {e}", path.display())))?;
    let content_hash = sha256_hex(&content);
    let doc_id = path.to_string_lossy().to_string();

    if let Some(existing) = self.config.storage.get_document(&doc_id)? {
      if existing.content_hash == content_hash {
        return Ok(IndexStats {
          files_skipped: 1,
          ..Default::default()
        });
      }
    }

    let candidates = self.config.chunker.chunk(&content);
    if candidates.is_empty() {
      return Ok(IndexStats {
        files_skipped: 1,
        ..Default::default()
      });
    }

    let chunk_texts: Vec<String> = candidates.iter().map(|c| c.text.clone()).collect();
    let embeddings = self.config.embedder.embed(&chunk_texts).await?;

    let chunks: Vec<Chunk> = candidates
      .iter()
      .map(|c| Chunk {
        id: sha256_hex(&c.text),
        doc_id: doc_id.clone(),
        text: c.text.clone(),
        byte_range: c.byte_range.clone(),
      })
      .collect();

    let chunk_ids: Vec<String> = chunks.iter().map(|c| c.id.clone()).collect();
    let tokenized_texts: Vec<String> = chunk_texts.iter().map(|t| self.config.segmenter.segment(t)).collect();

    let doc = Document {
      id: doc_id.clone(),
      file_path: path.to_path_buf(),
      content_hash: content_hash.clone(),
      content_size: content.len(),
      indexed_at: Utc::now(),
    };

    {
      let mut tx = self.config.storage.begin_tx()?;
      if self.config.storage.get_document(&doc_id)?.is_some() {
        tx.delete_chunks_by_doc(&doc_id)?;
      }
      tx.add_document(&doc)?;
      tx.add_chunks(&chunks)?;
      tx.add_vectors(&chunk_ids, &embeddings)?;
      tx.add_fts_chunks(&chunk_ids, &tokenized_texts)?;
      tx.commit()?;
    }

    Ok(IndexStats {
      files_indexed: 1,
      chunks_indexed: chunks.len(),
      ..Default::default()
    })
  }

  pub async fn index_directory(&self, path: &Path) -> Result<IndexStats> {
    let docs = self.config.reader.read_dir(path, true)?;
    let mut stats = IndexStats::default();
    for doc_src in &docs {
      let s = self.index_file(&doc_src.path).await?;
      stats.merge(&s);
    }
    Ok(stats)
  }
}

fn sha256_hex(s: &str) -> String {
  let mut hasher = Sha256::new();
  hasher.update(s.as_bytes());
  let hash = hasher.finalize();
  hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use docq_core::{ChunkCandidate, Embedder};
  use docq_storage::SqliteStorage;
  use tempfile::TempDir;

  struct StubEmbedder {
    dim: usize,
  }

  #[async_trait::async_trait]
  impl Embedder for StubEmbedder {
    fn dimension(&self) -> usize {
      self.dim
    }

    fn model_name(&self) -> &str {
      "stub"
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
      Ok(texts.iter().map(|_| vec![0.1_f32; self.dim]).collect())
    }
  }

  struct StubChunker;

  impl Chunker for StubChunker {
    fn chunk(&self, text: &str) -> Vec<ChunkCandidate> {
      if text.trim().is_empty() {
        return Vec::new();
      }
      vec![ChunkCandidate {
        text: text.to_string(),
        byte_range: 0..text.len(),
      }]
    }
  }

  fn test_storage() -> SqliteStorage {
    let s = SqliteStorage::open_in_memory().unwrap();
    s.init().unwrap();
    s
  }

  fn test_indexer(storage: SqliteStorage) -> Indexer {
    Indexer::new(IndexerConfig {
      chunker: Arc::new(StubChunker),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      storage: Arc::new(storage),
      reader: TextReader::new(),
    })
  }

  #[tokio::test]
  async fn test_index_file_basic() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("note.txt");
    std::fs::write(&path, "hello world").unwrap();

    let storage = test_storage();
    let indexer = test_indexer(storage);

    let stats = indexer.index_file(&path).await.unwrap();
    assert_eq!(stats.files_indexed, 1);
    assert_eq!(stats.chunks_indexed, 1);
    assert_eq!(stats.files_skipped, 0);
  }

  #[tokio::test]
  async fn test_index_file_skip_unchanged() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("note.txt");
    std::fs::write(&path, "hello world").unwrap();

    let storage = test_storage();
    let indexer = test_indexer(storage);

    let stats1 = indexer.index_file(&path).await.unwrap();
    assert_eq!(stats1.chunks_indexed, 1);

    let stats2 = indexer.index_file(&path).await.unwrap();
    assert_eq!(stats2.files_skipped, 1);
    assert_eq!(stats2.chunks_indexed, 0);
  }

  #[tokio::test]
  async fn test_index_file_reindex_on_change() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("note.txt");
    std::fs::write(&path, "hello world").unwrap();

    let storage = test_storage();
    let indexer = test_indexer(storage);

    let stats1 = indexer.index_file(&path).await.unwrap();
    assert_eq!(stats1.chunks_indexed, 1);

    std::fs::write(&path, "changed content").unwrap();
    let stats2 = indexer.index_file(&path).await.unwrap();
    assert_eq!(stats2.files_indexed, 1);
    assert_eq!(stats2.chunks_indexed, 1);
    assert_eq!(stats2.files_skipped, 0);
  }

  #[tokio::test]
  async fn test_index_directory() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "first file").unwrap();
    std::fs::write(tmp.path().join("b.md"), "second file").unwrap();
    std::fs::write(tmp.path().join("c.bin"), "binary").unwrap();

    let storage = test_storage();
    let indexer = test_indexer(storage);

    let stats = indexer.index_directory(tmp.path()).await.unwrap();
    assert_eq!(stats.files_indexed, 2);
    assert_eq!(stats.chunks_indexed, 2);
  }

  #[tokio::test]
  async fn test_index_then_search() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("note.txt");
    std::fs::write(&path, "分布式共识算法").unwrap();

    let storage = Arc::new(test_storage());
    let indexer = Indexer::new(IndexerConfig {
      chunker: Arc::new(StubChunker),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      storage: storage.clone(),
      reader: TextReader::new(),
    });

    indexer.index_file(&path).await.unwrap();

    let hits = storage.search_text("共识 算法", 10).unwrap();
    assert_eq!(hits.len(), 1);
  }
}
