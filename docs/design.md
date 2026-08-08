# docq v0.1 设计方案

> 版本：v0.1
> 最后更新：2026-08-07

---

## 1. 项目目标

docq 是一个**本地优先的完整 RAG 系统**（检索 + 答案合成），核心能力包括：

- **对 agent**：毫秒级 `search`，BM25 + 稠密向量 + RRF + cross-encoder rerank，零 LLM 成本。
- **对人**：`ask` 生成带内联引用的自然语言答案，运行在本地 GGUF 模型上。
- **存储**：所有索引保存在单个 SQLite 文件中，便于备份、迁移、版本控制。
- **中文优化**：
  - chunking 复刻 LlamaIndex `SentenceSplitter`
  - BM25 使用 jieba 词级分词
  - 默认中文友好的本地模型

本阶段（v0.1）暂不支持：

- MCP server
- PDF / xlsx / docx 解析（仅支持 `.txt`，`.md` 作为纯文本读取）
- Python bindings
- 文件监听自动索引
- CLI `model` 子命令

引用精度在本阶段到**文件级别 + 字节范围**，暂不到 heading / page / row。

---

## 2. 架构概览

### 2.1 Crate 结构

```text
docq/
├── Cargo.toml
└── crates/
    ├── docq-core/      # 类型 + trait，零重依赖
    ├── docq-model/     # 模型注册表、下载、缓存 + 推理后端
    ├── docq-indexer/   # 文档读取、分块、索引逻辑
    ├── docq-storage/   # Storage trait + SQLite 实现
    ├── docq-retrieve/  # BM25 + 向量 + RRF + rerank
    ├── docq-synth/     # ask / 答案合成
    └── docq/               # CLI binary
```

### 2.2 模块职责

| crate | 职责 |
|---|---|
| `docq-core` | 定义所有核心类型与 trait，以及 `Engine` facade API。零重依赖。 |
| `docq-model` | 模型注册表、HuggingFace 下载、本地缓存、checksum 校验；实现 `Embedder` / `Reranker` / `LLM` trait。 |
| `docq-indexer` | 递归读取 `.txt`/`.md`、分块、调用 embedder、写入 storage。负责增量索引与内容寻址去重。 |
| `docq-storage` | 存储抽象 `Storage` trait；SQLite 实现（schema、sqlite-vec、FTS5）。 |
| `docq-retrieve` | 调用 storage 做 BM25 与向量召回，RRF 融合，rerank，生成 `SearchHit` 与 `ScoreExplain`。 |
| `docq-synth` | 基于检索结果构造 prompt，调用 LLM，解析 `[N]` 引用，生成 `Answer`。 |
| `docq` | CLI 二进制，调用各 crate。 |

### 2.3 依赖关系

```text
docq
├── docq-synth
│   ├── docq-retrieve
│   │   ├── docq-storage
│   │   ├── docq-model (feature = "rerank")
│   │   └── docq-core
│   ├── docq-model (feature = "llm")
│   └── docq-core
├── docq-retrieve
├── docq-indexer
│   ├── docq-storage
│   ├── docq-model (feature = "embed")
│   └── docq-core
├── docq-storage
├── docq-model
└── docq-core

docq-storage -> docq-core
docq-model   -> docq-core
```

**原则：**

- 上层可依赖下层，下层不依赖上层。
- `core` 不依赖任何其他内部 crate。
- `indexer` 与 `retrieve` 互不依赖，都通过 `Storage` trait 操作数据。
- SQLite 细节完全隔离在 `docq-storage` 中。
- `docq-model` 通过 feature flags 避免"只想 search 也要编译 llama.cpp"。

### 2.4 `docq-model` 的 feature flags

```toml
[features]
default = ["embed", "rerank", "llm"]
embed = ["dep:fastembed"]
rerank = ["dep:fastembed"]
llm = ["dep:llama-cpp-2"]
```

依赖方按需启用：

