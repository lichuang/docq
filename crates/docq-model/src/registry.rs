use docq_core::ModelSpec;

// ---- Default embedding model ----

pub const EMBEDDING_REPO: &str = "Xenova/bge-small-zh-v1.5";
pub const EMBEDDING_FILE: &str = "onnx/model.onnx";
/// Tokenizer file co-located in the same HF repo as the embedding model.
pub const EMBEDDING_TOKENIZER_FILE: &str = "tokenizer.json";
/// Maximum input length the embedding model accepts. Chunks exceeding this
/// are silently truncated by the ONNX runtime, so `SentenceSplitter` must
/// use this as `chunk_size` to avoid losing tail content.
pub const EMBEDDING_MAX_TOKENS: usize = 512;

// ---- Default reranker model ----

pub const RERANKER_REPO: &str = "BAAI/bge-reranker-base";
pub const RERANKER_FILE: &str = "onnx/model.onnx";

// ---- Default LLM model ----

pub const LLM_REPO: &str = "Qwen/Qwen2.5-7B-Instruct-GGUF";
pub const LLM_FILE: &str = "qwen2.5-7b-instruct-q4_k_m.gguf";

// ---- Other supported embedding repos (for repo_id → fastembed mapping) ----

pub const EMBEDDING_REPO_BGE_LARGE_ZH: &str = "Xenova/bge-large-zh-v1.5";
pub const EMBEDDING_REPO_BGE_M3: &str = "BAAI/bge-m3";

// ---- Other supported reranker repos (for repo_id → fastembed mapping) ----

pub const RERANKER_REPO_BGE_V2_M3: &str = "BAAI/bge-reranker-v2-m3";
/// Same model as above, mirrored under a different HF repo.
pub const RERANKER_REPO_BGE_V2_M3_ALT: &str = "rozgo/bge-reranker-v2-m3";
pub const RERANKER_REPO_JINA_V1_TURBO_EN: &str = "jinaai/jina-reranker-v1-turbo-en";
pub const RERANKER_REPO_JINA_V2_MULTILINGUAL: &str = "jinaai/jina-reranker-v2-base-multilingual";

pub struct ModelRegistry;

impl ModelRegistry {
  pub fn default_embedding() -> ModelSpec {
    ModelSpec {
      role: "embedding".into(),
      repo_id: EMBEDDING_REPO.into(),
      filename: EMBEDDING_FILE.into(),
      revision: "main".into(),
      checksum: None,
    }
  }

  pub fn default_reranker() -> ModelSpec {
    ModelSpec {
      role: "reranker".into(),
      repo_id: RERANKER_REPO.into(),
      filename: RERANKER_FILE.into(),
      revision: "main".into(),
      checksum: None,
    }
  }

  pub fn default_llm() -> ModelSpec {
    ModelSpec {
      role: "chat".into(),
      repo_id: LLM_REPO.into(),
      filename: LLM_FILE.into(),
      revision: "main".into(),
      checksum: None,
    }
  }
}
