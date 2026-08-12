//! Engine facade — assembles all components into a single API.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use docq_core::{
  Chunker, Collection, Embedder, EngineStatus, Llm, LlmConfig, ModelSpec, Reranker, Result, SearchHit, Storage,
  Verbose, WordSegmenter,
};
use docq_indexer::{IndexStats, Indexer, IndexerConfig, JiebaSegmenter, SentenceSplitter, TextReader};
use docq_model::{BGE_SMALL_ZH_V1_5_TOKENIZER_FILE, FastEmbedEmbedder, FastEmbedReranker, GgufLlm, ModelHub};
use docq_retrieve::{Retriever, RetrieverConfig};

use crate::config::{DocqConfig, RetrievalConfig};
use docq_storage::SqliteStorage;
use docq_synth::{Synthesizer, SynthesizerConfig};

pub struct EngineConfig {
  pub workspace_path: PathBuf,
  pub model_cache_dir: PathBuf,
  pub config: DocqConfig,
  pub verbose: Verbose,
}

/// Pre-built components for `Engine::new` (dependency injection).
/// Tests construct this with stub implementations; `Engine::open_for_*`
/// constructs it with real model backends loaded on demand.
pub struct EngineComponents {
  pub storage: Arc<dyn Storage>,
  pub chunker: Arc<dyn Chunker>,
  pub embedder: Arc<dyn Embedder>,
  pub segmenter: Arc<dyn WordSegmenter>,
  pub reranker: Option<Arc<dyn Reranker>>,
  pub llm: Option<Arc<dyn Llm>>,
  pub reader: TextReader,
  pub retrieval: RetrievalConfig,
  pub verbose: Verbose,
}

pub struct Engine {
  storage: Arc<dyn Storage>,
  indexer: Indexer,
  retriever: Arc<Retriever>,
  synthesizer: Option<Synthesizer>,
  verbose: Verbose,
}

impl Engine {
  pub fn new(components: EngineComponents) -> Self {
    let EngineComponents {
      storage,
      chunker,
      embedder,
      segmenter,
      reranker,
      llm,
      reader,
      retrieval,
      verbose,
    } = components;

    let indexer = Indexer::new(IndexerConfig {
      chunker,
      embedder: embedder.clone(),
      segmenter: segmenter.clone(),
      storage: storage.clone(),
      reader,
      verbose,
    });

    let retriever = Arc::new(Retriever::new(RetrieverConfig {
      storage: storage.clone(),
      embedder,
      segmenter,
      reranker,
      bm25_top_k: retrieval.bm25_top_k,
      vector_top_k: retrieval.vector_top_k,
      rrf_k: retrieval.rrf_k,
      rerank_top_n: retrieval.rerank_top_n,
      verbose,
    }));

    let synthesizer = llm.map(|llm| {
      Synthesizer::new(SynthesizerConfig {
        retriever: retriever.clone(),
        llm,
        verbose,
      })
    });

    Self {
      storage,
      indexer,
      retriever,
      synthesizer,
      verbose,
    }
  }

  // ---- Shared helpers for model loading ----