- `docq-indexer` 启用 `embed`
- `docq-retrieve` 启用 `rerank`
- `docq-synth` 启用 `llm`
- `docq` 默认启用全部；可用 `--no-default-features` 关闭 `ask`

---

## 3. v0.1 待实现增强项（TODO）

相比一个最基础的"向量检索 + 本地 LLM" RAG，docq v0.1 需要在基础能力之上额外实现以下增强：

| 增强项 | 说明 | 落点 crate |
|---|---|---|
| **BM25 + jieba 中文分词** | FTS5 全文检索，支持中文词级匹配 | `docq-storage` + `docq-retrieve` |
| **cross-encoder reranker** | 对向量/BM25 召回结果做精排 | `docq-model` + `docq-retrieve` |
| **内联 `[N]` 引用解析** | LLM 输出带引用标记，解析后生成 `Citation` | `docq-synth` |
| **ModelHub 自动下载/缓存** | 首次使用自动从 HuggingFace 拉取模型 | `docq-model` |
| **7B 本地 LLM** | 默认 Qwen2.5-7B-Instruct-Q4_K_M | `docq-model` |
| **CLI `--json` 输出** | 所有命令支持 JSON，便于 agent 调用 | `docq` |
| **collection / workspace 管理** | `init`/`add`/`index` 多集合管理 | `docq` + `docq` |
| **ScoreExplain 分数解释** | 返回 BM25/向量/RRF/rerank 各环节分数 | `docq-retrieve` |

> 注：`sqlite-vec`、类 `SentenceSplitter` 分块、`chunks.text` 存储等属于 v0.1 基础设计选型，不是额外增强项。

---

## 4. 核心数据模型

```rust
// ===== 文档与分块 =====

pub struct Document {
    pub id: String,               // 文件相对路径
    pub file_path: PathBuf,
    pub content_hash: String,     // 内容 hash，用于变更检测
    pub content_size: usize,
    pub indexed_at: DateTime<Utc>,
}

pub struct Chunk {
    pub id: String,               // 内容 hash
    pub doc_id: String,
    pub text: String,             // 完整原始文本（实际嵌入的文本）
    pub byte_range: Range<usize>, // 在原始文件中的字节范围
}

pub struct ChunkCandidate {
    pub text: String,
    pub byte_range: Range<usize>,
}

// ===== 检索结果 =====

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

// ===== 答案与引用 =====

pub struct Answer {
    pub text: String,
    pub citations: Vec<Citation>,
}

pub struct Citation {
    pub marker: String,           // [1], [2]...
    pub source: String,           // "docs/a.txt (bytes 120-512)"
}

// ===== 模型配置 =====

pub struct ModelSpec {
    pub role: String,             // "embedding" / "reranker" / "chat"
    pub repo_id: String,          // HuggingFace repo id
    pub filename: String,         // GGUF / ONNX 文件名
    pub revision: String,
    pub checksum: Option<String>,
}
```

### 设计说明

- `Document.id` 使用**文件相对路径**，和 POC 的 `filename_as_id=True` 一致。重命名即重新索引，逻辑简单。
- `Document.content_hash` 用于检测文件内容变更，实现增量索引。
- `Chunk.id` 使用**内容 hash**，天然支持去重与变更检测。
- `Chunk.text` 存储**实际被嵌入的完整文本**，保证 search/ask 看到的文本和 embedding 完全一致。
- `byte_range` 用于 citation 定位。

---

## 5. 核心 trait

