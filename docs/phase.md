# docq v0.1 实现阶段划分

> 面向 coding agent 的逐步实现指南。每个阶段包含：目标、具体任务、验收标准、依赖关系。
> 基于 `docs/design.md` 的设计。

---

## 前置说明

**P0 技术验证已完成。** 用户已自行验证：

1. `llama-cpp-2` 可加载并稳定推理 `Qwen2.5-7B-Instruct-Q4_K_M.gguf`；
2. `fastembed` 可加载并推理 `BAAI/bge-small-zh-v1.5`；
3. `sqlite-vec` 与 `rusqlite bundled` 可集成并运行 top-k 查询；
4. `jieba-rs` 预分词 + FTS5 `unicode61` 的中文检索效果可接受。

因此本文档从 **P1 开始直接进入实现**。P0 仅作为验证记录保留，无需重复执行。

---

## 总览

| 阶段 | 目标 | 预计时间 | 前置依赖 |
|---|---|---|---|
| **P0** | ~~技术验证~~（已完成） | 0 天 | 无 |
| **P1** | Workspace + Core 类型/trait | 3~4 天 | 无 |
| **P2** | Storage trait + SQLite 基础 | 3~4 天 | P1 |
| **P3** | sqlite-vec + FTS5 | 3~4 天 | P2 |
| **P4** | ModelHub（下载/缓存/注册表） | 2~3 天 | P1 |
| **P5** | Embedder + 文本读取 | 3~4 天 | P4 |
| **P6** | Chunker + Indexer 流程 | 4~5 天 | P3, P5 |
| **P7** | BM25 + 向量召回 + RRF | 4~5 天 | P6 |
| **P8** | Reranker | 2~3 天 | P7 |
| **P9** | LLM 后端 | 3~4 天 | P1 |
| **P10** | Ask / Synthesis | 3~4 天 | P8, P9 |
| **P11** | Engine Facade | 2~3 天 | P10 |
| **P12** | CLI | 3~4 天 | P11 |
| **P13** | 集成测试 + 打磨 | 5~7 天 | P12 |

**总预计：9 ~ 13 周**（已扣除 P0 的 3~5 天）

---

## P0：技术验证（已完成，仅作记录）

### 目标

验证 4 个关键技术点，确认后续实现可行。所有验证已通过，结论为继续按原方案实现。

### P0.1 fastembed + BGE-small-zh-v1.5

**任务：**

1. 在 `poc/embed/` 创建临时 crate。
2. 依赖 `fastembed = "5"`。
3. 加载 `models/bge-small-zh-v1.5/` 下的 ONNX 模型。
4. 对以下文本生成 embedding：
   - "今天是我的生日"
   - "今天是他的生日"
   - "Rust 的所有权机制"
5. 计算两两 cosine similarity。

**验收标准：**

```rust
// 两个相似句子的相似度应 > 0.85
assert!(sim("今天是我的生日", "今天是他的生日") > 0.85);
// 不相似句子应 < 0.5
assert!(sim("今天是我的生日", "Rust 的所有权机制") < 0.5);
```

**输出：** `poc/embed/README.md` 记录加载代码、API 用法、注意事项。

### P0.2 llama-cpp-2 + Qwen2.5-7B

**任务：**

1. 在 `poc/llm/` 创建临时 crate。
2. 依赖 `llama-cpp-2 = "0.1"`。
3. 创建 `.cargo/config.toml`：
   ```toml
   [env]
   GGML_METAL = "OFF"
   ```
4. 加载 `models/qwen2.5-7b-instruct/qwen2.5-7b-instruct-q4_k_m.gguf`。
5. 使用 chat template 调用：
   - system: "You are a helpful assistant."
   - user: "什么是 Raft 算法？用中文简要回答。"
6. 记录加载时间、首次 token 时间、token/s。

**验收标准：**

- 能成功加载模型不崩溃。
- 生成 50~100 个中文字符，内容相关。
- macOS CPU 下 token/s > 5（可接受阈值）。

**输出：** `poc/llm/README.md` 记录代码、参数、性能数据。

### P0.3 sqlite-vec + rusqlite bundled

**任务：**

