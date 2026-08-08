use thiserror::Error;

#[derive(Debug, Error)]
pub enum DocqError {
  #[error("parse error: {0}")]
  Parse(#[from] ParseError),

  #[error("store error: {0}")]
  Store(#[from] StoreError),

  #[error("embed error: {0}")]
  Embed(#[from] EmbedError),

  #[error("retrieve error: {0}")]
  Retrieve(#[from] RetrieveError),

  #[error("synth error: {0}")]
  Synth(#[from] SynthError),

  #[error("llm error: {0}")]
  Llm(#[from] LlmError),

  #[error("model error: {0}")]
  Model(#[from] ModelError),
}

pub type Result<T> = std::result::Result<T, DocqError>;

#[derive(Debug, Error)]
pub enum ParseError {
  #[error("{0}")]
  Other(String),
}

#[derive(Debug, Error)]
pub enum StoreError {
  #[error("{0}")]
  Other(String),
}

#[derive(Debug, Error)]
pub enum EmbedError {
  #[error("{0}")]
  Other(String),
}

#[derive(Debug, Error)]
pub enum RetrieveError {
  #[error("{0}")]
  Other(String),
}

#[derive(Debug, Error)]
pub enum SynthError {
  #[error("{0}")]
  Other(String),
}

#[derive(Debug, Error)]
pub enum LlmError {
  #[error("{0}")]
  Other(String),
}

#[derive(Debug, Error)]
pub enum ModelError {
  #[error("{0}")]
  Other(String),
}
