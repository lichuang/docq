use std::num::NonZeroU32;

use docq_core::{Llm, LlmConfig, LlmError, ModelSpec, Result};
use encoding_rs::UTF_8;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use crate::ModelHub;

pub struct GgufLlm {
  backend: LlamaBackend,
  model: LlamaModel,
  chat_template: LlamaChatTemplate,
  config: LlmConfig,
}

impl GgufLlm {
  pub async fn from_model_hub(hub: &ModelHub, spec: &ModelSpec, config: &LlmConfig) -> Result<Self> {
    let path = hub.resolve(spec).await?;

    let mut backend = LlamaBackend::init().map_err(|e| LlmError::Other(format!("init backend: {e}")))?;
    backend.void_logs();
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, &path, &model_params)
      .map_err(|e| LlmError::Other(format!("load model: {e}")))?;
    let chat_template = model.chat_template(None).map_err(|e| LlmError::Other(format!("get chat template: {e}")))?;

    Ok(Self {
      backend,
      model,
      chat_template,
      config: config.clone(),
    })
  }
}

#[async_trait::async_trait]
impl Llm for GgufLlm {
  async fn complete(&self, prompt: &str) -> Result<String> {
    let messages = [
      LlamaChatMessage::new("system".into(), "You are a helpful assistant.".into())
        .map_err(|e| LlmError::Other(format!("create system message: {e}")))?,
      LlamaChatMessage::new("user".into(), prompt.to_string())
        .map_err(|e| LlmError::Other(format!("create user message: {e}")))?,
    ];

    let formatted = self
      .model
      .apply_chat_template(&self.chat_template, &messages, true)
      .map_err(|e| LlmError::Other(format!("apply chat template: {e}")))?;

    let tokens = self
      .model
      .str_to_token(&formatted, AddBos::Always)
      .map_err(|e| LlmError::Other(format!("tokenize: {e}")))?;

    let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(self.config.n_ctx));
    let mut ctx = self
      .model
      .new_context(&self.backend, ctx_params)
      .map_err(|e| LlmError::Other(format!("create context: {e}")))?;

    let mut batch = LlamaBatch::new(tokens.len() + self.config.max_tokens, 1);
    for (i, token) in tokens.iter().enumerate() {
      batch
        .add(*token, i as i32, &[0], i == tokens.len() - 1)
        .map_err(|e| LlmError::Other(format!("batch add: {e}")))?;
    }

    ctx.decode(&mut batch).map_err(|e| LlmError::Other(format!("decode: {e}")))?;

    let mut sampler = LlamaSampler::chain_simple([
      LlamaSampler::temp(self.config.temperature),
      LlamaSampler::top_p(self.config.top_p, 1),
      LlamaSampler::dist(self.config.seed),
    ]);

    let mut output = String::new();
    let mut decoder = UTF_8.new_decoder();

    for step in 0..self.config.max_tokens {
      let pos = tokens.len() as i32 + step as i32;
      let token = sampler.sample(&ctx, batch.n_tokens() - 1);

      if self.model.is_eog_token(token) {
        break;
      }

      let piece = self
        .model
        .token_to_piece(token, &mut decoder, true, None)
        .map_err(|e| LlmError::Other(format!("detokenize: {e}")))?;
      output.push_str(&piece);

      batch.clear();
      batch
        .add(token, pos, &[0], true)
        .map_err(|e| LlmError::Other(format!("batch add generated: {e}")))?;

      ctx.decode(&mut batch).map_err(|e| LlmError::Other(format!("decode generated: {e}")))?;
    }

    Ok(output)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ModelRegistry;
  use docq_core::Llm;
  use tempfile::TempDir;

  #[tokio::test]
  #[ignore = "requires network + ~4.5GB model download; run with cargo test -- --ignored"]
  async fn test_llm_complete() {
    let tmp = TempDir::new().unwrap();
    let hub = ModelHub::new(tmp.path().to_path_buf());
    let spec = ModelRegistry::default_llm();

    let llm = GgufLlm::from_model_hub(&hub, &spec, &LlmConfig::default()).await.unwrap();
    let output = llm.complete("What is 2+3? Answer briefly.").await.unwrap();
    assert!(!output.is_empty());
    assert!(output.contains("5") || output.contains("五"));
  }
}
