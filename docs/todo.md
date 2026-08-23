# docq 优化清单

> 来源：2026-08-22 全代码库审查。按优先级排列。

## 一、Bug / 正确性问题（优先修复）

### P0-1. ~~`Storage::init` 违反 trait 契约 — dimension=0 应跳过建表~~ ✅ 已完成

- 文件：`crates/docq-storage/src/sqlite.rs`
- 改动：`init(0)` 不再报错，跳过 `init_vectors` 直接返回 Ok，符合 trait 文档约定。

### P0-2. ~~`docq add` 硬编码 dimension 初始化 vec_chunks~~ ✅ 已完成

- 文件：`crates/docq/src/main.rs`
- 改动：`run_init` 和 `open_storage` 改为 `init(0)`（只建基础表，不建 vec_chunks）。删除 `BGE_SMALL_ZH_V1_5_DIMENSION` import。后续 `index` 命令通过 `Engine::open_for_index` 用真实 embedding 维度调 `init(dimension)` 建 vec_chunks。

### P0-3. ~~跨文件相同文本的 chunk dedup 丢失 doc_id 关联~~ ✅ 已完成

- 文件：`crates/docq-storage/src/sqlite.rs`、`crates/docq-core/src/traits.rs`、`crates/docq-indexer/src/lib.rs`
- 改动：新增 `chunk_documents(chunk_id, doc_id)` 多对多关联表，从 `chunks` 表移除 `doc_id` 列。`insert_chunks` 改为 `INSERT OR IGNORE`（共享文本只存一次）。新增 `StorageTx::add_chunk_documents` 方法。`delete_chunks_by_doc` 改为从 `chunk_documents` 删除关联，仅当 chunk 无任何文档引用时才清理 `chunks`/`vec_chunks`/`fts_chunks`。`get_chunks` 通过子查询从 `chunk_documents` 填充 `doc_id`。新增测试 `test_shared_chunk_survives_deleting_one_doc` 验证共享 chunk 在删除一个文档后仍存在。`cargo test -p docq-storage` 10/10 通过。

### P1-4. ~~SQLite 外键未启用 — `ON DELETE CASCADE` 形同虚设~~ ✅ 已完成

- 文件：`crates/docq-storage/src/sqlite.rs`
- 改动：`open` 和 `open_in_memory` 的 PRAGMA 批处理中增加 `PRAGMA foreign_keys = ON`。

### P1-5. ~~无增量删除 — 已删除文件不会从索引中移除~~ ✅ 已完成

- 文件：`crates/docq-indexer/src/lib.rs`、`crates/docq/src/main.rs`
- 改动：`index_directory` 新增 tombstone sweep — 在索引前先 `list_documents()` + `get_document_paths()`，对比磁盘实际文件，删除不再存在的文档（通过 `delete_document` + `delete_chunks_by_doc`）。`IndexStats` 新增 `files_removed` 字段，CLI 输出显示 removed 计数。新增测试 `test_index_directory_removes_deleted_files`。

### P1-6. ~~`embedding_to_bytes` 用 native endian — 跨架构 DB 不可移植~~ ✅ 已完成

- 文件：`crates/docq-storage/src/sqlite.rs`
- 改动：`embedding_to_bytes` 从 `to_ne_bytes()` 改为 `to_le_bytes()`，确保 DB 文件跨架构一致。

## 二、性能优化

### P1-7. ~~BM25 和 Vector recall 串行，可并行~~ ✅ 已完成

- 文件：`crates/docq-retrieve/src/retriever.rs`
- 改动：将 BM25 抽为 `async fn bm25_recall`，内部用 `tokio::task::spawn_blocking` 在阻塞线程跑同步的 jieba 分词 + FTS5 查询，`search` 中用 `tokio::join!` 与 `vector_recall` 并行执行。

### P2-8. ~~未启用 WAL 模式 — 读写互斥~~ ✅ 已完成

- 文件：`crates/docq-storage/src/sqlite.rs`
- 改动：`open` 的 PRAGMA 批处理中增加 `PRAGMA journal_mode = WAL`（`open_in_memory` 不加 — 内存数据库无文件 WAL）。

### P2-9. ~~`SentenceSplitter::token_count` 重复计算~~ ✅ 已完成

- 文件：`crates/docq-indexer/src/chunker.rs`
- 改动：`chunk()` 中 units 从 `Vec<String>` 改为 `Vec<(String, usize)>`，分割阶段计算 token_count 并缓存，构建 chunk 和计算 overlap 时直接复用缓存值，消除重复 tokenizer encode 调用。

### P2-10. ~~`prepare_file` 对每个文件单独查 DB 检查 content_hash~~ ✅ 已完成

- 文件：`crates/docq-indexer/src/indexer.rs`
- 改动：`index_directory` 和 `index_file` 预先 `list_documents()` 一次构建 `HashMap<String, Document>`，传给 `prepare_file` 在内存中比对 content_hash。`sweep_deleted` 也复用同一份 `list_documents()` 结果，避免重复查询。

