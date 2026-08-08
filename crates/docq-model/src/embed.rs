use std::sync::Mutex;

use docq_core::{EmbedError, Embedder, ModelError, ModelSpec, Result};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use crate::ModelHub;

pub struct FastEmbedEmbedder {
  inner: Mutex<TextEmbedding>,
  model_name: String,
  dim: usize,
}

impl FastEmbedEmbedder {
  pub async fn from_model_hub(hub: &ModelHub, spec: &ModelSpec) -> Result<Self> {
    let model = embedding_model_for(&spec.repo_id)?;
    let dim = TextEmbedding::get_model_info(&model).map_err(|e| ModelError::Other(e.to_string()))?.dim;
    let options = TextInitOptions::new(model).with_cache_dir(hub.cache_dir().to_path_buf());
    let inner = TextEmbedding::try_new(options).map_err(|e| ModelError::Other(e.to_string()))?;
    Ok(Self {
      inner: Mutex::new(inner),
      model_name: spec.repo_id.clone(),
      dim,
    })
  }
}

fn embedding_model_for(repo_id: &str) -> Result<EmbeddingModel> {
  match repo_id {
    "BAAI/bge-small-zh-v1.5" => Ok(EmbeddingModel::BGESmallZHV15),
    "BAAI/bge-large-zh-v1.5" => Ok(EmbeddingModel::BGELargeZHV15),
    "BAAI/bge-m3" => Ok(EmbeddingModel::BGEM3),
    other => Err(ModelError::Other(format!("unsupported embedding model: {other}")).into()),
  }
}

#[async_trait::async_trait]
impl Embedder for FastEmbedEmbedder {
  fn dimension(&self) -> usize {
    self.dim
  }

  fn model_name(&self) -> &str {
    &self.model_name
  }

  async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let mut inner = self.inner.lock().map_err(|_| EmbedError::Other("mutex poisoned".into()))?;
    let embeddings = inner.embed(texts, None).map_err(|e| EmbedError::Other(e.to_string()))?;
    Ok(embeddings)
  }
}
