# docq 优化清单

> 来源：2026-08-22 全代码库审查。按优先级排列。

## 一、Bug / 正确性问题（优先修复）

### P0-1. `Storage::init` 违反 trait 契约 — dimension=0 应跳过建表

- 文件：`crates/docq-storage/src/sqlite.rs:201-203`
- 现状：`traits.rs:54-56` 文档说 "Pass `0` to skip creating the vector table when no embedder is available yet."，但实现中 dimension==0 直接报错。
- 修复：dimension==0 时跳过 `init_vectors` 调用，直接返回 Ok。

### P0-2. `docq add` 硬编码 dimension 初始化 vec_chunks

- 文件：`crates/docq/src/main.rs:414-418` (`open_storage`)
- 现状：`add` 命令始终用 `BGE_SMALL_ZH_V1_5_DIMENSION`(512) 调 `init`。如果用户配置了 dim=1024 的模型（如 BGE-M3），`add` 先建了 `FLOAT[512]` 表，之后 `index` 调 `init(1024)` → dimension mismatch → 报错，无法索引。
- 修复：`add` 不需要向量表，应传 0（配合 P0-1）或完全不调 `init`。

### P0-3. ~~跨文件相同文本的 chunk dedup 丢失 doc_id 关联~~ ✅ 已完成

- 文件：`crates/docq-storage/src/sqlite.rs`、`crates/docq-core/src/traits.rs`、`crates/docq-indexer/src/lib.rs`
- 改动：新增 `chunk_documents(chunk_id, doc_id)` 多对多关联表，从 `chunks` 表移除 `doc_id` 列。`insert_chunks` 改为 `INSERT OR IGNORE`（共享文本只存一次）。新增 `StorageTx::add_chunk_documents` 方法。`delete_chunks_by_doc` 改为从 `chunk_documents` 删除关联，仅当 chunk 无任何文档引用时才清理 `chunks`/`vec_chunks`/`fts_chunks`。`get_chunks` 通过子查询从 `chunk_documents` 填充 `doc_id`。新增测试 `test_shared_chunk_survives_deleting_one_doc` 验证共享 chunk 在删除一个文档后仍存在。`cargo test -p docq-storage` 10/10 通过。

### P1-4. SQLite 外键未启用 — `ON DELETE CASCADE` 形同虚设

- 文件：`crates/docq-storage/src/sqlite.rs`
- 现状：从未执行 `PRAGMA foreign_keys = ON`。`delete_document` 手动先删 `document_paths` 再删 `documents` 绕过，但直接删 `documents` 行不会级联清理。
- 修复：open 时执行 `PRAGMA foreign_keys = ON`。

### P1-5. 无增量删除 — 已删除文件不会从索引中移除

- 文件：`crates/docq-indexer/src/lib.rs` (`Indexer::index_directory`)
- 现状：只处理目录中存在的文件，磁盘删除的文件不会清理 documents/chunks/vectors/fts。
- 修复：增加 tombstone sweep — 对比 `list_documents()` 与实际文件，删除消失的文档。

### P1-6. `embedding_to_bytes` 用 native endian — 跨架构 DB 不可移植

- 文件：`crates/docq-storage/src/sqlite.rs:28-34`
- 现状：`to_ne_bytes()`，DB 文件从 x86 拷到 ARM 向量字节序错误，余弦距离全错。
- 修复：改用 `to_le_bytes()`。

## 二、性能优化

### P1-7. ~~BM25 和 Vector recall 串行，可并行~~ ✅ 已完成

- 文件：`crates/docq-retrieve/src/retriever.rs`
- 改动：将 BM25 抽为 `async fn bm25_recall`，内部用 `tokio::task::spawn_blocking` 在阻塞线程跑同步的 jieba 分词 + FTS5 查询，`search` 中用 `tokio::join!` 与 `vector_recall` 并行执行。

### P2-8. 未启用 WAL 模式 — 读写互斥

- 文件：`crates/docq-storage/src/sqlite.rs:46`
- 现状：只设了 `busy_timeout`，无 `PRAGMA journal_mode=WAL`。写事务阻塞所有读。
- 修复：open 时执行 `PRAGMA journal_mode=WAL`。

### P2-9. `SentenceSplitter::token_count` 重复计算

- 文件：`crates/docq-indexer/src/chunker.rs:100-102` 和 `114-116`
- 现状：构建 chunk 和计算 overlap 时对同一 unit 重复调 tokenizer encode。
- 修复：缓存 `(unit → token_count)` 或首次计算时存入 struct。

### P2-10. `prepare_file` 对每个文件单独查 DB 检查 content_hash

- 文件：`crates/docq-indexer/src/lib.rs:154-158`
- 现状：大目录（1000+ 文件）N 次独立查询。
- 修复：预先 `list_documents()` 一次拿到所有 hash，内存中比对。

### P2-11. `flush_batch` 内对每个文件重复查 `get_document`

- 文件：`crates/docq-indexer/src/lib.rs:208`
- 现状：`prepare_file` 已知文件是否存在（通过 content_hash），但信息未传递到 `flush_batch`，多一次 DB 往返。
- 修复：在 `PendingFile` 中加 `is_update: bool` 字段，从 `prepare_file` 传递。

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

### P2-14. `ModelSpec.role` 是 String — 应为 enum

- 文件：`crates/docq-core/src/models.rs:74-81`
- 现状：`role: String`，取值 "embedding"/"reranker"/"chat"，容易拼写错误。
- 修复：定义 `enum ModelRole { Embedding, Reranker, Chat }`。

### P3-15. `ask` 硬编码 top-5 检索

- 文件：`crates/docq-synth/src/synthesizer.rs:44`
- 现状：`self.retriever.search(query, 5).await?`，5 是写死的。
- 修复：加入 `SynthesizerConfig` 或 config.toml 让用户可调。

### P3-16. `LlmConfig` 的 f32→String→f32 往返

- 文件：`crates/docq/src/config.rs:57-67`
- 现状：`temperature`/`top_p` 存为 String 避免序列化精度问题，再解析回来。
- 修复：用 `#[serde(with = ...)]` 或自定义 newtype 更干净。

### P2-17. `build_chunker` 硬编码 tokenizer 文件名

- 文件：`crates/docq/src/engine.rs:133`
- 现状：即使配了 BGE-M3，也从 M3 的 repo 下 `BGE_SMALL_ZH_V1_5_TOKENIZER_FILE`("tokenizer.json")。隐式耦合，未来加无 tokenizer.json 的模型会静默失败。
- 修复：让 `ModelSpec` 或 `ModelEntry` 携带 tokenizer filename，或统一约定。

### P2-18. `SentenceSplitter::split_paragraphs` 用三个换行 `\n\n\n`

- 文件：`crates/docq-indexer/src/chunker.rs:24`
- 现状：`text.split("\n\n\n")`，LlamaIndex 的 SentenceSplitter 默认用双换行 `\n\n`。本该分段的内容不分段，影响 chunk 边界质量。
- 修复：确认是否有意为之；若非，改为 `\n\n`。

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

## 五、已有的其他 todo

- index by multi thread
- use lancedb for vector storage/search