1. 在 `poc/vec/` 创建临时 crate。
2. 依赖 `rusqlite = { version = "0.40", features = ["bundled"] }` 和 `sqlite-vec`。
3. 打开 `:memory:` 数据库，加载 sqlite-vec 扩展。
4. 建表：
   ```sql
   CREATE VIRTUAL TABLE test_vec USING vec0(
       id TEXT PRIMARY KEY,
       embedding FLOAT[512]
   );
   ```
5. 插入 1000 条 512 维随机向量。
6. 查询 top-10。

**验收标准：**

- 能成功建表、插入、查询。
- top-k 结果按距离排序正确。
- 1000 条向量查询耗时 < 100ms。

**输出：** `poc/vec/README.md`。

### P0.4 jieba + FTS5

**任务：**

1. 在 `poc/fts/` 创建临时 crate。
2. 依赖 `jieba-rs` 和 `rusqlite`。
3. 建独立 FTS5 表：
   ```sql
   CREATE VIRTUAL TABLE test_fts USING fts5(id, text, tokenize='unicode61');
   ```
4. 对以下文本 jieba 分词后空格拼接再插入：
   - "分布式共识算法解决多个节点达成一致的问题"
   - "Raft 是一种易于理解的共识算法"
5. 查询 "共识算法" 时同样 jieba 分词后检索。

**验收标准：**

- 查询 "共识算法" 召回两条文档。
- 查询 "Raft" 只召回第二条。
- 不 jieba 分词直接查 "共识算法" 时，召回结果少于分词后。

**输出：** `poc/fts/README.md`。

### P0.5 验证结论

- 全部四个 spike 已通过，技术方案可行。
- 无需回退到 3B / Ollama HTTP 方案（除非运行环境内存 < 6GB）。
- 直接进入 P1 实现。

---

## P1：Workspace + Core 类型/trait

### 目标

搭建 workspace 和 crate 结构，定义所有核心类型与 trait。

### 任务

#### P1.1 创建 Workspace

**文件：**

- `Cargo.toml`
- `crates/docq-core/Cargo.toml`
- `crates/docq-model/Cargo.toml`
- `crates/docq-indexer/Cargo.toml`
- `crates/docq-storage/Cargo.toml`
- `crates/docq-retrieve/Cargo.toml`
- `crates/docq-synth/Cargo.toml`
- `crates/docq/Cargo.toml`
- `.cargo/config.toml`
- `rust-toolchain.toml`
- `rustfmt.toml`

**要求：**

- Workspace resolver = "2"
- 公共依赖声明在根 `Cargo.toml [workspace.dependencies]`
- 每个 crate 使用 `{ workspace = true }`
- `docq-model` 配置 feature flags：
  ```toml
  [features]
  default = ["embed", "rerank", "llm"]
  embed = ["dep:fastembed"]
  rerank = ["dep:fastembed"]
  llm = ["dep:llama-cpp-2"]
  ```
- `cargo build` 在所有 crate 上通过（无代码时也应通过）。

#### P1.2 定义错误类型

**文件：** `crates/docq-core/src/error.rs`

**要求：**

```rust
pub enum DocqError {
    Parse(ParseError),
    Store(StoreError),
    Embed(EmbedError),
    Retrieve(RetrieveError),
    Synth(SynthError),
    Llm(LlmError),
    Model(ModelError),
}

pub type Result<T> = std::result::Result<T, DocqError>;
```

每个子错误 enum 至少包含一个 `Other(String)` 变体便于开发期使用。

#### P1.3 定义核心数据类型

**文件：** `crates/docq-core/src/models.rs`

**要求定义：**

```rust
pub struct Document {
    pub id: String,
    pub file_path: PathBuf,
    pub content_hash: String,
    pub content_size: usize,
    pub indexed_at: DateTime<Utc>,
}

pub struct Chunk {
    pub id: String,
    pub doc_id: String,
    pub text: String,
    pub byte_range: Range<usize>,
}

pub struct ChunkCandidate {
    pub text: String,
    pub byte_range: Range<usize>,
}

pub struct SearchHit {
    pub chunk: Chunk,
    pub score: f32,
    pub explain: ScoreExplain,
}

pub struct ScoreExplain {
    pub bm25_score: Option<f32>,
    pub vector_score: Option<f32>,
    pub rrf_score: Option<f32>,
    pub rerank_score: Option<f32>,
    pub final_score: f32,
}

pub struct Answer {
    pub text: String,
    pub citations: Vec<Citation>,
}

pub struct Citation {
    pub marker: String,
    pub source: String,
}

pub struct ModelSpec {
    pub role: String,
    pub repo_id: String,
    pub filename: String,
    pub revision: String,
    pub checksum: Option<String>,
}
```

