use docq_core::ModelSpec;

// ---- Default models ----

pub const EMBEDDING_REPO: &str = "Xenova/bge-small-zh-v1.5";
pub const EMBEDDING_FILE: &str = "onnx/model.onnx";
pub const RERANKER_REPO: &str = "BAAI/bge-reranker-base";
pub const RERANKER_FILE: &str = "onnx/model.onnx";
pub const LLM_REPO: &str = "Qwen/Qwen2.5-7B-Instruct-GGUF";
pub const LLM_FILE: &str = "qwen2.5-7b-instruct-q4_k_m.gguf";

// ---- All supported embedding repos (for repo_id → fastembed mapping) ----

pub const EMBEDDING_REPO_BGE_LARGE_ZH: &str = "Xenova/bge-large-zh-v1.5";
pub const EMBEDDING_REPO_BGE_M3: &str = "BAAI/bge-m3";

// ---- All supported reranker repos (for repo_id → fastembed mapping) ----

pub const RERANKER_REPO_BGE_V2_M3: &str = "BAAI/bge-reranker-v2-m3";
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
