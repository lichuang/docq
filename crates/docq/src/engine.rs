//! Engine facade — assembles all components into a single API.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use docq_core::{
  Chunker, Collection, Embedder, EngineStatus, Llm, LlmConfig, ModelSpec, Reranker, Result, SearchHit, Storage,
  WordSegmenter,
};
use docq_indexer::{IndexStats, Indexer, IndexerConfig, JiebaSegmenter, SentenceSplitter, TextReader};
use docq_model::{
  FastEmbedEmbedder, FastEmbedReranker, GgufLlm, ModelHub, ModelRegistry, EMBEDDING_MAX_TOKENS,
  EMBEDDING_TOKENIZER_FILE,
};
use docq_retrieve::{Retriever, RetrieverConfig};
use docq_storage::SqliteStorage;
use docq_synth::{Synthesizer, SynthesizerConfig};

pub struct EngineConfig {
  pub workspace_path: PathBuf,
  pub model_cache_dir: PathBuf,
}

/// Pre-built components for `Engine::new` (dependency injection).
/// Tests construct this with stub implementations; `Engine::open`
/// constructs it with real model backends.
pub struct EngineComponents {
  pub storage: Arc<dyn Storage>,
  pub chunker: Arc<dyn Chunker>,
  pub embedder: Arc<dyn Embedder>,
  pub segmenter: Arc<dyn WordSegmenter>,
  pub reranker: Option<Arc<dyn Reranker>>,
  pub llm: Arc<dyn Llm>,
  pub reader: TextReader,
}

pub struct Engine {
  storage: Arc<dyn Storage>,
  indexer: Indexer,
  retriever: Arc<Retriever>,
  synthesizer: Synthesizer,
}

impl Engine {
  /// Construct an `Engine` from pre-built components (dependency injection).
  /// Tests use this to inject stub embedders / LLMs without network access.
  pub fn new(components: EngineComponents) -> Self {
    let EngineComponents {
      storage,
      chunker,
      embedder,
      segmenter,
      reranker,
      llm,
      reader,
    } = components;

    let indexer = Indexer::new(IndexerConfig {
      chunker,
      embedder: embedder.clone(),
      segmenter: segmenter.clone(),
      storage: storage.clone(),
      reader,
    });

    let retriever = Arc::new(Retriever::new(RetrieverConfig {
      storage: storage.clone(),
      embedder,
      segmenter,
      reranker,
      bm25_top_k: 100,
      vector_top_k: 100,
      rrf_k: 60,
      rerank_top_n: 20,
    }));

    let synthesizer = Synthesizer::new(SynthesizerConfig {
      retriever: retriever.clone(),
      llm,
    });

    Self {
      storage,
      indexer,
      retriever,
      synthesizer,
    }
  }

  /// Open a workspace, download models on first use, and assemble all
  /// components with default configurations. Requires network access for
  /// initial model download (~6 GB total).
  pub async fn open(config: EngineConfig) -> Result<Self> {
    std::fs::create_dir_all(&config.workspace_path)
      .map_err(|e| docq_core::StoreError::Other(format!("create workspace dir: {e}")))?;
    let db_path = config.workspace_path.join("docq.db");
    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open(&db_path)?);
    storage.init()?;

    let hub = ModelHub::new(config.model_cache_dir);

    // ---- Download models and record their specs in model_versions ----
    // hub.ensure downloads the model (if not cached) and writes the spec
    // to the model_versions table via a StorageTx. This lets the indexer
    // detect stale embeddings when the embedding model is upgraded.
    let emb_spec = ModelRegistry::default_embedding();
    hub.ensure(&emb_spec, storage.as_ref()).await?;

    let rerank_spec = ModelRegistry::default_reranker();
    hub.ensure(&rerank_spec, storage.as_ref()).await?;

    let llm_spec = ModelRegistry::default_llm();
    hub.ensure(&llm_spec, storage.as_ref()).await?;

    // ---- Build inference backends from the downloaded model files ----
    let embedder = Arc::new(FastEmbedEmbedder::from_model_hub(&hub, &emb_spec).await?);
    let reranker = Arc::new(FastEmbedReranker::from_model_hub(&hub, &rerank_spec).await?);
    let llm = Arc::new(GgufLlm::from_model_hub(&hub, &llm_spec, &LlmConfig::default()).await?);

    let segmenter = Arc::new(JiebaSegmenter);
    let reader = TextReader::new();