### P2-11. ~~`flush_batch` 内对每个文件重复查 `get_document`~~ ✅ 已完成

- 文件：`crates/docq-indexer/src/indexer.rs`
- 改动：`PendingFile` 新增 `is_update: bool` 字段，由 `prepare_file` 在内存比对时设置。`flush_batch` 根据 `is_update` 决定是否调 `delete_chunks_by_doc`，不再对每个文件查 `get_document`。

### P3-12. 每次 CLI 调用都重新加载全部模型

- 文件：`crates/docq/src/engine.rs`
- 现状：`search` 加载 ~1.1GB，`ask` 加载 ~6GB，每次冷启动。最大 UX 瓶颈。
- 修复：daemon/server 模式或模型常驻进程（MCP server 是对接点）。

### P1-22. ~~模型加载串行 — reranker 和 LLM 可并行加载~~ ✅ 已完成

- 文件：`crates/docq/src/engine.rs`、`crates/docq-model/src/{hub,rerank,gguf}.rs`
- 改动：`build_ask_components` 改为两阶段 — Phase 1 串行加载 embedding + `storage.init(dimension)`，Phase 2 用 `spawn_blocking` + `tokio::join!` 并行加载 reranker (~1.1GB) 和 LLM (~6GB)。给 `ModelHub` 加 `Clone` + `resolve_sync`/`ensure_sync`，给 `FastEmbedReranker`/`GgufLlm` 加 `from_model_hub_sync`，删除不再使用的 async `load_llm`。

## 三、设计 / API 改进

### P2-13. ~~所有错误类型都是 `Other(String)` — 无法结构化匹配~~ ✅ 已完成

- 文件：`crates/docq-core/src/error.rs` + 全 crate 60 个创建点
- 改动：将 7 个 error enum 从单一 `Other(String)` 改为结构化变体。`StoreError` → `Sqlite`/`MutexPoisoned`/`InvalidDimension`/`SchemaMismatch`/`TransactionAlreadyCommitted`/`ArgumentMismatch`/`Io`/`NotFound`/`InvalidTimestamp`；`EmbedError` → `EmptyResult`/`MutexPoisoned`/`InferenceFailed`；`RetrieveError` → `TaskJoin`/`MutexPoisoned`/`RerankFailed`；`LlmError` → `BackendInit`/`ModelLoad`/`InferenceFailed`/`NotLoaded`/`InvalidConfig`/`TokenizerLoad`；`ModelError` → `ModelInitFailed`/`ModelInfoFailed`/`UnsupportedModel`/`DownloadFailed`/`HubApiFailed`/`TaskJoin`；`ParseError` → `Io`/`ExtractFailed`/`ZipFailed`/`ZipEntryMissing`/`XmlParseFailed`。`SynthError` 保留 `Other`（无创建点）。`cargo check --workspace` + `clippy -D warnings` + `fmt --check` 全通过。

### P2-14. ~~`ModelSpec.role` 是 String — 应为 enum~~ ✅ 已完成

- 文件：`crates/docq-core/src/models.rs`、`crates/docq-core/src/traits.rs`、`crates/docq-storage/src/sqlite.rs`、`crates/docq-model/src/{hub,registry,lib}.rs`、`crates/docq/src/{config,engine}.rs`
- 改动：新增 `ModelRole` enum（`Embedding`/`Reranker`/`Chat`/`Tokenizer`），实现 `as_str`/`Display`/`FromStr`/`Serialize`/`Deserialize`。`ModelSpec.role` 从 `String` 改为 `ModelRole`（`Copy`）。`Storage` trait 的 `get_model_version`/`set_model_version` 签名从 `&str` 改为 `ModelRole`。DB 存储仍用 `as_str()` 写入字符串列，读出时用 `role` 参数直接回填。`config.rs` 的 `to_spec` 参数从 `&str` 改为 `ModelRole`。全部 67 个测试通过。

### P3-15. `ask` 硬编码 top-5 检索

- 文件：`crates/docq-synth/src/synthesizer.rs:44`
- 现状：`self.retriever.search(query, 5).await?`，5 是写死的。
- 修复：加入 `SynthesizerConfig` 或 config.toml 让用户可调。

### P3-16. `LlmConfig` 的 f32→String→f32 往返

- 文件：`crates/docq/src/config.rs:57-67`
- 现状：`temperature`/`top_p` 存为 String 避免序列化精度问题，再解析回来。
- 修复：用 `#[serde(with = ...)]` 或自定义 newtype 更干净。

### P2-17. ~~`build_chunker` 硬编码 tokenizer 文件名~~ ✅ 已完成

- 文件：`crates/docq/src/config.rs`、`crates/docq/src/engine.rs`
- 改动：`ModelEntry` 新增 `tokenizer_filename` 字段（`#[serde(default)]` 向后兼容旧 config.toml）。`build_chunker` 和 `load_embedding` 改为接收 `tokenizer_filename` 参数，从 `engine_config.config.models.embedding.tokenizer_filename` 传入。删除 `engine.rs` 中对 `BGE_SMALL_ZH_V1_5_TOKENIZER_FILE` 常量的硬编码引用。