```rust
// ===== core/src/traits.rs =====

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
    // workspace
    fn init(&self) -> Result<()>;

    // documents
    fn add_document(&self, doc: &Document) -> Result<()>;
    fn get_document(&self, doc_id: &str) -> Result<Option<Document>>;
    fn list_documents(&self) -> Result<Vec<Document>>;
    fn delete_document(&self, doc_id: &str) -> Result<()>;

    // chunks
    fn add_chunks(&self, chunks: &[Chunk]) -> Result<()>;
    fn get_chunks(&self, chunk_ids: &[String]) -> Result<Vec<Chunk>>;
    fn delete_chunks_by_doc(&self, doc_id: &str) -> Result<()>;

    // vectors
    fn add_vectors(
        &self,
        chunk_ids: &[String],
        embeddings: &[Vec<f32>],
    ) -> Result<()>;
    fn search_vectors(&self, embedding: &[f32], top_k: usize) -> Result<Vec<(String, f32)>>;

    // full-text search
    fn search_text(&self, query: &str, top_k: usize) -> Result<Vec<(String, f32)>>;

    // model versions
    fn set_model_version(&self, role: &str, version: &ModelSpec) -> Result<()>;
    fn get_model_version(&self, role: &str) -> Result<Option<ModelSpec>>;
}
```

### 异步边界

- `Storage`：**全部 sync**（SQLite 是本地同步 IO）。
- `Embedder` / `Reranker` / `LLM`：**全部 async**（涉及重型推理）。
- `Engine` / `Indexer` / `Retriever` / `Synthesizer` 的公共 API：**async**。

---

## 6. 模型层设计

### 6.1 `docq-model`：注册表 + 下载/缓存 + 推理后端

```rust
pub struct ModelHub {
    cache_dir: PathBuf,
}

impl ModelHub {
    pub fn new(cache_dir: PathBuf) -> Self;
    pub async fn ensure(&self, spec: &ModelSpec) -> Result<PathBuf>;
}
```

职责：

- 维护默认模型配置
- 从 HuggingFace 下载模型文件
- 本地缓存管理（默认 `~/.cache/docq/models/`）
- checksum 校验（v0.1 可选）
- 实现 `Embedder` / `Reranker` / `LLM` trait

### 6.2 默认模型

| 角色 | 默认模型 | 格式 | 维度/大小 |
|---|---|---|---|
| Embedding | `BAAI/bge-small-zh-v1.5` | ONNX | 512 维，~100MB |
| Reranker | `BAAI/bge-reranker-base` | ONNX | ~1GB |
| Chat | `Qwen2.5-7B-Instruct-Q4_K_M.gguf` | GGUF | ~4.5GB |

### 6.3 后端选择

- **Embedding**：`fastembed` crate
- **Reranker**：`fastembed` 或 `ort`（待验证）
- **LLM**：`llama-cpp-2` crate

> macOS 注意：`llama-cpp-2` 默认需要 macOS 15+ SDK 的 Metal API。v0.1 通过 `.cargo/config.toml` 设置 `GGML_METAL=OFF` 走 CPU 后端，兼容 macOS 14.x。

---

## 7. 分词策略

### 7.1 Chunking 分词：复刻 LlamaIndex `SentenceSplitter`

POC 中使用：

```python
Settings.text_splitter = SentenceSplitter(chunk_size=2048, chunk_overlap=200)
```

docq 复刻其核心逻辑：

```rust
pub struct SentenceSplitter {
    pub chunk_size: usize,        // default 2048 tokens
    pub chunk_overlap: usize,     // default 200 tokens
    pub paragraph_separator: &'static str,  // "\n\n\n"
}

impl Chunker for SentenceSplitter {
    fn chunk(&self, text: &str) -> Vec<ChunkCandidate> {
        // 1. paragraph split
        // 2. sentence split by regex: [^,.;。？！]+[,.;。？！]?|[,.;。？！]
        // 3. fallback: word split (whitespace)
        // 4. final fallback: char split
        // 5. greedy merge with overlap
    }
}
```

**Tokenizer 选择：** 使用 `tokenizers` crate 加载 embedding 模型自带的 `tokenizer.json`。这样 chunk 大小和 embedding 模型的实际输入长度一致，避免截断。

### 7.2 FTS5 分词：jieba 预分词

为了对中文文本实现词级 BM25，这里采用 jieba 预分词方案：

- 索引时：`jieba.cut(chunk.text)` → 用空格连接 tokens → 写入 FTS5
- 查询时：`jieba.cut(query)` → 用空格连接 → 查询 FTS5
- FTS5 tokenizer 用内置 `unicode61`

