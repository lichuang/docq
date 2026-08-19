//! Hybrid retrieval: BM25 + vector recall fused via Reciprocal Rank Fusion.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use docq_core::{
  Chunk, EmbedError, Embedder, Reranker, Result, ScoreExplain, ScoredChunk, SearchHit, Storage, Verbose, WordSegmenter,
};

use crate::fusion;

pub struct RetrieverConfig {
  pub storage: Arc<dyn Storage>,
  pub embedder: Arc<dyn Embedder>,
  pub segmenter: Arc<dyn WordSegmenter>,
  pub reranker: Option<Arc<dyn Reranker>>,
  /// BM25 recall depth (default 100).
  pub bm25_top_k: usize,
  /// Vector recall depth (default 100).
  pub vector_top_k: usize,
  /// RRF smoothing constant (default 60).
  pub rrf_k: usize,
  /// Number of RRF results to rerank (default 20).
  pub rerank_top_n: usize,
  /// Whether to print per-step progress and timings.
  pub verbose: Verbose,
}

pub struct Retriever {
  storage: Arc<dyn Storage>,
  embedder: Arc<dyn Embedder>,
  segmenter: Arc<dyn WordSegmenter>,
  reranker: Option<Arc<dyn Reranker>>,
  bm25_top_k: usize,
  vector_top_k: usize,
  rrf_k: usize,
  rerank_top_n: usize,
  verbose: Verbose,
}

impl Retriever {
  pub fn new(config: RetrieverConfig) -> Self {
    Self {
      storage: config.storage,
      embedder: config.embedder,
      segmenter: config.segmenter,
      reranker: config.reranker,
      bm25_top_k: config.bm25_top_k,
      vector_top_k: config.vector_top_k,
      rrf_k: config.rrf_k,
      rerank_top_n: config.rerank_top_n,
      verbose: config.verbose,
    }
  }

  /// Run a hybrid search: embed the query, recall from BM25 and vector
  /// stores, fuse with RRF, optionally rerank with a cross-encoder, then
  /// return the top-k chunks with full score breakdowns.
  ///
  /// Score directions in `ScoreExplain` are unified to "higher is better":
  /// - `bm25_score`: raw FTS5 BM25 score (higher = more relevant)
  /// - `vector_score`: derived similarity = `1.0 - distance` (higher = closer)
  /// - `rrf_score`: RRF fused score (higher = better rank)
  /// - `rerank_score` / `final_score`: cross-encoder score (higher = more
  ///   relevant); when no reranker is configured, `final_score = rrf_score`.
  pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
      return Ok(Vec::new());
    }

    let _total = self.verbose.start("search");

    // ---- Channel 1: BM25 lexical recall ----
    // Index time stored jieba-segmented text; query must be segmented the same
    // way so FTS5 matches on the same token boundaries.
    let segmented_query = {
      let _step = self.verbose.start("segment query");
      self.segmenter.segment(query)
    };
    let safe_query = sanitize_fts5_query(&segmented_query);
    let bm25_results = {
      let _step = self.verbose.start("BM25 recall");
      self.storage.search_text(&safe_query, self.bm25_top_k)?
    };

    // ---- Channel 2: vector semantic recall ----
    // Embed the query, then KNN-search sqlite-vec which returns cosine
    // *distance* (lower = more similar). Convert to similarity so every
    // score in ScoreExplain follows the same "higher is better" convention.
    let query_embedding = {
      let _step = self.verbose.start("embed query");
      self
        .embedder
        .embed(&[query.to_string()])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| EmbedError::Other("empty embedding result".into()))?
    };

    let vector_raw = {
      let _step = self.verbose.start("vector recall");
      self.storage.search_vectors(&query_embedding, self.vector_top_k)?
    };
    let bm25_map: HashMap<String, f32> = bm25_results.iter().cloned().collect();
    let vector_map: HashMap<String, f32> = vector_raw.iter().map(|(id, dist)| (id.clone(), 1.0 - dist)).collect();

    // ---- RRF fusion ----
    // RRF uses only rank positions, not raw scores, so the directional
    // difference between BM25 (higher better) and distance (lower better)
    // does not affect fusion. Pass the original score vectors; fusion
    // ignores their values and uses rank only.
    let fused = {
      let _step = self.verbose.start("RRF fusion");
      fusion::reciprocal_rank_fusion(&bm25_results, &vector_raw, self.rrf_k)
    };
    if fused.is_empty() {
      return Ok(Vec::new());
    }

    let rrf_map: HashMap<String, f32> = fused.iter().cloned().collect();

    // ---- Determine fetch depth, rerank, and final ordering in one branch ----
    // Without a reranker: fetch top_k, keep RRF order.
    // With a reranker: fetch rerank_top_n, cross-encoder rerank, sort by score.
    let (chunk_map, rerank_map, ordered) = match &self.reranker {
      None => {
        let ids: Vec<String> = fused.iter().take(top_k).map(|(id, _)| id.clone()).collect();
        let chunks = self.storage.get_chunks(&ids)?;
        let map = chunks.into_iter().map(|c| (c.id.clone(), c)).collect();
        (map, HashMap::new(), fused)
      }
      Some(reranker) => {
        // The reranker receives the raw query (not jieba-segmented) because it
        // runs its own BERT-style tokenization. It produces a single relevance
        // score per (query, chunk) pair — higher is better, same direction as RRF.
        let ids: Vec<String> = fused.iter().take(self.rerank_top_n).map(|(id, _)| id.clone()).collect();
        let chunks = self.storage.get_chunks(&ids)?;
        let chunk_map: HashMap<String, Chunk> = chunks.into_iter().map(|c| (c.id.clone(), c)).collect();

        let rerank_chunks: Vec<Chunk> = ids.iter().filter_map(|id| chunk_map.get(id).cloned()).collect();
        let scored: Vec<ScoredChunk> = {
          let _step = self.verbose.start("rerank");
          reranker.rerank(query, &rerank_chunks).await?
        };
        let rerank_map: HashMap<String, f32> = scored.into_iter().map(|sc| (sc.chunk.id, sc.score)).collect();

        let mut sorted: Vec<(String, f32)> = fused.into_iter().take(self.rerank_top_n).collect();
        sorted.sort_by(|a, b| {
          let ra = rerank_map.get(&a.0).copied().unwrap_or(0.0);
          let rb = rerank_map.get(&b.0).copied().unwrap_or(0.0);
          rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal)
        });

        (chunk_map, rerank_map, sorted)
      }
    };

    // ---- Resolve file paths for the retrieved chunks ----
    let file_paths = {
      let _step = self.verbose.start("resolve paths");
      let doc_ids: Vec<String> =
        chunk_map.values().map(|c| c.doc_id.clone()).collect::<HashSet<_>>().into_iter().collect();
      self.storage.get_document_paths(&doc_ids)?
    };

    // ---- Assemble SearchHit with per-stage ScoreExplain ----
    let hits = {
      let _step = self.verbose.start("assemble hits");
      ordered
        .into_iter()
        .take(top_k)
        .filter_map(|(id, _)| {
          let chunk = chunk_map.get(&id)?;
          let rrf_score = rrf_map.get(&id).copied();
          let rerank_score = rerank_map.get(&id).copied();
          let final_score = rerank_score.or(rrf_score).unwrap_or(0.0);
          Some(SearchHit {
            chunk: chunk.clone(),
            file_path: file_paths.get(&chunk.doc_id).map(PathBuf::from).unwrap_or_default(),
            score: final_score,
            explain: ScoreExplain {
              bm25_score: bm25_map.get(&id).copied(),
              vector_score: vector_map.get(&id).copied(),
              rrf_score,
              rerank_score,
              final_score,
            },
          })
        })
        .collect()
    };

    Ok(hits)
  }
}