所有类型实现 `Debug` + `Clone`；需要序列化的实现 `Serialize`/`Deserialize`。

#### P1.4 定义核心 trait

**文件：** `crates/docq-core/src/traits.rs`

**要求定义：**

```rust
#[async_trait]
pub trait Embedder: Send + Sync {
    fn dimension(&self) -> usize;
    fn model_name(&self) -> &str;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[async_trait]
pub trait Reranker: Send + Sync {
    async fn rerank(&self, query: &str, chunks: &[Chunk]) -> Result<Vec<ScoredChunk>>;
}

#[async_trait]
pub trait LLM: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
}

pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str) -> Vec<ChunkCandidate>;
}

pub trait Storage: Send + Sync {
    fn init(&self) -> Result<()>;

    fn add_document(&self, doc: &Document) -> Result<()>;
    fn get_document(&self, doc_id: &str) -> Result<Option<Document>>;
    fn list_documents(&self) -> Result<Vec<Document>>;
    fn delete_document(&self, doc_id: &str) -> Result<()>;

    fn add_chunks(&self, chunks: &[Chunk]) -> Result<()>;
    fn get_chunks(&self, chunk_ids: &[String]) -> Result<Vec<Chunk>>;
    fn delete_chunks_by_doc(&self, doc_id: &str) -> Result<()>;

    fn add_vectors(&self, chunk_ids: &[String], embeddings: &[Vec<f32>]) -> Result<()>;
    fn search_vectors(&self, embedding: &[f32], top_k: usize) -> Result<Vec<(String, f32)>>;

    fn search_text(&self, query: &str, top_k: usize) -> Result<Vec<(String, f32)>>;

    fn set_model_version(&self, role: &str, version: &ModelSpec) -> Result<()>;
    fn get_model_version(&self, role: &str) -> Result<Option<ModelSpec>>;
}
```

#### P1.5 `docq-core` 导出

**文件：** `crates/docq-core/src/lib.rs`

**要求：**

```rust
pub mod error;
pub mod models;
pub mod traits;

pub use error::{DocqError, Result};
pub use models::*;
pub use traits::*;
```

### 验收标准

- `cargo check --workspace` 通过。
- `cargo test -p docq-core` 通过（可只有类型构造测试）。
- 所有类型和 trait 能在其他 crate 中通过 `use docq_core::*` 引用。

---

## P2：Storage trait + SQLite 基础

### 目标

实现 `Storage` trait 的 SQLite 版本，完成基础 metadata 和 chunk 存储。

### 任务

#### P2.1 创建 `SqliteStorage`

**文件：** `crates/docq-storage/src/lib.rs`、`crates/docq-storage/src/sqlite.rs`

**要求：**

```rust
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub fn open_in_memory() -> Result<Self>;
}

impl Storage for SqliteStorage {
    // 实现所有 Storage trait 方法
}
```

#### P2.2 Schema 初始化

**要求：** `Storage::init()` 创建以下表：

```sql
CREATE TABLE documents (
    doc_id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    content_size INTEGER NOT NULL,
    indexed_at TEXT NOT NULL
);

CREATE TABLE chunks (
    chunk_id TEXT PRIMARY KEY,
    doc_id TEXT NOT NULL,
    text TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    FOREIGN KEY (doc_id) REFERENCES documents(doc_id)
);

CREATE TABLE model_versions (
    role TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    revision TEXT NOT NULL,
    checksum TEXT
);
```

#### P2.3 实现 documents / chunks CRUD

**要求：**

- `add_document` / `get_document` / `list_documents` / `delete_document`
- `add_chunks` / `get_chunks` / `delete_chunks_by_doc`
- 所有操作通过 `rusqlite` 执行
- 使用事务保证 `add_chunks` 批量写入原子性

