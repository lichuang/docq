use std::path::{Path, PathBuf};

use docq_core::{ModelError, ModelSpec, Result, Storage};
use hf_hub::api::sync::ApiBuilder;
use hf_hub::{Repo, RepoType};

pub struct ModelHub {
  cache_dir: PathBuf,
}

impl ModelHub {
  pub fn new(cache_dir: PathBuf) -> Self {
    Self { cache_dir }
  }

  pub fn cache_dir(&self) -> &Path {
    &self.cache_dir
  }

  pub async fn ensure(&self, spec: &ModelSpec, storage: &dyn Storage) -> Result<PathBuf> {
    let api = ApiBuilder::new()
      .with_cache_dir(self.cache_dir.clone())
      .with_progress(false)
      .build()
      .map_err(|e| ModelError::Other(e.to_string()))?;

    let repo = Repo::with_revision(spec.repo_id.clone(), RepoType::Model, spec.revision.clone());
    let path = api.repo(repo).get(&spec.filename).map_err(|e| ModelError::Other(e.to_string()))?;

    let mut tx = storage.begin_tx()?;
    tx.set_model_version(&spec.role, spec)?;
    tx.commit()?;

    Ok(path)
  }
}
