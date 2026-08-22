use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use docq_core::{Chunk, Chunker, Document, Embedder, Result, Storage, Verbose, WordSegmenter};
use sha2::{Digest, Sha256};

use crate::ReaderRegistry;

const EMBED_BATCH_SIZE: usize = 500;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexStats {
  pub files_indexed: usize,
  pub files_skipped: usize,
  pub files_removed: usize,
  pub chunks_indexed: usize,
}

impl IndexStats {
  pub fn merge(&mut self, other: &IndexStats) {
    self.files_indexed += other.files_indexed;
    self.files_skipped += other.files_skipped;
    self.files_removed += other.files_removed;
    self.chunks_indexed += other.chunks_indexed;
  }
}

fn sha256_hex(s: &str) -> String {
  let mut hasher = Sha256::new();
  hasher.update(s.as_bytes());
  let hash = hasher.finalize();
  hash.iter().map(|b| format!("{b:02x}")).collect()
}

pub struct IndexerConfig {
  pub chunker: Arc<dyn Chunker>,
  pub embedder: Arc<dyn Embedder>,
  pub segmenter: Arc<dyn WordSegmenter>,
  pub storage: Arc<dyn Storage>,
  pub readers: ReaderRegistry,
  pub verbose: Verbose,
}

pub struct Indexer {
  chunker: Arc<dyn Chunker>,
  embedder: Arc<dyn Embedder>,
  segmenter: Arc<dyn WordSegmenter>,
  storage: Arc<dyn Storage>,
  readers: ReaderRegistry,
  verbose: Verbose,
}

struct PendingFile {
  path: PathBuf,
  doc: Document,
  chunks: Vec<Chunk>,
  chunk_texts: Vec<String>,
  tokenized_texts: Vec<String>,
  is_update: bool,
}

impl Indexer {
  pub fn new(config: IndexerConfig) -> Self {
    Self {
      chunker: config.chunker,
      embedder: config.embedder,
      segmenter: config.segmenter,
      storage: config.storage,
      readers: config.readers,
      verbose: config.verbose,
    }
  }