#### P2.4 创建 `InMemoryStorage`

**文件：** `crates/docq-storage/src/memory.rs`

**要求：**

- 用 `HashMap` 实现 `Storage`
- 仅用于测试
- 实现所有 `Storage` trait 方法

### 验收标准

```rust
#[test]
fn test_document_crud() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();
    // add / get / list / delete 验证
}

#[test]
fn test_chunk_crud() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();
    // add chunks / get chunks / delete by doc 验证
}
```

- `cargo test -p docq-storage` 全部通过。

---

## P3：sqlite-vec + FTS5

### 目标

在 SQLite 中集成 sqlite-vec 向量索引和 FTS5 全文索引。

### 任务

#### P3.1 集成 sqlite-vec

**文件：** `crates/docq-storage/src/sqlite.rs`

**要求：**

1. 依赖 `sqlite-vec`。
2. 在 `SqliteStorage::init()` 中加载扩展。
3. 创建虚拟表：
   ```sql
   CREATE VIRTUAL TABLE vec_chunks USING vec0(
       chunk_id TEXT PRIMARY KEY,
       embedding FLOAT[512]
   );
   ```
4. 实现 `Storage::add_vectors`。
5. 实现 `Storage::search_vectors`。

#### P3.2 集成 FTS5

**文件：** `crates/docq-storage/src/sqlite.rs`

**要求：**

1. 创建独立 FTS5 虚拟表：
   ```sql
   CREATE VIRTUAL TABLE fts_chunks USING fts5(
       chunk_id,
       text,
       tokenize='unicode61'
   );
   ```
2. 实现 `Storage::search_text`。
3. 在 `add_chunks` 时同步插入 `fts_chunks`（text 需调用方提供 jieba 分词后版本）。

**注意：** `chunks.text` 存原始文本，`fts_chunks.text` 存 jieba 分词后空格拼接文本。

#### P3.3 事务一致性

**要求：**

- `Storage` 提供 `begin_tx()` 返回 `StorageTx`
- `StorageTx` 支持 `add_document` / `add_chunks` / `add_vectors` / `add_fts_chunks`
- `StorageTx::commit()` 一次性提交

### 验收标准

```rust
#[test]
fn test_vector_search() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();
    // 插入 10 个 512 维向量
    // 查询 top-3，验证顺序正确
}

#[test]
fn test_text_search() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();
    // 插入已 jieba 分词的文本
    // 查询 "共识 算法"，验证召回
}
```

- `cargo test -p docq-storage` 通过。

---

## P4：ModelHub（下载/缓存/注册表）

### 目标

实现模型下载、缓存、版本管理。

### 任务

#### P4.1 默认模型配置

**文件：** `crates/docq-model/src/registry.rs`

**要求：**

```rust
pub struct ModelRegistry;

impl ModelRegistry {
    pub fn default_embedding() -> ModelSpec;
    pub fn default_reranker() -> ModelSpec;
    pub fn default_llm() -> ModelSpec;
}
```

返回：

- embedding: `BAAI/bge-small-zh-v1.5`
- reranker: `BAAI/bge-reranker-base`
- llm: `Qwen/Qwen2.5-7B-Instruct-GGUF`

#### P4.2 ModelHub

**文件：** `crates/docq-model/src/hub.rs`

**要求：**

```rust
pub struct ModelHub {
    cache_dir: PathBuf,
}

impl ModelHub {
    pub fn new(cache_dir: PathBuf) -> Self;
    pub async fn ensure(&self, spec: &ModelSpec) -> Result<PathBuf>;
}
```

- 检查 `cache_dir / repo_id / filename` 是否存在
- 不存在则使用 `hf-hub` 下载
- 返回本地文件路径

#### P4.3 模型版本记录

**要求：**

- `ModelHub` 下载完成后，调用 `Storage::set_model_version` 记录
- v0.1 跳过 checksum 校验，但预留字段

### 验收标准

```rust
#[tokio::test]
async fn test_hub_ensure() {
    let hub = ModelHub::new(temp_dir());
    let spec = ModelRegistry::default_embedding();
    let path = hub.ensure(&spec).await.unwrap();
    assert!(path.exists());
}
```