/// Escape each token in an FTS5 query so that special characters like `-`,
/// `:`, `(`, `)`, `"` etc. are treated as literals rather than query syntax.
/// Tokens are split on whitespace and each is wrapped in double quotes.
fn sanitize_fts5_query(query: &str) -> String {
  query
    .split_whitespace()
    .map(|token| {
      if token.is_empty() {
        String::new()
      } else {
        // Escape embedded double quotes by doubling them.
        let escaped = token.replace('"', "\"\"");
        format!("\"{escaped}\"")
      }
    })
    .collect::<Vec<_>>()
    .join(" ")
}

#[cfg(test)]
mod tests {
  use super::*;
  use docq_core::{ChunkCandidate, Chunker, Embedder, Reranker, Result, ScoredChunk, Storage};
  use docq_indexer::{Indexer, IndexerConfig, JiebaSegmenter, ReaderRegistry, TextFileReader};
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
        readers: test_readers(),
        verbose: Verbose(false),
      });
      indexer.index_file(&path).await.unwrap();
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

  fn test_retriever(storage: &Arc<SqliteStorage>) -> Retriever {
    Retriever::new(RetrieverConfig {
      storage: storage.clone(),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      reranker: None,
      bm25_top_k: 100,
      vector_top_k: 100,
      rrf_k: 60,
      rerank_top_n: 20,
      verbose: Verbose(false),
    })
  }

  struct StubReranker;

  #[async_trait::async_trait]
  impl Reranker for StubReranker {
    async fn rerank(&self, _query: &str, chunks: &[Chunk]) -> Result<Vec<ScoredChunk>> {
      Ok(
        chunks
          .iter()
          .enumerate()
          .map(|(i, c)| ScoredChunk {
            chunk: c.clone(),
            score: (chunks.len() - i) as f32,
          })
          .collect(),
      )
    }
  }

  fn test_retriever_with_reranker(storage: &Arc<SqliteStorage>) -> Retriever {
    Retriever::new(RetrieverConfig {
      storage: storage.clone(),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      reranker: Some(Arc::new(StubReranker)),
      bm25_top_k: 100,
      vector_top_k: 100,
      rrf_k: 60,
      rerank_top_n: 20,
      verbose: Verbose(false),
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

  #[test]
  fn test_sanitize_fts5_query_quotes_each_token() {
    assert_eq!(sanitize_fts5_query("Multi-Paxos"), "\"Multi-Paxos\"");
    assert_eq!(sanitize_fts5_query("hello world"), "\"hello\" \"world\"");
    assert_eq!(
      sanitize_fts5_query("a \"quoted\" term"),
      "\"a\" \"\"\"quoted\"\"\" \"term\""
    );
  }

  #[tokio::test]
  async fn test_hybrid_search_with_reranker() {
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

    let retriever = test_retriever_with_reranker(&storage);
    let hits = retriever.search("生日", 5).await.unwrap();
    assert!(!hits.is_empty());

    for hit in &hits {
      assert!(hit.explain.rerank_score.is_some());
      assert_eq!(hit.score, hit.explain.final_score);
      assert_eq!(hit.score, hit.explain.rerank_score.unwrap());
    }
  }
}
