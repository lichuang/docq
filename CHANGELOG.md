## [0.1.0] - 2026-08-13

### 🚀 Features

- Scaffold workspace with docq-core types, traits, and error taxonomy
- Implement SQLite-backed Storage with documents, chunks, and model versions CRUD
- Integrate sqlite-vec and FTS5 vector and full-text search into SqliteStorage
- Add ModelRegistry defaults and ModelHub with HuggingFace download/cache and model version recording
- Add FastEmbedEmbedder with ModelSpec-driven model selection and TextReader for recursive file scanning
- Add SentenceSplitter, jieba WordSegmenter, and Indexer with incremental reindex and cascade delete
- Add Retriever with BM25 and vector recall fused via RRF with jieba-segmented query and unified score direction
- Add cross-encoder reranker with optional rerank integration in Retriever and centralize model repo constants
- Add GgufLlm with LlmConfig for local GGUF inference with configurable sampling parameters
- Add Synthesizer with prompt builder, citation parser, and LLM answer generation grounded in retrieved chunks
- Add Engine facade with DI components, collection persistence, model version recording, and model-constrained chunk sizing
- *(cli)* Implement lazy-loading Engine facade and interactive CLI
- *(config)* Add workspace TOML config and platform-aware default directories
- *(cli)* Ensure the global default config.toml is created and loaded from the system config directory on every command, independent of the workspace directory
- *(cli)* Support custom config files via --config/-c and auto-create the global default config.toml on every command
- *(cli)* Support custom config files via --config/-c and auto-create the global default config.toml on every command
- *(cli)* Add -v/--verbose global flag and per-step timing output for index, search and ask
- *(indexer)* Abstract FileReader trait and ReaderRegistry dispatcher for pluggable PDF/DOC/text readers; refactor(core): move DocumentSource to docq-core for cross-crate reuse
- *(indexer)* Add PDF support via pdf-extract behind the `pdf` feature and register PdfReader in default readers; doc(testdata): reorganize bundled example docs into testdata/md and add a sample PDF
- *(indexer)* Add DOCX reader with zip + quick-xml

### 🐛 Bug Fixes

- *(retrieve)* Quote FTS5 query tokens to avoid syntax errors on hyphens and special characters, add verbose index progress and timing, and document the bundled testdata/en example
- *(model)* Raise BGE-small-zh max tokens from 512 to 1024; fix(model): align llama.cpp n_batch with n_ctx and truncate over-budget prompts to prevent GGML_ASSERT aborts on large chunks

### 🚜 Refactor

- Split Storage into read-only Storage and write-only StorageTx with all mutations flowing through transactions
- Flatten deep-path type references to top-level imports and document the convention in AGENTS.md
- *(model)* Rename default model registry constants to reflect model and file names, and prepare workspace crates for crates.io publishing
- *(indexer)* Batch embedding calls up to 500 chunks and extract prepare_file helper to reduce ONNX invocation overhead; doc(readme): polish bundled testdata attribution and grammar
- *(indexer)* Reorganize reader module by moving TextFileReader to reader/txt_reader.rs and ReaderRegistry to reader_registry.rs for clearer separation of format readers and dispatch logic

### ⚙️ Miscellaneous Tasks

- *(release)* Prepare all workspace crates for crates.io with required metadata, versioned internal dependencies, a version-aware publish script, and fix clippy warnings
