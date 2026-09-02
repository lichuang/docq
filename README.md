# docq

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

**Local-first search and answers for your documents.**

`docq` (short for **document query**) is a local, offline-ready RAG tool written in Rust. It indexes your personal document collections and lets you search or ask questions with cited answers — everything stays on your machine: indexes, models, and queries.

## ✨ What makes docq different

- **Offline document search & Q&A engine** — Search passages or ask natural-language questions; everything runs locally with cited answers.
- **Hybrid retrieval** — Combines BM25 keyword search, dense vector search, RRF fusion, and cross-encoder reranking.
- **Single-file index** — Everything lives in one SQLite database (`sqlite-vec` + FTS5).
- **Cited answers** — `ask` returns natural-language answers with inline `[N]` citations pointing back to source files.
- **Chinese-optimized** — Sentence-level chunking and jieba word-level tokenization for BM25.
- **Library-first workspace** — Core traits live in `docq-core`; heavy backends are isolated behind feature flags.

## ⚖️ Comparison with other tools

| Feature | docq | QMD | LlamaIndex | Chroma | Obsidian Smart Connections |
|---|---|---|---|---|---|
| Fully offline | ✅ | ✅ | ⚠️ (cloud optional) | ✅ | ❌ (uses OpenAI) |
| Single-file index | ✅ SQLite | ⚠️ SQLite (project-local optional) | ❌ | ❌ | ❌ |
| Hybrid retrieval (BM25 + vector + rerank) | ✅ | ✅ | ✅ plugins | ❌ vector only | ❌ |
| LLM query expansion | ❌ | ✅ | ✅ | ❌ | ❌ |
| MCP / agent integration | 🚧 roadmap | ✅ | ✅ | ❌ | ❌ |
| Local LLM answers | ✅ | ❌ | ✅ | ❌ | ⚠️ |
| Chinese-optimized BM25 | ✅ | ⚠️ | ⚠️ | ❌ | ❌ |
| Rust / native performance | ✅ | ❌ Node/Bun | ❌ Python | ❌ Python | ❌ JS |
| Rich output formats (JSON/CSV/XML/MD) | ❌ JSON only | ✅ | ✅ | ❌ | ❌ |
| PDF / Office extraction | ✅ | ❌ | ✅ plugins | ❌ | ❌ |

## 🚀 Quick start

```bash
# Install from source
cargo install --path crates/docq

# Create a workspace (uses ~/.config/docq by default)
docq init

# Add a directory of documents
docq add ~/notes --name notes

# Build the index
docq index

# Search for passages
docq search "quarterly revenue"

# Ask a question and get a cited answer
docq ask "What was the revenue in Q2?"
```

Run `docq --help` and `docq <command> --help` to discover all options.

## 📚 Supported document formats

- **Markdown** (`.md`) and plain text (`.txt`)
- **PDF** (`.pdf`) — enabled by default via the `pdf` feature
- **Microsoft Word** (`.docx`) — enabled by default via the `docx` feature

You can disable optional format support at build time with `--no-default-features`.

## 🧪 Try it with bundled test data

The repository includes sample documents under `testdata/` (excerpts from the public tutorial **Distributed System Illustrated** by [codedump.info](https://www.codedump.info/dist-system-en/?ref=docq)). Try it without preparing your own files:

```bash
docq init
docq add testdata/ --name notes
docq index

# Search
docq search "Multi-Paxos improvements"

# Ask with citations
docq ask "What are the improvements of Multi-Paxos over the Paxos algorithm?"

# See step-by-step timing
 docq ask "What are the improvements of Multi-Paxos over the Paxos algorithm?" -v
```

It also works in Chinese:

```bash
docq ask "multi paxos 相比 paxos 算法的改进点？"
```

## 🏗️ Architecture

```
        cli (docq)
         │
         ▼
    docq (Engine facade)
   ╱    │         ╲
retrieve  index   synthesize
  │       │          │
  ▼       ▼          ▼
storage + model backends
   ╲      │      ╱
        core
```

- **`docq-core`** — Shared types, traits, and errors. Zero heavy dependencies.
- **`docq-storage`** — SQLite implementation of the `Storage` trait (`sqlite-vec`, FTS5).
- **`docq-indexer`** — File reading, chunking, and incremental indexing.
- **`docq-retrieve`** — BM25 + vector recall → RRF → rerank.
- **`docq-model`** — Local model backends: FastEmbed (embed/rerank) and llama.cpp (LLM).
- **`docq-synth`** — Prompt building, LLM completion, and citation parsing.
- **`docq`** — CLI and `Engine` facade.

## 🛠️ Global options

Every command accepts these flags:

- `--workspace <path>` — Use a different workspace directory.
- `--config <path>` / `-c <path>` — Use a custom configuration file.
- `--model-cache <path>` — Store downloaded models in a custom location.

Examples:

```bash
docq --workspace ./project-kb init
docq --workspace ./project-kb --config ./project-kb/docq.toml add ./docs --name docs
docq --workspace ./project-kb search "deployment checklist" --json
```

## 📤 Output formats

`search`, `ask`, and `status` support `--json` for machine-readable output:

```bash
docq search "budget approval" --json
docq ask "Who approved the budget?" --json
docq status --json
```

Use `--explain` with `search` to see the score breakdown:

```bash
docq search "budget approval" --explain
```

## ⚙️ Configuration

The global configuration file is created automatically on first run:

- macOS / Linux: `~/.config/docq/config.toml`
- Windows: `%LOCALAPPDATA%\docq\config.toml`

Override it with `--config`.

## 📥 First-use downloads

The first time you index, search, or ask, `docq` downloads the required local models to `--model-cache` (`~/.cache/docq/models` by default). After that, everything works offline.

## 🎮 GPU acceleration

The prebuilt binary uses the CPU backend. On macOS (Apple Silicon), Metal GPU acceleration is enabled automatically during compilation. On Windows and Linux, build from source:

### Vulkan (Windows / Linux — AMD, Intel, NVIDIA)

Install the [Vulkan SDK](https://vulkan.lunarg.com/), then:

```bash
cargo install docq --features llama-cpp-2/vulkan
```

### CUDA (Linux / Windows — NVIDIA only)

Install the [CUDA Toolkit](https://developer.nvidia.com/cuda-toolkit), then:

```bash
cargo install docq --features llama-cpp-2/cuda
```

If no GPU is available at runtime, `docq` automatically falls back to CPU.

## 🗺️ Roadmap

- [ ] MCP server for agent integration
- [ ] LLM query expansion for hybrid retrieval
- [ ] xlsx / csv indexing
- [ ] File-watcher auto-indexing
- [ ] `docq model` subcommand for model management
- [ ] Customizable output formats (e.g. JSON, CSV, Markdown)
- [ ] Cited answers with source snippets and referenced content
- [ ] Prebuilt release binaries

## 🚧 Status

Early development. The CLI and configuration may change before 1.0. Issues and PRs are welcome.

## 📄 License

MIT OR Apache-2.0