实现上，`fts_chunks` 作为独立虚拟表：

```sql
CREATE VIRTUAL TABLE fts_chunks USING fts5(
    chunk_id,
    text,
    tokenize='unicode61'
);
```

`fts_chunks.text` 存 jieba 分词后空格拼接的文本，`chunks.text` 存原始文本。

---

## 8. 存储层设计

### 8.1 SQLite Schema

```sql
-- 文档表
CREATE TABLE documents (
    doc_id TEXT PRIMARY KEY,        -- 文件相对路径
    file_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    content_size INTEGER NOT NULL,
    indexed_at TEXT NOT NULL
);

-- 文本块表
CREATE TABLE chunks (
    chunk_id TEXT PRIMARY KEY,      -- 内容 hash
    doc_id TEXT NOT NULL,
    text TEXT NOT NULL,             -- 完整原始文本（实际嵌入的文本）
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    FOREIGN KEY (doc_id) REFERENCES documents(doc_id)
);

-- 向量虚拟表（sqlite-vec）
CREATE VIRTUAL TABLE vec_chunks USING vec0(
    chunk_id TEXT PRIMARY KEY,
    embedding FLOAT[512]
);

-- 全文检索虚拟表（jieba 预分词后写入）
CREATE VIRTUAL TABLE fts_chunks USING fts5(
    chunk_id,
    text,
    tokenize='unicode61'
);

-- 模型版本表
CREATE TABLE model_versions (
    role TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    revision TEXT NOT NULL,
    checksum TEXT
);
```

### 8.2 事务与一致性

一次 `index_collection` 操作对应一个 storage 事务：

```rust
let mut tx = storage.begin_tx()?;
tx.add_document(&doc)?;
tx.add_chunks(&chunks)?;
tx.add_vectors(&chunk_ids, &embeddings)?;
tx.add_fts_chunks(&chunk_ids, &tokenized_texts)?;
tx.commit()?;
```

---

## 9. 索引流程

```text
docq index
  │
  ▼
读取 workspace 配置（collection 路径列表）
  │
  ▼
对每个 collection 路径递归遍历 .txt/.md 文件
  │
  ▼
对每个文件：
  ├── 计算 content_hash
  ├── 如果 hash 没变 → 跳过
  ├── 如果 hash 变了 → 删除旧 doc + chunks + vectors + fts
  └── 读取文本
        │
        ▼
  SentenceSplitter 分块
        │
        ▼
  Embedder.embed(batch) 生成 embeddings
        │
        ▼
  SQLite 事务写入：
      documents, chunks, vec_chunks, fts_chunks
```

---

## 10. 检索流程

```text
query
  │
  ├──► jieba 分词 → FTS5 BM25 → top-100 (chunk_id, bm25_score)
  │
  ├──► embed(query) → sqlite-vec cosine → top-100 (chunk_id, vector_score)
  │
  ▼
RRF 融合
  rrf_score = Σ 1 / (60 + rank)
  │
  ▼
取 top-20
  │
  ▼
cross-encoder rerank
  │
  ▼
返回 top-k SearchHit，含 ScoreExplain
```

### 默认参数

- `bm25_top_k = 100`
- `vector_top_k = 100`
- `rrf_k = 60`
- `rerank_top_n = 20`
- 最终返回 `top_k` 由调用方指定（默认 5）

---

## 11. Ask 流程

```text
query
  │
  ▼
Retriever::search(query, top_k=5)
  │
  ▼
构造 prompt:

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
  │
  ▼
LLM::complete(prompt)
  │
  ▼
解析 [N] 引用
  │
  ▼
过滤无效引用（只保留 context 中存在的 marker）
  │
  ▼
Answer { text, citations }
```

---

## 12. Engine API

