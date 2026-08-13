//! File reading, chunking, and indexing pipeline.

pub mod chunker;
pub mod reader;
pub mod reader_registry;
pub mod tokenizer;

pub use chunker::SentenceSplitter;
#[cfg(feature = "pdf")]
pub use reader::PdfReader;
pub use reader::TextFileReader;
pub use reader_registry::ReaderRegistry;
pub use tokenizer::{JiebaSegmenter, jieba_tokenize};

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use docq_core::{Chunk, Chunker, Document, Embedder, ParseError, Result, Storage, Verbose, WordSegmenter};
use sha2::{Digest, Sha256};

/// Maximum chunks buffered before flushing to embedding + storage.
/// Controls peak memory: 500 chunks × 512-dim f32 ≈ 1 MB embeddings +
/// chunk text. Large enough to amortize ONNX call overhead, small enough
/// to stay within ~2 MB per batch.
const EMBED_BATCH_SIZE: usize = 500;

pub struct IndexerConfig {
  pub chunker: Arc<dyn Chunker>,
  pub embedder: Arc<dyn Embedder>,
  pub segmenter: Arc<dyn WordSegmenter>,
  pub storage: Arc<dyn Storage>,
  pub readers: ReaderRegistry,
  pub verbose: Verbose,
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

struct PendingFile {
  doc: Document,
  chunks: Vec<Chunk>,
  chunk_texts: Vec<String>,
  tokenized_texts: Vec<String>,
}

impl Indexer {
  pub fn new(config: IndexerConfig) -> Self {
    Self { config }
  }

  pub async fn index_file(&self, path: &Path) -> Result<IndexStats> {
    let content = std::fs::read_to_string(path).map_err(|e| ParseError::Other(format!("{}: {e}", path.display())))?;
    match self.prepare_file(path, &content)? {
      Some(pending) => {
        let mut batch = vec![pending];
        self.flush_batch(&mut batch).await
      }
      None => Ok(IndexStats {
        files_skipped: 1,
        ..Default::default()
      }),
    }
  }

  pub async fn index_directory(&self, path: &Path) -> Result<IndexStats> {
    let docs = self.config.readers.read_dir(path, true)?;
    let total = docs.len();
    let mut stats = IndexStats::default();
    let mut pending: Vec<PendingFile> = Vec::new();
    let mut pending_chunk_count = 0usize;

    for (i, doc_src) in docs.iter().enumerate() {
      self.config.verbose.log(&format!(
        "chunking file {}/{} ({:.0}%): {}",
        i + 1,
        total,
        (i + 1) as f32 / total.max(1) as f32 * 100.0,
        doc_src.path.display()
      ));

      match self.prepare_file(&doc_src.path, &doc_src.content)? {
        Some(pf) => {
          pending_chunk_count += pf.chunks.len();
          pending.push(pf);
          if pending_chunk_count >= EMBED_BATCH_SIZE {
            let s = self.flush_batch(&mut pending).await?;
            stats.files_indexed += s.files_indexed;
            stats.chunks_indexed += s.chunks_indexed;
            pending_chunk_count = 0;
          }
        }
        None => {
          stats.files_skipped += 1;
        }
      }
    }

    if !pending.is_empty() {
      let s = self.flush_batch(&mut pending).await?;
      stats.files_indexed += s.files_indexed;
      stats.chunks_indexed += s.chunks_indexed;
    }

    Ok(stats)
  }

  /// Read a single file's content, skip unchanged/empty files, and build a
  /// `PendingFile` ready for batched embedding and storage.
  fn prepare_file(&self, path: &Path, content: &str) -> Result<Option<PendingFile>> {
    let content_hash = sha256_hex(content);
    let doc_id = path.to_string_lossy().to_string();

    if let Some(existing) = self.config.storage.get_document(&doc_id)?
      && existing.content_hash == content_hash
    {
      return Ok(None);
    }

    let candidates = self.config.chunker.chunk(content);
    if candidates.is_empty() {
      return Ok(None);
    }

    let chunk_texts: Vec<String> = candidates.iter().map(|c| c.text.clone()).collect();
    let tokenized_texts: Vec<String> = chunk_texts.iter().map(|t| self.config.segmenter.segment(t)).collect();
    let chunks: Vec<Chunk> = candidates
      .iter()
      .map(|c| Chunk {
        id: sha256_hex(&c.text),
        doc_id: doc_id.clone(),
        text: c.text.clone(),
        byte_range: c.byte_range.clone(),
      })
      .collect();

    let doc = Document {
      id: doc_id,
      file_path: path.to_path_buf(),
      content_hash,
      content_size: content.len(),
      indexed_at: Utc::now(),
    };

    Ok(Some(PendingFile {
      doc,
      chunks,
      chunk_texts,
      tokenized_texts,
    }))
  }

  /// Embed all chunks in `pending` in one batch, then write each file
  /// to storage in its own transaction.
  async fn flush_batch(&self, pending: &mut Vec<PendingFile>) -> Result<IndexStats> {
    let all_texts: Vec<String> = pending.iter().flat_map(|f| f.chunk_texts.iter().cloned()).collect();
    let all_embeddings = self.config.embedder.embed(&all_texts).await?;

    let mut stats = IndexStats::default();
    let mut offset = 0usize;

    for pf in pending.drain(..) {
      let n = pf.chunks.len();
      let embeddings: Vec<Vec<f32>> = all_embeddings[offset..offset + n].to_vec();
      let chunk_ids: Vec<String> = pf.chunks.iter().map(|c| c.id.clone()).collect();

      let mut tx = self.config.storage.begin_tx()?;
      if self.config.storage.get_document(&pf.doc.id)?.is_some() {
        tx.delete_chunks_by_doc(&pf.doc.id)?;
      }
      tx.add_document(&pf.doc)?;
      tx.add_chunks(&pf.chunks)?;
      tx.add_vectors(&chunk_ids, &embeddings)?;
      tx.add_fts_chunks(&chunk_ids, &pf.tokenized_texts)?;
      tx.commit()?;

      stats.files_indexed += 1;
      stats.chunks_indexed += n;
      offset += n;
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

  fn test_readers() -> ReaderRegistry {
    let mut reg = ReaderRegistry::new();
    reg.register(Arc::new(TextFileReader::new()));
    reg
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
      readers: test_readers(),
      verbose: Verbose(false),
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
      readers: test_readers(),
      verbose: Verbose(false),
    });

    indexer.index_file(&path).await.unwrap();

    let hits = storage.search_text("共识 算法", 10).unwrap();
    assert_eq!(hits.len(), 1);
  }
}
