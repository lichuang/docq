//! Model registry, download/cache, and inference backends.

pub mod embed;
pub mod hub;
pub mod registry;

pub use embed::FastEmbedEmbedder;
pub use hub::ModelHub;
pub use registry::ModelRegistry;

#[cfg(test)]
mod tests {
  use super::*;
  use docq_core::{Embedder, ModelSpec, Storage};
  use docq_storage::SqliteStorage;
  use std::fs;
  use tempfile::TempDir;

  fn empty_storage() -> SqliteStorage {
    let s = SqliteStorage::open_in_memory().unwrap();
    s.init().unwrap();
    s
  }

  fn seed_cache_file(cache_dir: &std::path::Path, spec: &ModelSpec, content: &str) {
    let repo_dir = cache_dir.join(format!("models--{}", spec.repo_id.replace('/', "--")));
    let commit = "fakecommit";
    let snapshot_dir = repo_dir.join("snapshots").join(commit);
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&snapshot_dir).unwrap();
    fs::create_dir_all(&refs_dir).unwrap();
    fs::write(snapshot_dir.join(&spec.filename), content).unwrap();
    fs::write(refs_dir.join(&spec.revision), commit).unwrap();
  }

  #[tokio::test]
  async fn test_registry_defaults() {
    let emb = ModelRegistry::default_embedding();
    assert_eq!(emb.role, "embedding");
    assert_eq!(emb.repo_id, "BAAI/bge-small-zh-v1.5");

    let rnk = ModelRegistry::default_reranker();
    assert_eq!(rnk.role, "reranker");

    let llm = ModelRegistry::default_llm();
    assert_eq!(llm.role, "chat");
    assert!(llm.filename.ends_with(".gguf"));
  }

  #[tokio::test]
  async fn test_ensure_cache_hit() {
    let tmp = TempDir::new().unwrap();
    let storage = empty_storage();
    let hub = ModelHub::new(tmp.path().to_path_buf());

    let spec = ModelRegistry::default_embedding();
    seed_cache_file(tmp.path(), &spec, "fake model bytes");

    let path = hub.ensure(&spec, &storage).await.unwrap();
    assert!(path.exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), "fake model bytes");

    let recorded = storage.get_model_version("embedding").unwrap().unwrap();
    assert_eq!(recorded.repo_id, spec.repo_id);
  }

  #[tokio::test]
  #[ignore = "requires network; run with cargo test -- --ignored"]
  async fn test_ensure_real_download_embedding() {
    let tmp = TempDir::new().unwrap();
    let storage = empty_storage();
    let hub = ModelHub::new(tmp.path().to_path_buf());

    let spec = ModelRegistry::default_embedding();
    let path = hub.ensure(&spec, &storage).await.unwrap();
    assert!(path.exists());
  }

  #[tokio::test]
  #[ignore = "requires network; run with cargo test -- --ignored"]
  async fn test_embedder_embed() {
    let tmp = TempDir::new().unwrap();
    let hub = ModelHub::new(tmp.path().to_path_buf());
    let spec = ModelRegistry::default_embedding();

    let embedder = FastEmbedEmbedder::from_model_hub(&hub, &spec).await.unwrap();
    assert_eq!(embedder.dimension(), 512);
    assert_eq!(embedder.model_name(), "BAAI/bge-small-zh-v1.5");

    let texts = vec!["你好".to_string(), "世界".to_string()];
    let embs = embedder.embed(&texts).await.unwrap();
    assert_eq!(embs.len(), 2);
    assert_eq!(embs[0].len(), 512);
  }
}