```rust
// docq-core/src/lib.rs

pub struct Engine {
    storage: Arc<dyn Storage>,
    embedder: Arc<dyn Embedder>,
    reranker: Arc<dyn Reranker>,
    llm: Arc<dyn LLM>,
    chunker: Arc<dyn Chunker>,
    config: EngineConfig,
}

impl Engine {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub fn add_collection(&self, path: impl AsRef<Path>, name: &str) -> Result<()>;
    pub fn list_collections(&self) -> Result<Vec<Collection>>;
    pub async fn index(&self) -> Result<IndexStats>;
    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchHit>>;
    pub async fn ask(&self, query: &str) -> Result<Answer>;
    pub fn status(&self) -> Result<EngineStatus>;
}
```

---

## 13. CLI 设计

```bash
# 初始化 workspace
docq init [--path ~/.docq]

# 添加集合
docq add <path> --name <collection>

# 构建/更新索引
docq index [--collection <name>]

# 检索（零 LLM 成本）
docq search <query> [--top-k 5] [--explain] [--json]

# 问答
docq ask <query> [--json]

# 查看状态
docq status [--json]
```

所有命令支持 `--json`。

**v0.1 不做 `docq model` 子命令。** 模型配置通过配置文件或环境变量指定，首次使用自动下载。

---

## 14. 实现阶段

| 阶段 | 目标 | 预计时间 |
|---|---|---|
| **P1：骨架** | workspace、core 类型与 trait、storage trait 空壳、CLI 命令空壳 | 3~5 天 |
| **P2：Storage + Model** | SQLite schema、sqlite-vec、jieba 预分词 FTS5、模型下载缓存 | 2 周 |
| **P3：Indexer** | 文本读取、SentenceSplitter 分块、增量索引、内容寻址去重 | 1~2 周 |
| **P4：Retrieve** | BM25、向量召回、RRF、rerank、explain | 2 周 |
| **P5：Synth + Ask** | GGUF chat、prompt、citation 解析 | 1~2 周 |
| **P6：CLI + 测试** | `--json`、错误处理、端到端测试、文档 | 1~2 周 |

> 注：P0 技术验证已完成，详见 `docs/phase.md` 的 P0 记录。

---

## 15. 已验证的关键技术点

以下 4 点已在进入实现前由用户完成验证，当前方案基于验证结果继续推进：

1. **`llama-cpp-2` 加载 Qwen2.5-7B GGUF 并稳定推理** — 已通过。
2. **`fastembed` 加载并推理 `BAAI/bge-small-zh-v1.5`** — 已通过。
3. **sqlite-vec 与 rusqlite bundled 的集成** — 已通过。
4. **jieba 预分词 + FTS5 `unicode61` 的中文检索效果** — 已通过。

回退方案（仅在目标机器内存 < 6GB 时考虑）：改用 3B 级模型，或保留 Ollama HTTP 后端作为可选 feature。

---

## 16. 与 LlamaIndex POC 的对应关系

| LlamaIndex 组件 | docq crate |
|---|---|
| `Settings` | `EngineConfig` / `docq-core` |
| `Document` / `TextNode` | `core::Document` / `core::Chunk` |
| `SimpleDirectoryReader` | `docq-indexer::TextReader` |
| `SentenceSplitter` | `docq-indexer::SentenceSplitter` |
| `HuggingFaceEmbedding` | `docq-model::FastEmbedEmbedder` |
| `SimpleVectorStore` | `docq-storage::SqliteStorage` |
| `VectorIndexRetriever` | `docq-retrieve` 的向量召回部分 |
| `SentenceTransformerRerank`（可选后处理器） | `docq-retrieve` 的 rerank 部分 |
| `RetrieverQueryEngine` + `CompactAndRefine` | `docq-synth` |

---

## 17. 后续演进方向

本阶段完成后，可逐步扩展：

- 支持 Markdown heading-aware 分块
- 支持 PDF / xlsx / docx extractor
- 增加 MCP server
- 文件监听自动索引
- CLI `model` 子命令
- Python bindings (PyO3)
- 记忆层：fact extraction、versioning、conflict resolution
