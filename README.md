# docq

**Local hybrid search for your notes and memories — BM25 + vectors + reranking, embedded, offline, Rust-native.**

`docq` is an embedded search engine for personal knowledge: markdown notes, PDFs, spreadsheets, documents. It indexes everything into a single SQLite file and answers two kinds of questions:

- **`search`** — for agents. Millisecond-latency ranked passages with file, heading, and score breakdown. Zero LLM cost.
- **`ask`** — for humans. Answers synthesized from the retrieved passages, **with citations down to the page / heading / row**. Runs fully offline on a local GGUF model, or against your own OpenAI-compatible endpoint.

> 中文简介：docq 是一个本地优先的混合检索引擎（BM25 + 向量 + 重排序），为你的笔记与记忆服务。对 agent 暴露毫秒级检索，对人提供带引用的问答。内置中文词级分词（jieba），所有索引存于单个 SQLite 文件，全程离线可用。

---

## Why

Existing options force a bad trade:

- **grep / ripgrep** — you must know the exact words you're looking for.
- **Vector-only search** — fuzzy, misses exact terms, poor ranking.
- **LLM memory frameworks (Mem0, Zep…)** — every write burns LLM tokens; retrieval quality is an afterthought.
- **qmd** — proved the "better grep" category, but its CJK full-text path falls back to character-splitting (no word segmentation), and it stops at passages — no answers.

docq's bets:

1. **Retrieval quality is the whole product.** Hybrid BM25 + dense vectors, RRF fusion, cross-encoder rerank — the pipeline proven by qmd, implemented natively in Rust with first-class Chinese support (word-level jieba segmentation, Chinese-friendly default models).
2. **Agents have their own brains.** `search` never calls an LLM, never touches the network, and costs nothing per query. Reasoning stays with the caller.
3. **Humans want answers, not file offsets.** `ask` adds an *optional* synthesis layer — answers grounded in retrieved passages, every claim carrying a citation.
4. **Library-first.** The engine is a crate you `cargo add`. The CLI and MCP server are thin clients over it. Stable Rust, no nightly, no C++ toolchain required for the core.

## Features

| | |
|---|---|
| Hybrid retrieval | BM25 (SQLite FTS5) + dense vectors (sqlite-vec), RRF fusion, cross-encoder rerank |
| Chinese-optimized | Word-level jieba segmentation for FTS; Chinese-friendly default embedding models |
| Answers with citations | Optional `ask` layer: local GGUF chat model or OpenAI-compatible endpoint; every claim linked to its source |
| Single-file storage | One SQLite file per workspace — copy it, back it up, `gitignore` it |
| Content-addressed | Identical content stored and embedded once |
| Agent-native | MCP server (`search` / `ask` / `get` / `status`) + CLI with `--json` on every command |
| Fully offline | Local GGUF models downloaded once; no API keys required |
| Multi-format | Markdown first; PDF / xlsx / docx via feature flags (see roadmap) |

## Quick start

```bash
cargo install docq-cli        # installs `docq`

docq init
docq add ~/notes --name notes
docq index

# A: retrieval — milliseconds, zero cost
docq search "定价方案为什么选坐席制？" --explain
docq search "pricing seat-based" --json

# B: answers — with citations (downloads a local chat model on first use)
docq ask "定价方案为什么选坐席制？"
```

Example `ask` output:

```
定价方案在 3 月 3 日确定为按坐席收费而非按用量 [1]，原因是访谈发现
团队均按人头做预算，按用量计费会在试用期制造账单焦虑 [2]。

Sources
[1] notes/decisions/pricing.md › 定价决策 (2026-03-03)
[2] notes/interviews/trial-users.md (p. 2)
```

## MCP

```json
{
  "mcpServers": {
    "docq": { "command": "docq", "args": ["serve"] }
  }
}
```

Tools exposed: `search`, `ask`, `get`, `status`, `list_collections`.

## Library

```rust
let engine = docq::Engine::open("~/.docq")?;
engine.add_collection("~/notes", "notes")?;
engine.index().await?;

// A: retrieval
let hits = engine.search("定价方案为什么选坐席制？", 5).await?;
for h in &hits {
    println!("{:.2} {} — {}", h.score, h.chunk.path.display(), h.explain);
}

// B: answers (feature = "ask")
let ans = engine.ask("定价方案为什么选坐席制？").await?;
println!("{}", ans.text);
for c in &ans.citations {
    println!("[{}] {}", c.marker, c.source);
}
```

Only what you use gets compiled:

```toml
# just the engine
docq = "0.1"

# with the answer layer (pulls the local chat-model backend)
docq = { version = "0.1", features = ["ask"] }
```

## Architecture

```
        cli (docq)        mcp
         └───────┬─────────┘
                 ▼
            docq            facade — the only crate library users need
          ╱      │      ╲
  retrieve     index    synthesize        synthesize is optional (feature "ask")
      │  ╲      │          │
      │   ╲     │          ▼
      │    ╲    │       model           GGUF models, download/cache/verify,
      │     ╲   │      ╱                embed / rerank / chat backends
      ▼      ▼ ▼     ▼
           core                     types + traits, zero heavy dependencies
```

- **`core`** — types (`Chunk`, `SearchHit`, `SourceRef`, `Answer`, `Citation`) and traits (`Embedder`, `Reranker`, `TextIndex`, `VectorIndex`, `Synthesizer`). Depend on it to build custom backends without inheriting any model runtime.
- **`index`** — markdown-aware chunking (heading-path preserved), jieba segmentation, incremental reindex, content-addressed dedup, one SQLite file.
- **`retrieve`** — parallel BM25 + ANN recall → RRF → rerank, with full score breakdown (`--explain`). Milliseconds, no LLM, no network.
- **`synthesize`** — citation-grounded answer generation; local GGUF by default, OpenAI-compatible endpoint via one config line.
- **`model`** — model registry, verified downloads, version pinning (embedding-model upgrades trigger explicit reindex, never silent staleness).

## Models

Defaults are local GGUF, downloaded once and cached:

| Role | Default | Notes |
|---|---|---|
| Embedding | Chinese-friendly multilingual model | swap via config or env |
| Reranker | Small cross-encoder | optional but recommended |
| Chat (`ask`) | 4B-class instruct model (Q8) | runs fine on a consumer GPU or CPU |

Every model can be overridden; see `docq model list`.

## Roadmap



## Status

**Early development.** The API is not stable; expect breaking changes before 1.0. Design document: `docs/design.md`.

## License

MIT OR Apache-2.0