- 单元测试使用本地已存在模型文件，避免 CI 下载。
- `cargo test -p docq-model` 通过。

---

## P5：Embedder + 文本读取

### 目标

实现 embedding 推理和文件读取。

### 任务

#### P5.1 FastEmbedEmbedder

**文件：** `crates/docq-model/src/embed.rs`

**要求：**

```rust
pub struct FastEmbedEmbedder {
    inner: TextEmbedding,
    model_name: String,
}

impl FastEmbedEmbedder {
    pub async fn from_model_hub(hub: &ModelHub) -> Result<Self>;
}

impl Embedder for FastEmbedEmbedder {
    fn dimension(&self) -> usize;
    fn model_name(&self) -> &str;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
```

- 使用 `fastembed` 加载 `BAAI/bge-small-zh-v1.5`
- batch inference

#### P5.2 文本读取器

**文件：** `crates/docq-indexer/src/reader.rs`

**要求：**

```rust
pub struct TextReader {
    extensions: Vec<String>,
    ignore_patterns: Vec<glob::Pattern>,
}

impl TextReader {
    pub fn new() -> Self;
    pub fn with_extensions(extensions: &[&str]) -> Self;
    pub fn read_dir(&self, path: &Path, recursive: bool) -> Result<Vec<DocumentSource>>;
}

pub struct DocumentSource {
    pub path: PathBuf,
    pub content: String,
}
```

- 默认扩展名：`.txt`, `.md`
- 忽略：隐藏文件、`.git/`、`target/`、`node_modules/`
- 读取文件内容为 UTF-8 `String`
- 非 UTF-8 文件记录 warning 并跳过

### 验收标准

```rust
#[tokio::test]
async fn test_embedder() {
    let embedder = FastEmbedEmbedder::from_model_hub(&hub).await.unwrap();
    let texts = vec!["你好".to_string(), "世界".to_string()];
    let embs = embedder.embed(&texts).await.unwrap();
    assert_eq!(embs.len(), 2);
    assert_eq!(embs[0].len(), 512);
}

#[test]
fn test_reader() {
    let reader = TextReader::new();
    let docs = reader.read_dir(test_dir, true).unwrap();
    assert!(!docs.is_empty());
}
```

---

## P6：Chunker + Indexer 流程

### 目标

实现文本分块和完整索引流程。

### 任务

#### P6.1 SentenceSplitter

**文件：** `crates/docq-indexer/src/chunker.rs`

**要求：**

```rust
pub struct SentenceSplitter {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

impl Default for SentenceSplitter {
    fn default() -> Self {
        Self { chunk_size: 2048, chunk_overlap: 200 }
    }
}

impl Chunker for SentenceSplitter {
    fn chunk(&self, text: &str) -> Vec<ChunkCandidate>;
}
```

- 使用 `tokenizers` 加载 BGE tokenizer
- 分层切分：paragraph → sentence（支持中文标点 `。！？`）→ word → char
- greedy merge 到 `chunk_size` tokens
- 相邻 chunk 保留 `chunk_overlap` tokens
- 记录每个 chunk 在原始文件中的字节范围

#### P6.2 jieba 分词辅助函数

**文件：** `crates/docq-indexer/src/tokenizer.rs`

**要求：**

```rust
pub fn jieba_tokenize(text: &str) -> String;
```

- 用 `jieba-rs` 分词
- 用空格连接 tokens

#### P6.3 Indexer

**文件：** `crates/docq-indexer/src/lib.rs`

**要求：**

```rust
pub struct IndexerConfig {
    pub chunker: Arc<dyn Chunker>,
    pub embedder: Arc<dyn Embedder>,
    pub storage: Arc<dyn Storage>,
}

pub struct Indexer {
    config: IndexerConfig,
}

impl Indexer {
    pub fn new(config: IndexerConfig) -> Self;
    pub async fn index_file(&self, path: &Path) -> Result<IndexStats>;
    pub async fn index_collection(&self, path: &Path) -> Result<IndexStats>;
}
```

**索引流程：**