### P2-18. ~~`SentenceSplitter::split_paragraphs` 用三个换行 `\n\n\n`~~ ✅ 已确认无需修改

- 文件：`crates/docq-indexer/src/chunker.rs:24`
- 结论：LlamaIndex 的 `SentenceSplitter` 测试用例（`test_sentence_splitter.py::test_paragraphs`）明确使用 `\n\n\n` 分隔段落，当前实现与 LlamaIndex 行为一致，不是 bug。

## 四、Minor

### P3-19. `ScoreExplain` 的 `vector_score` 方向转换散落在 retriever 中

- 文件：`crates/docq-retrieve/src/retriever.rs:259`
- 现状：`1.0 - dist` 转换散落在 retriever，sqlite-vec 语义变化需在此改。
- 修复：转换逻辑封装在 storage 层。

### P3-20. `IndexStats::merge` 可用 `Add` trait

- 文件：`crates/docq-indexer/src/lib.rs:56-61`
- 现状：手动 merge。
- 修复：实现 `std::ops::Add` 更符合 Rust 惯例。

### P3-21. 缺少 `Storage` trait 并发测试

- 文件：`crates/docq-storage/src/sqlite.rs`
- 现状：`Arc<Mutex<Connection>>` 序列化所有操作，但未测试多线程下不死锁/不 panic。library 用户可能并发调 `search`。
- 修复：加多线程并发读写测试。

## 五、Ask 性能优化（来自实测 profiling）

> 基于 M1 Max + Metal GPU，ask 总耗时 11.9 秒（检索 ~4s + LLM ~7.8s）。

### P3-23. 降低 `rerank_top_n`（已搁置 — 回答质量下降，已回滚）

- 尝试：`rerank_top_n` 从 20 改为 10，rerank 时间减半但回答质量明显下降，已回滚。

### P3-24. jieba 首次初始化冷启动

- 现状：jieba 首次调用 794ms（`OnceLock` 加载词典），纯英文查询也无谓走 jieba。
- 备注：第二次查询已快，无法完全消除。可考虑英文查询跳过 jieba 分词。

### P3-25. ~~LLM prompt 截断~~ ✅ 已完成

- 文件：`crates/docq/src/config.rs`、`crates/docq-core/src/models.rs`
- 改动：`n_ctx` 默认值从 4096 改为 8192（`LlmGenerationConfig` 和 `LlmConfig` 两处），消除 prompt 截断。生成速度不受影响，仅增加 KV cache 内存占用。

### P3-26. 降低 LLM `max_tokens`

- 现状：`max_tokens = 512`，LLM 生成 7.8 秒。
- 修复：降低到 256 可线性缩短到 ~4 秒，但可能截断长答案。

### P3-27. 换更小的 LLM

- 现状：Qwen2.5-3B q4_k_m（~1.9GB），生成 7.8 秒。
- 修复：Qwen2.5-1.5B 或 Qwen2.5-0.5B，速度 2-4 倍提升，质量下降。

### P3-28. ~~换更快的 reranker~~ ✅ 已完成

- 文件：`~/.config/docq/config.toml`
- 改动：reranker 从 `BAAI/bge-reranker-base` 换为 `jinaai/jina-reranker-v1-turbo-en`。rerank 从 3144ms → 731ms（4.3 倍），ask 总耗时从 11.9s → 6.5s。回答质量可接受。

### P4-29. 智能模型选择策略

- 现状：所有模型在 config.toml 中手动配置，新用户默认用一套固定配置，不区分语言和硬件。
- 目标：根据用户环境自动推荐/选择模型组合：
  - **文档语言**：中文文档 → BGE reranker（多语言）+ BGE-zh embedding；英文文档 → jina-turbo reranker + BGE-en embedding
  - **硬件配置**：8GB RAM → Qwen2.5-0.5B（~0.5GB）；16GB → Qwen2.5-3B（~1.9GB）；有 Metal/CUDA → 可用更大模型
  - **使用场景**：纯 search → 只加载 embedding + reranker；ask → 全量加载含 LLM
- 备注：属于产品层面优化，config.toml 已支持用户手动配置，此为自动化方向。

### P4-30. 多平台 GPU 加速二进制发布

- 现状：CI 只发布 CPU 版本。macOS 编译时自动启用 Metal，Windows/Linux 用户只能从源码编译启用 Vulkan/CUDA。
- 目标：为 Windows/Linux 提供预编译的 GPU 加速版本（Vulkan 通用版 + CUDA NVIDIA 专用版）。
- 难点：CI runner 无 GPU，需要安装 Vulkan SDK / CUDA Toolkit，编译时间长，需要多矩阵 job。
- 临时方案：README 已说明用户如何 `cargo install --features` 本地编译。

## 六、已有的其他 todo

- index by multi thread
- use lancedb for vector storage/search