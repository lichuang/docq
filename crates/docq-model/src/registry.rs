use docq_core::ModelSpec;

pub struct ModelRegistry;

impl ModelRegistry {
  pub fn default_embedding() -> ModelSpec {
    ModelSpec {
      role: "embedding".into(),
      repo_id: "BAAI/bge-small-zh-v1.5".into(),
      filename: "model.onnx".into(),
      revision: "main".into(),
      checksum: None,
    }
  }

  pub fn default_reranker() -> ModelSpec {
    ModelSpec {
      role: "reranker".into(),
      repo_id: "BAAI/bge-reranker-base".into(),
      filename: "model.onnx".into(),
      revision: "main".into(),
      checksum: None,
    }
  }

  pub fn default_llm() -> ModelSpec {
    ModelSpec {
      role: "chat".into(),
      repo_id: "Qwen/Qwen2.5-7B-Instruct-GGUF".into(),
      filename: "qwen2.5-7b-instruct-q4_k_m.gguf".into(),
      revision: "main".into(),
      checksum: None,
    }
  }
}