1. 读取文件内容
2. 计算 `content_hash = sha256(content)`
3. 查询 storage，若 hash 相同则 skip
4. 若文件已存在但 hash 不同，删除旧 chunks/vectors/fts
5. `Chunker::chunk(content)` 生成 chunks
6. `Embedder::embed(chunk_texts)` 生成 embeddings
7. 对每个 chunk text 做 jieba 分词
8. SQLite 事务写入：documents / chunks / vec_chunks / fts_chunks

### 验收标准

```rust
#[tokio::test]
async fn test_index_and_search() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();
    let indexer = Indexer::new(IndexerConfig { ... });
    let stats = indexer.index_file(test_txt).await.unwrap();
    assert!(stats.chunks_indexed > 0);

    // 重复索引应 skip
    let stats2 = indexer.index_file(test_txt).await.unwrap();
    assert_eq!(stats2.chunks_indexed, 0);
}
```

---

## P7：BM25 + 向量召回 + RRF

### 目标

实现混合检索的基础流程。

### 任务

#### P7.1 Retriever 结构

**文件：** `crates/docq-retrieve/src/lib.rs`

**要求：**

```rust
pub struct RetrieverConfig {
    pub storage: Arc<dyn Storage>,
    pub embedder: Arc<dyn Embedder>,
    pub bm25_top_k: usize,
    pub vector_top_k: usize,
    pub rrf_k: usize,
}

pub struct Retriever {
    config: RetrieverConfig,
}

impl Retriever {
    pub fn new(config: RetrieverConfig) -> Self;
    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>>;
}
```

#### P7.2 两路召回

**要求：**

- `search_bm25(query, top_k)` → `Storage::search_text`
- `search_vector(query, top_k)` → embed query → `Storage::search_vectors`

#### P7.3 RRF 融合

**文件：** `crates/docq-retrieve/src/fusion.rs`

**要求：**

```rust
pub fn reciprocal_rank_fusion(
    bm25_results: &[(String, f32)],
    vector_results: &[(String, f32)],
    k: usize,
) -> Vec<(String, f32)>
```

- 计算每个 chunk_id 的 `rrf_score = Σ 1 / (k + rank)`
- 按 score 降序排列

#### P7.4 组装 SearchHit

**要求：**

- 根据 RRF top 结果从 storage 取出 `Chunk`
- 填充 `ScoreExplain`（bm25_score / vector_score / rrf_score）
- final_score = rrf_score

### 验收标准

```rust
#[tokio::test]
async fn test_hybrid_search() {
    // 准备索引好的 storage
    let retriever = Retriever::new(config);
    let hits = retriever.search("生日", 5).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].score > 0.0);
}
```

---

## P8：Reranker

### 目标

实现 cross-encoder 精排。

### 任务

#### P8.1 Reranker 实现

**文件：** `crates/docq-model/src/rerank.rs`

**要求：**

```rust
pub struct FastEmbedReranker {
    inner: TextRerank, // 或基于 ort 的实现
}

impl Reranker for FastEmbedReranker {
    async fn rerank(&self, query: &str, chunks: &[Chunk]) -> Result<Vec<ScoredChunk>>;
}
```

- 加载 `BAAI/bge-reranker-base`
- 输入 `(query, chunk.text)` pairs
- 输出按 relevance score 排序

#### P8.2 集成到 Retriever

**文件：** `crates/docq-retrieve/src/lib.rs`

**要求：**

- `RetrieverConfig` 增加 `reranker: Option<Arc<dyn Reranker>>`
- RRF 后取 top-20，调用 reranker
- 最终按 rerank score 排序
- 填充 `ScoreExplain.rerank_score`

### 验收标准

```rust
#[tokio::test]
async fn test_rerank() {
    let reranker = FastEmbedReranker::from_model_hub(&hub).await.unwrap();
    let chunks = vec![Chunk { ... }, Chunk { ... }];
    let scored = reranker.rerank("query", &chunks).await.unwrap();
    assert_eq!(scored.len(), chunks.len());
}
```

---

## P9：LLM 后端

### 目标

实现本地 GGUF LLM 推理。

### 任务

#### P9.1 LlamaLlm

**文件：** `crates/docq-model/src/llm.rs`

**要求：**