  pub async fn index_file(&self, path: &Path) -> Result<IndexStats> {
    let doc_src = match self.readers.read_file(path)? {
      Some(doc) => doc,
      None => {
        return Ok(IndexStats {
          files_skipped: 1,
          ..Default::default()
        });
      }
    };
    let existing_docs = self.storage.list_documents()?;
    let existing_map: HashMap<String, Document> = existing_docs.into_iter().map(|d| (d.id.clone(), d)).collect();
    match self.prepare_file(&doc_src.path, &doc_src.content, &existing_map)? {
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
    let docs = self.readers.read_dir(path, true)?;
    let total = docs.len();
    let mut stats = IndexStats::default();
    let mut pending: Vec<PendingFile> = Vec::new();
    let mut pending_chunk_count = 0usize;

    let current_doc_ids: std::collections::HashSet<String> =
      docs.iter().map(|d| sha256_hex(&d.path.to_string_lossy())).collect();

    let all_docs = self.storage.list_documents()?;
    stats.files_removed = self.sweep_deleted(path, &current_doc_ids, &all_docs)?;

    let existing_map: HashMap<String, Document> = all_docs.into_iter().map(|d| (d.id.clone(), d)).collect();

    for (i, doc_src) in docs.iter().enumerate() {
      self.verbose.log(&format!(
        "chunking file {}/{} ({:.0}%): {}",
        i + 1,
        total,
        (i + 1) as f32 / total.max(1) as f32 * 100.0,
        doc_src.path.display()
      ));

      match self.prepare_file(&doc_src.path, &doc_src.content, &existing_map)? {
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

  fn sweep_deleted(
    &self,
    dir: &Path,
    current_doc_ids: &std::collections::HashSet<String>,
    all_docs: &[Document],
  ) -> Result<usize> {
    if all_docs.is_empty() {
      return Ok(0);
    }

    let all_doc_ids: Vec<String> = all_docs.iter().map(|d| d.id.clone()).collect();
    let paths = self.storage.get_document_paths(&all_doc_ids)?;

    let dir_prefix = dir.to_string_lossy().to_string();
    let mut removed = 0usize;

    for doc in all_docs {
      if current_doc_ids.contains(&doc.id) {
        continue;
      }
      let Some(file_path) = paths.get(&doc.id) else {
        continue;
      };
      if !file_path.starts_with(&dir_prefix) {
        continue;
      }
      let mut tx = self.storage.begin_tx()?;
      tx.delete_document(&doc.id)?;
      tx.delete_chunks_by_doc(&doc.id)?;
      tx.commit()?;
      removed += 1;
    }

    Ok(removed)
  }

  /// Read a single file's content, skip unchanged/empty files, and build a
  /// `PendingFile` ready for batched embedding and storage.
  fn prepare_file(
    &self,
    path: &Path,
    content: &str,
    existing_docs: &HashMap<String, Document>,
  ) -> Result<Option<PendingFile>> {
    let content_hash = sha256_hex(content);
    let path_str = path.to_string_lossy().to_string();
    let doc_id = sha256_hex(&path_str);

    let is_update = if let Some(existing) = existing_docs.get(&doc_id) {
      if existing.content_hash == content_hash {
        return Ok(None);
      }
      true
    } else {
      false
    };

    let candidates = self.chunker.chunk(content);
    if candidates.is_empty() {
      return Ok(None);
    }

    let chunk_texts: Vec<String> = candidates.iter().map(|c| c.text.clone()).collect();
    let tokenized_texts: Vec<String> = chunk_texts.iter().map(|t| self.segmenter.segment(t)).collect();
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
      content_hash,
      content_size: content.len(),
      indexed_at: Utc::now(),
    };

    Ok(Some(PendingFile {
      path: path.to_path_buf(),
      doc,
      chunks,
      chunk_texts,
      tokenized_texts,
      is_update,
    }))
  }

  /// Embed all chunks in `pending` in one batch, then write each file
  /// to storage in its own transaction.
  async fn flush_batch(&self, pending: &mut Vec<PendingFile>) -> Result<IndexStats> {
    let all_texts: Vec<String> = pending.iter().flat_map(|f| f.chunk_texts.iter().cloned()).collect();
    let all_embeddings = self.embedder.embed(&all_texts).await?;

    let mut stats = IndexStats::default();
    let mut offset = 0usize;

    for pf in pending.drain(..) {
      let n = pf.chunks.len();
      let embeddings: Vec<Vec<f32>> = all_embeddings[offset..offset + n].to_vec();
      let chunk_ids: Vec<String> = pf.chunks.iter().map(|c| c.id.clone()).collect();

      let mut tx = self.storage.begin_tx()?;
      if pf.is_update {
        tx.delete_chunks_by_doc(&pf.doc.id)?;
      }
      tx.add_document(&pf.doc)?;
      tx.set_document_path(&pf.doc.id, &pf.path.to_string_lossy())?;
      tx.add_chunks(&pf.chunks)?;
      tx.add_chunk_documents(&chunk_ids, &pf.doc.id)?;
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{JiebaSegmenter, TextFileReader};
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
    s.init(512).unwrap();
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
  async fn test_index_directory_removes_deleted_files() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "first file").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "second file").unwrap();

    let storage: Arc<dyn Storage> = Arc::new(test_storage());
    let indexer = Indexer::new(IndexerConfig {
      chunker: Arc::new(StubChunker),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      storage: storage.clone(),
      readers: test_readers(),
      verbose: Verbose(false),
    });
    indexer.index_directory(tmp.path()).await.unwrap();

    assert_eq!(storage.list_documents().unwrap().len(), 2);

    std::fs::remove_file(tmp.path().join("a.txt")).unwrap();

    let indexer2 = Indexer::new(IndexerConfig {
      chunker: Arc::new(StubChunker),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      storage: storage.clone(),
      readers: test_readers(),
      verbose: Verbose(false),
    });
    let stats = indexer2.index_directory(tmp.path()).await.unwrap();
    assert_eq!(stats.files_removed, 1);
    assert_eq!(stats.files_indexed, 0);

    let docs = storage.list_documents().unwrap();
    assert_eq!(docs.len(), 1);
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