  fn open_storage(workspace_path: &Path) -> Result<Arc<dyn Storage>> {
    std::fs::create_dir_all(workspace_path)
      .map_err(|e| docq_core::StoreError::Other(format!("create workspace dir: {e}")))?;
    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_workspace(workspace_path)?);
    storage.init()?;
    Ok(storage)
  }

  async fn build_chunker(
    hub: &ModelHub,
    emb_spec: &ModelSpec,
    indexing: &crate::config::IndexingConfig,
  ) -> Result<Arc<dyn Chunker>> {
    let tokenizer_spec = ModelSpec {
      role: "tokenizer".into(),
      repo_id: emb_spec.repo_id.clone(),
      filename: BGE_SMALL_ZH_V1_5_TOKENIZER_FILE.into(),
      revision: emb_spec.revision.clone(),
      checksum: None,
    };
    let path = hub.resolve(&tokenizer_spec).await?;
    let tokenizer = tokenizers::Tokenizer::from_file(&path)
      .map_err(|e| docq_core::LlmError::Other(format!("load tokenizer: {e}")))?;
    Ok(Arc::new(SentenceSplitter::new(
      tokenizer,
      indexing.chunk_size,
      indexing.chunk_overlap,
    )))
  }

  async fn load_embedding(
    hub: &ModelHub,
    storage: &dyn Storage,
    spec: &ModelSpec,
    indexing: &crate::config::IndexingConfig,
  ) -> Result<(Arc<dyn Embedder>, Arc<dyn Chunker>)> {
    hub.ensure(spec, storage).await?;
    let embedder = Arc::new(FastEmbedEmbedder::from_model_hub(hub, spec).await?);
    let chunker = Self::build_chunker(hub, spec, indexing).await?;
    Ok((embedder, chunker))
  }

  async fn load_reranker(hub: &ModelHub, storage: &dyn Storage, spec: &ModelSpec) -> Result<Arc<dyn Reranker>> {
    hub.ensure(spec, storage).await?;
    Ok(Arc::new(FastEmbedReranker::from_model_hub(hub, spec).await?))
  }

  async fn load_llm(hub: &ModelHub, storage: &dyn Storage, spec: &ModelSpec, llm: &LlmConfig) -> Result<Arc<dyn Llm>> {
    hub.ensure(spec, storage).await?;
    Ok(Arc::new(GgufLlm::from_model_hub(hub, spec, llm).await?))
  }

  // ---- On-demand open methods ----

  /// Open for indexing: loads embedding model only (~100 MB).
  pub async fn open_for_index(config: EngineConfig) -> Result<Self> {
    let EngineConfig {
      workspace_path,
      model_cache_dir,
      config,
      verbose,
    } = config;
    let storage = Self::open_storage(&workspace_path)?;
    let hub = ModelHub::new(model_cache_dir);
    let emb_spec = config.models.embedding.to_spec("embedding");
    let (embedder, chunker) = {
      let _step = verbose.start("load embedding model");
      Self::load_embedding(&hub, storage.as_ref(), &emb_spec, &config.indexing).await?
    };

    Ok(Self::new(EngineComponents {
      storage,
      chunker,
      embedder,
      segmenter: Arc::new(JiebaSegmenter),
      reranker: None,
      llm: None,
      reader: TextReader::new(),
      retrieval: config.retrieval.clone(),
      verbose,
    }))
  }

  /// Open for search: loads embedding + reranker models (~1.1 GB).
  pub async fn open_for_search(config: EngineConfig) -> Result<Self> {
    let EngineConfig {
      workspace_path,
      model_cache_dir,
      config,
      verbose,
    } = config;
    let storage = Self::open_storage(&workspace_path)?;
    let hub = ModelHub::new(model_cache_dir);
    let emb_spec = config.models.embedding.to_spec("embedding");
    let rerank_spec = config.models.reranker.to_spec("reranker");
    let (embedder, chunker) = {
      let _step = verbose.start("load embedding model");
      Self::load_embedding(&hub, storage.as_ref(), &emb_spec, &config.indexing).await?
    };
    let reranker = {
      let _step = verbose.start("load reranker model");
      Self::load_reranker(&hub, storage.as_ref(), &rerank_spec).await?
    };

    Ok(Self::new(EngineComponents {
      storage,
      chunker,
      embedder,
      segmenter: Arc::new(JiebaSegmenter),
      reranker: Some(reranker),
      llm: None,
      reader: TextReader::new(),
      retrieval: config.retrieval.clone(),
      verbose,
    }))
  }

  /// Open for ask: loads all models (~6 GB).
  pub async fn open_for_ask(config: EngineConfig) -> Result<Self> {
    let EngineConfig {
      workspace_path,
      model_cache_dir,
      config,
      verbose,
    } = config;
    let storage = Self::open_storage(&workspace_path)?;
    let hub = ModelHub::new(model_cache_dir);
    let emb_spec = config.models.embedding.to_spec("embedding");
    let rerank_spec = config.models.reranker.to_spec("reranker");
    let llm_spec = config.models.llm.to_spec("chat");
    let llm_config: LlmConfig = config.llm.clone().try_into()?;
    let (embedder, chunker) = {
      let _step = verbose.start("load embedding model");
      Self::load_embedding(&hub, storage.as_ref(), &emb_spec, &config.indexing).await?
    };
    let reranker = {
      let _step = verbose.start("load reranker model");
      Self::load_reranker(&hub, storage.as_ref(), &rerank_spec).await?
    };
    let llm = {
      let _step = verbose.start("load LLM");
      Self::load_llm(&hub, storage.as_ref(), &llm_spec, &llm_config).await?
    };

    Ok(Self::new(EngineComponents {
      storage,
      chunker,
      embedder,
      segmenter: Arc::new(JiebaSegmenter),
      reranker: Some(reranker),
      llm: Some(llm),
      reader: TextReader::new(),
      retrieval: config.retrieval.clone(),
      verbose,
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
    let _total = self.verbose.start("index");
    let collections = self.storage.list_collections()?;
    let mut stats = IndexStats::default();
    for col in collections {
      let s = self.indexer.index_directory(&col.path).await?;
      stats.merge(&s);
    }
    Ok(stats)
  }

  pub async fn index_one(&self, name: &str) -> Result<IndexStats> {
    let _total = self.verbose.start("index one collection");
    let collections = self.storage.list_collections()?;
    let col = collections
      .into_iter()
      .find(|c| c.name == name)
      .ok_or_else(|| docq_core::StoreError::Other(format!("collection not found: {name}")))?;
    self.indexer.index_directory(&col.path).await
  }

  pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
    self.retriever.search(query, top_k).await
  }

  pub async fn ask(&self, query: &str) -> Result<docq_core::Answer> {
    let synth = self
      .synthesizer
      .as_ref()
      .ok_or_else(|| docq_core::LlmError::Other("LLM not loaded — use open_for_ask".into()))?;
    synth.ask(query).await
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
      llm: Some(Arc::new(StubLlm)),
      reader: TextReader::new(),
      retrieval: crate::config::RetrievalConfig {
        bm25_top_k: 100,
        vector_top_k: 100,
        rrf_k: 60,
        rerank_top_n: 20,
      },
      verbose: Verbose(false),
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

  #[tokio::test]
  async fn test_engine_ask_without_llm_errors() {
    let tmp = TempDir::new().unwrap();
    let storage = test_storage(&tmp);
    let components = EngineComponents {
      storage,
      chunker: Arc::new(StubChunker),
      embedder: Arc::new(StubEmbedder { dim: 512 }),
      segmenter: Arc::new(JiebaSegmenter),
      reranker: None,
      llm: None,
      reader: TextReader::new(),
      retrieval: crate::config::RetrievalConfig {
        bm25_top_k: 100,
        vector_top_k: 100,
        rrf_k: 60,
        rerank_top_n: 20,
      },
      verbose: Verbose(false),
    };
    let engine = Engine::new(components);
    let result = engine.ask("test").await;
    assert!(result.is_err());
  }
}