```rust
pub struct LlamaLlm {
    // llama-cpp-2 相关字段
}

impl LlamaLlm {
    pub async fn from_model_hub(hub: &ModelHub) -> Result<Self>;
}

impl LLM for LlamaLlm {
    async fn complete(&self, prompt: &str) -> Result<String>;
}
```

- 加载 `Qwen2.5-7B-Instruct-Q4_K_M.gguf`
- 使用 chat template
- 默认参数：n_ctx=4096，temp=0.7，top_p=0.9，max_tokens=512

#### P9.2 Prompt-only 模式（可选）

**要求：**

- `LLM` trait 的实现可以是本地的，也可以未来扩展为 HTTP API
- v0.1 只实现本地 GGUF

### 验收标准

```rust
#[tokio::test]
async fn test_llm_complete() {
    let llm = LlamaLlm::from_model_hub(&hub).await.unwrap();
    let output = llm.complete("What is 2+3?").await.unwrap();
    assert!(!output.is_empty());
}
```

- 测试标记为 `#[ignore]`，仅在本地有模型时运行，避免 CI 失败。

---

## P10：Ask / Synthesis

### 目标

实现基于检索结果的问答合成。

### 任务

#### P10.1 PromptBuilder

**文件：** `crates/docq-synth/src/prompt.rs`

**要求：**

```rust
pub fn build_ask_prompt(query: &str, hits: &[SearchHit]) -> String;
```

输出格式：

```
Context information is below.
---------------------
[1] docs/a.txt (bytes 120-512):
<chunk text>

[2] docs/b.txt (bytes 800-1200):
<chunk text>
---------------------
Given the context information and not prior knowledge, answer the query.
Cite sources using [1], [2], etc.
Query: {query}
Answer:
```

#### P10.2 CitationParser

**文件：** `crates/docq-synth/src/citation.rs`

**要求：**

```rust
pub fn parse_citations(answer: &str, valid_markers: &[String]) -> Vec<Citation>;
```

- 正则提取 `\[(\d+)\]`
- 过滤不在 `valid_markers` 中的标记
- 返回 `Citation { marker, source }`

#### P10.3 Synthesizer

**文件：** `crates/docq-synth/src/lib.rs`

**要求：**

```rust
pub struct SynthesizerConfig {
    pub retriever: Arc<Retriever>,
    pub llm: Arc<dyn LLM>,
}

pub struct Synthesizer {
    config: SynthesizerConfig,
}

impl Synthesizer {
    pub fn new(config: SynthesizerConfig) -> Self;
    pub async fn ask(&self, query: &str) -> Result<Answer>;
}
```

**流程：**

1. `Retriever::search(query, top_k=5)`
2. `build_ask_prompt(query, &hits)`
3. `LLM::complete(prompt)`
4. `parse_citations(&output, &markers)`
5. 返回 `Answer { text, citations }`

### 验收标准

```rust
#[tokio::test]
async fn test_ask() {
    let synth = Synthesizer::new(config);
    let answer = synth.ask("我的生日是哪一天？").await.unwrap();
    assert!(!answer.text.is_empty());
    assert!(!answer.citations.is_empty());
}
```

---

## P11：Engine Facade

### 目标

组合所有组件，对外暴露 `Engine` API。

### 任务

#### P11.1 Engine 实现

**文件：** `crates/docq-core/src/engine.rs`

**要求：**

```rust
pub struct EngineConfig {
    pub workspace_path: PathBuf,
    pub model_cache_dir: PathBuf,
}

pub struct Engine {
    storage: Arc<dyn Storage>,
    embedder: Arc<dyn Embedder>,
    reranker: Arc<dyn Reranker>,
    llm: Arc<dyn LLM>,
    chunker: Arc<dyn Chunker>,
    indexer: Indexer,
    retriever: Retriever,
    synthesizer: Synthesizer,
}

impl Engine {
    pub async fn open(config: EngineConfig) -> Result<Self>;
    pub fn add_collection(&self, path: impl AsRef<Path>, name: &str) -> Result<()>;
    pub fn list_collections(&self) -> Result<Vec<Collection>>;
    pub async fn index(&self) -> Result<IndexStats>;
    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>>;
    pub async fn ask(&self, query: &str) -> Result<Answer>;
    pub fn status(&self) -> Result<EngineStatus>;
}
```