    // ---- Build SentenceSplitter with the embedding model's tokenizer ----
    // The tokenizer.json lives in the same HF repo as the embedding model,
    // so we resolve it via hub.resolve() with a separate ModelSpec.
    let tokenizer_spec = ModelSpec {
      role: "tokenizer".into(),
      repo_id: emb_spec.repo_id.clone(),
      filename: EMBEDDING_TOKENIZER_FILE.into(),
      revision: emb_spec.revision.clone(),
      checksum: None,
    };
    let tokenizer_file = hub.resolve(&tokenizer_spec).await?;
    let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_file)
      .map_err(|e| docq_core::LlmError::Other(format!("load tokenizer: {e}")))?;
    let chunker = Arc::new(SentenceSplitter::new(
      tokenizer,
      EMBEDDING_MAX_TOKENS,
      EMBEDDING_MAX_TOKENS / 10,
    ));

    Ok(Self::new(EngineComponents {
      storage,
      chunker,
      embedder,
      segmenter,
      reranker: Some(reranker),
      llm,
      reader,
    }))
  }

  pub fn add_collection(&self, path: impl AsRef<Path>, name: &str) -> Result<()> {
    let canonical = std::fs::canonicalize(path.as_ref())
      .map_err(|e| docq_core::StoreError::Other(format!("canonicalize {}: {e}", path.as_ref().display())))?;
    let path_str = canonical.to_string_lossy().to_string();
    let mut tx = self.storage.begin_tx()?;
    tx.add_collection(name, &path_str)?;
    tx.commit()?;
    Ok(())
  }

  pub fn list_collections(&self) -> Result<Vec<Collection>> {
    self.storage.list_collections()
  }

  pub async fn index(&self) -> Result<IndexStats> {
    let collections = self.storage.list_collections()?;
    let mut stats = IndexStats::default();
    for col in collections {
      let s = self.indexer.index_directory(&col.path).await?;
      stats.merge(&s);
    }
    Ok(stats)
  }

  pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
    self.retriever.search(query, top_k).await
  }

  pub async fn ask(&self, query: &str) -> Result<docq_core::Answer> {
    self.synthesizer.ask(query).await
  }

  pub fn status(&self) -> Result<EngineStatus> {
    let docs = self.storage.list_documents()?;
    let collections = self.storage.list_collections()?;
    let chunks = self.storage.count_chunks()?;
    Ok(EngineStatus {
      documents: docs.len(),
      chunks,
      collections,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use docq_core::{ChunkCandidate, Chunker, Embedder, Llm, Storage};
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

  struct StubLlm;

  #[async_trait::async_trait]
  impl Llm for StubLlm {
    async fn complete(&self, _prompt: &str) -> Result<String> {
      Ok("This is a stub answer [1].".to_string())
    }
  }

  fn test_storage(tmp: &TempDir) -> Arc<dyn Storage> {
    let storage = Arc::new(SqliteStorage::open(tmp.path().join("test.db")).unwrap()) as Arc<dyn Storage>;
    storage.init().unwrap();
    storage
  }

  fn test_components(storage: Arc<dyn Storage>) -> EngineComponents {
    EngineComponents {
      storage,
      chunker: Arc::new(StubChunker),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      reranker: None,
      llm: Arc::new(StubLlm),
      reader: TextReader::new(),
    }
  }

  #[tokio::test]
  async fn test_engine_add_collection_and_status() {
    let tmp = TempDir::new().unwrap();
    let storage = test_storage(&tmp);
    let engine = Engine::new(test_components(storage));

    let notes_dir = TempDir::new().unwrap();
    engine.add_collection(notes_dir.path(), "notes").unwrap();

    let collections = engine.list_collections().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].name, "notes");

    let status = engine.status().unwrap();
    assert_eq!(status.collections.len(), 1);
    assert_eq!(status.documents, 0);
  }

  #[tokio::test]
  async fn test_engine_index_and_search() {
    let tmp = TempDir::new().unwrap();
    let storage = test_storage(&tmp);
    let engine = Engine::new(test_components(storage));

    let notes_dir = TempDir::new().unwrap();
    std::fs::write(notes_dir.path().join("note.txt"), "今天是我的生日").unwrap();
    engine.add_collection(notes_dir.path(), "notes").unwrap();

    let stats = engine.index().await.unwrap();
    assert!(stats.chunks_indexed > 0);

    let hits = engine.search("生日", 5).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].chunk.text.contains("生日"));
  }

  #[tokio::test]
  async fn test_engine_ask() {
    let tmp = TempDir::new().unwrap();
    let storage = test_storage(&tmp);
    let engine = Engine::new(test_components(storage));

    let notes_dir = TempDir::new().unwrap();
    std::fs::write(notes_dir.path().join("note.txt"), "定价方案选坐席制").unwrap();
    engine.add_collection(notes_dir.path(), "notes").unwrap();
    engine.index().await.unwrap();

    let answer = engine.ask("定价方案").await.unwrap();
    assert!(!answer.text.is_empty());
  }
}
