//! Hybrid retrieval: BM25 + vector recall fused via Reciprocal Rank Fusion.

use std::collections::HashMap;
use std::sync::Arc;

use docq_core::{Chunk, Embedder, Result, ScoreExplain, SearchHit, Storage, WordSegmenter};

use crate::fusion;

pub struct RetrieverConfig {
  pub storage: Arc<dyn Storage>,
  pub embedder: Arc<dyn Embedder>,
  pub segmenter: Arc<dyn WordSegmenter>,
  /// BM25 recall depth (default 100).
  pub bm25_top_k: usize,
  /// Vector recall depth (default 100).
  pub vector_top_k: usize,
  /// RRF smoothing constant (default 60).
  pub rrf_k: usize,
}

pub struct Retriever {
  config: RetrieverConfig,
}

impl Retriever {
  pub fn new(config: RetrieverConfig) -> Self {
    Self { config }
  }

  /// Run a hybrid search: embed the query, recall from BM25 and vector
  /// stores in parallel channels, fuse with RRF, then return the top-k
  /// chunks with full score breakdowns.
  ///
  /// Score directions in `ScoreExplain` are unified to "higher is better":
  /// - `bm25_score`: raw FTS5 BM25 score (higher = more relevant)
  /// - `vector_score`: derived similarity = `1.0 - distance` (higher = closer)
  /// - `rrf_score` / `final_score`: RRF fused score (higher = better rank)
  pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
      return Ok(Vec::new());
    }

    // ---- Channel 1: BM25 lexical recall ----
    // Index time stored jieba-segmented text; query must be segmented the same
    // way so FTS5 matches on the same token boundaries.
    let segmented_query = self.config.segmenter.segment(query);
    let bm25_results = self.config.storage.search_text(&segmented_query, self.config.bm25_top_k)?;

    // ---- Channel 2: vector semantic recall ----
    // Embed the query, then KNN-search sqlite-vec which returns cosine
    // *distance* (lower = more similar). Convert to similarity so every
    // score in ScoreExplain follows the same "higher is better" convention.
    let query_embedding = self
      .config
      .embedder
      .embed(&[query.to_string()])
      .await?
      .into_iter()
      .next()
      .ok_or_else(|| docq_core::EmbedError::Other("empty embedding result".into()))?;

    let vector_raw = self.config.storage.search_vectors(&query_embedding, self.config.vector_top_k)?;

    let vector_results: Vec<(String, f32)> = vector_raw.iter().map(|(id, dist)| (id.clone(), 1.0 - dist)).collect();

    // ---- Build lookup maps for score attribution ----
    let bm25_map: HashMap<String, f32> = bm25_results.iter().cloned().collect();
    let vector_map: HashMap<String, f32> = vector_results.iter().cloned().collect();

    // ---- RRF fusion ----
    // RRF uses only rank positions, not raw scores, so the directional
    // difference between BM25 (higher better) and distance (lower better)
    // does not affect fusion. Pass the original score vectors; fusion
    // ignores their values and uses rank only.
    let fused = fusion::reciprocal_rank_fusion(&bm25_results, &vector_raw, self.config.rrf_k);

    let top_ids: Vec<String> = fused.iter().take(top_k).map(|(id, _)| id.clone()).collect();
    if top_ids.is_empty() {
      return Ok(Vec::new());
    }

    // ---- Fetch full chunk text from storage ----
    let chunks = self.config.storage.get_chunks(&top_ids)?;
    let chunk_map: HashMap<String, Chunk> = chunks.into_iter().map(|c| (c.id.clone(), c)).collect();

    // ---- Assemble SearchHit with per-stage ScoreExplain ----
    let hits = fused
      .into_iter()
      .take(top_k)
      .filter_map(|(id, rrf_score)| {
        let chunk = chunk_map.get(&id)?;
        let bm25_score = bm25_map.get(&id).copied();
        let vector_score = vector_map.get(&id).copied();
        Some(SearchHit {
          chunk: chunk.clone(),
          score: rrf_score,
          explain: ScoreExplain {
            bm25_score,
            vector_score,
            rrf_score: Some(rrf_score),
            rerank_score: None,
            final_score: rrf_score,
          },
        })
      })
      .collect();

    Ok(hits)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use docq_core::{Chunker, Embedder, Result, Storage};
  use docq_indexer::{Indexer, IndexerConfig, JiebaSegmenter, TextReader};
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
      Ok(texts.iter().map(|t| hash_embedding(t, self.dim)).collect())
    }
  }

  fn hash_embedding(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0_f32; dim];
    for (i, byte) in text.bytes().enumerate() {
      vec[i % dim] += byte as f32 / 255.0;
    }
    vec
  }

  struct StubChunker;

  impl Chunker for StubChunker {
    fn chunk(&self, text: &str) -> Vec<docq_core::ChunkCandidate> {
      if text.trim().is_empty() {
        return Vec::new();
      }
      vec![docq_core::ChunkCandidate {
        text: text.to_string(),
        byte_range: 0..text.len(),
      }]
    }
  }

  async fn seed_index(storage: &Arc<SqliteStorage>, texts: &[(&str, &str)]) {
    let tmp = TempDir::new().unwrap();
    for (filename, content) in texts.iter() {
      let path = tmp.path().join(filename);
      std::fs::write(&path, content).unwrap();
      let indexer = Indexer::new(IndexerConfig {
        chunker: Arc::new(StubChunker),
        embedder: Arc::new(StubEmbedder { dim: 512 }),
        segmenter: Arc::new(JiebaSegmenter),
        storage: storage.clone(),
        reader: TextReader::new(),
      });
      indexer.index_file(&path).await.unwrap();
    }
  }

  fn test_storage() -> SqliteStorage {
    let s = SqliteStorage::open_in_memory().unwrap();
    s.init().unwrap();
    s
  }

  fn test_retriever(storage: &Arc<SqliteStorage>) -> Retriever {
    Retriever::new(RetrieverConfig {
      storage: storage.clone(),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      bm25_top_k: 100,
      vector_top_k: 100,
      rrf_k: 60,
    })
  }

  #[tokio::test]
  async fn test_hybrid_search_returns_results() {
    let storage = Arc::new(test_storage());
    seed_index(
      &storage,
      &[
        ("a.txt", "今天是我的生日"),
        ("b.txt", "分布式共识算法"),
        ("c.txt", "天气不错"),
      ],
    )
    .await;

    let retriever = test_retriever(&storage);
    let hits = retriever.search("生日", 5).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].score > 0.0);
    assert!(
      hits[0].chunk.text.contains("生日"),
      "top hit should contain '生日', got: {}",
      hits[0].chunk.text
    );
  }

  #[tokio::test]
  async fn test_hybrid_search_score_explain() {
    let storage = Arc::new(test_storage());
    seed_index(
      &storage,
      &[("a.txt", "共识算法解决一致性问题"), ("b.txt", "Raft共识算法")],
    )
    .await;

    let retriever = test_retriever(&storage);
    let hits = retriever.search("共识算法", 5).await.unwrap();
    assert!(!hits.is_empty());

    for hit in &hits {
      let exp = &hit.explain;
      assert!(exp.rrf_score.is_some());
      assert_eq!(exp.final_score, exp.rrf_score.unwrap());
      assert!(exp.rerank_score.is_none());
    }
  }

  #[tokio::test]
  async fn test_hybrid_search_empty_query() {
    let storage = Arc::new(test_storage());
    seed_index(&storage, &[("a.txt", "hello")]).await;

    let retriever = test_retriever(&storage);
    let hits = retriever.search("", 5).await;
    assert!(hits.is_ok());
  }

  #[tokio::test]
  async fn test_hybrid_search_no_results() {
    let storage = Arc::new(test_storage());

    let retriever = test_retriever(&storage);
    let hits = retriever.search("不存在的内容", 5).await.unwrap();
    assert!(hits.is_empty());
  }
}