#### P11.2 Collection 配置持久化

**要求：**

- collection 列表存在 SQLite 的 `collections` 表
- `add_collection` 写入记录
- `index()` 遍历所有 collection

### 验收标准

```rust
#[tokio::test]
async fn test_engine_end_to_end() {
    let engine = Engine::open(EngineConfig { ... }).await.unwrap();
    engine.add_collection(test_dir, "notes").unwrap();
    let stats = engine.index().await.unwrap();
    assert!(stats.chunks_indexed > 0);

    let hits = engine.search("生日", 5).await.unwrap();
    assert!(!hits.is_empty());
}
```

---

## P12：CLI

### 目标

实现 `docq` 命令行工具。

### 任务

#### P12.1 CLI 框架

**文件：** `crates/docq/src/main.rs`

**要求：**

使用 `clap` derive API：

```rust
#[derive(Parser)]
struct Cli { ... }

#[derive(Subcommand)]
enum Commands {
    Init { ... },
    Add { ... },
    Index { ... },
    Search { ... },
    Ask { ... },
    Status { ... },
}
```

#### P12.2 各命令实现

| 命令 | 行为 |
|---|---|
| `docq init [--path ~/.docq]` | 创建 workspace 目录，初始化 SQLite |
| `docq add <path> --name <collection>` | 添加 collection |
| `docq index [--collection <name>]` | 索引全部或指定 collection |
| `docq search <query> [--top-k 5] [--explain] [--json]` | 搜索 |
| `docq ask <query> [--json]` | 问答 |
| `docq status [--json]` | 显示 workspace 状态 |

#### P12.3 JSON 输出

**要求：**

- 所有命令支持 `--json`
- 成功时输出 JSON 到 stdout
- 错误时输出 JSON 到 stderr（包含 `error` 字段）

### 验收标准

```bash
cd /tmp/docq-test
docq init
echo "今天是 2024 年 5 月 20 日，是我的生日。" > note.txt
docq add . --name notes
docq index
docq search "生日" --top-k 3 --json | jq '.hits | length'  # 应 > 0
docq ask "我的生日是哪一天？" --json | jq '.answer'         # 应非空
```

---

## P13：集成测试 + 打磨

### 目标

端到端验证，修复问题，完善文档。

### 任务

#### P13.1 端到端测试

**文件：** `crates/docq/tests/integration_test.rs`

**场景：**

1. 索引 5~10 个测试文档
2. 执行 10 个 search query，验证 top hit 相关
3. 执行 5 个 ask query，验证答案包含正确信息
4. 验证引用指向真实 chunk

#### P13.2 性能基准

**文件：** `crates/docq/benches/search_bench.rs`

**要求：**

- 测量 1000 个 chunks 下的 search 延迟
- 目标：search < 500ms（含 embedding + BM25 + RRF + rerank）
- 测量 ask 首次模型加载时间 vs 后续推理时间

#### P13.3 边界情况

- 空 workspace
- 查询无结果
- 文件被删除后重新索引
- 模型文件缺失时的错误提示
- 非 UTF-8 文件
- 超大文件（>10MB）

#### P13.4 文档

- 更新 `README.md`
- 添加 `docs/usage.md`：快速开始、模型下载、CLI 示例
- 添加 `docs/development.md`：如何运行测试、如何添加新格式

### 验收标准

- `cargo test --workspace` 全部通过。
- `cargo fmt --all -- --check` 通过。
- `cargo clippy --all-features -- -D warnings` 通过。
- CLI 能完成从 init 到 ask 的完整流程。
- 文档完整，新用户能按 README 跑起来。

---

## 附录：开发期约定

### 测试模型

- 单元测试优先使用 stub embedder / stub reranker / stub LLM，避免依赖真实模型。
- 需要真实模型的测试使用 `#[ignore]`，本地手动运行。

### 提交规范

```
feat: add sqlite-vec integration
fix: handle missing model file in ModelHub
refactor: split Storage trait into sync methods
```

### 每日检查点

每完成一个 P 阶段，应满足：

1. `cargo check --workspace` 通过
2. 该阶段新增测试通过
3. 已实现的 CLI 子命令可用
4. 阶段文档更新到 `docs/phase.md` 状态列
