# AGENTS.md — Navigation for Coding Agents

> This file is a navigation guide for coding agents (such as Claude/Sisyphus) working on this repository. Read it first, then consult `docs/phase.md` for per-phase tasks.

## What is docq

`docq` is a **local-first, full RAG system** (retrieval + answer synthesis) written in Rust as a library-first multi-crate workspace:
- Millisecond-latency `search` for agents (BM25 + dense vectors + RRF + cross-encoder rerank), with zero LLM cost.
- `ask` for humans — natural-language answers with inline `[N]` citations, running on a local GGUF model or an OpenAI-compatible endpoint.
- All indexes live in a single SQLite file (`sqlite-vec` + `FTS5` + plain tables); the whole pipeline works offline.
- Chinese-optimized: chunking replicates LlamaIndex's `SentenceSplitter`; BM25 uses jieba word-level pre-tokenization.

See `README.md` for the vision, `docs/design.md` for the authoritative design, and `docs/phase.md` for the phased task list.

## Current progress

See the overview table at the top of `docs/phase.md` (each phase has ✅ / ⬜). Current state:

| Phase | Status |
|---|---|
| P0 technical validation | ✅ done (verified by user; POC code not in repo) |
| P1 Workspace + Core types/traits | ✅ done |
| P2 Storage trait + SQLite basics | ✅ done (no InMemoryStorage — removed) |
| P3.1 sqlite-vec integration | ✅ done |
| P3.2 FTS5 integration | ✅ done |
| P3.3 Transactional consistency (StorageTx) | ✅ done |
| P10 Ask / Synthesis | ✅ done |
| P11–P13 | ⬜ pending |

## Common commands

```bash
# Fastest compile check
cargo check --workspace

# Run all tests
cargo test --workspace

# Run tests for one crate
cargo test -p docq-storage

# Format check and apply
cargo fmt --all -- --check
cargo fmt --all

# Clippy (project requires -D warnings)
cargo clippy --all-features -- -D warnings
```

**Notes:**
- The Rust toolchain is pinned to `1.95.0` stable in `rust-toolchain.toml`. Do not use nightly.
- On macOS, `llama-cpp-2` defaults to Metal. This project forces the CPU backend via `GGML_METAL=OFF` in `.cargo/config.toml` for macOS 14.x compatibility — do not remove this setting.
- The first `cargo check` builds heavy C/C++ dependencies (`libsqlite3-sys`, `llama-cpp-sys-2`, `ort-sys`); 5–10 minutes is normal. Incremental builds are much faster.

## Architecture and crate relationships

```
        cli (docq)        mcp (future)
         └───────┬─────────┘
                 ▼
            docq            facade — the only crate library users touch
          ╱      │      ╲
  retrieve     index    synthesize        synthesize is optional (feature "ask")
      │  ╲      │          │
      │   ╲     │          ▼
      │    ╲    │       model           GGUF / ONNX backends
      │     ╲   │      ╱
      ▼      ▼ ▼     ▼
           core                     types + traits, zero heavy dependencies
```

### Crate responsibilities and dependencies

| crate | responsibility | depends on |
|---|---|---|
| `docq-core` | All core types + traits + error types; zero internal deps | none |
| `docq-model` | Model registry, HF download cache, verification, inference backends (Embedder/Reranker/Llm) | core |
| `docq-indexer` | File reading, chunking, incremental indexing, content-addressed dedup | core + storage + model(embed) |
| `docq-storage` | SQLite `Storage` impl: documents / chunks / `vec_chunks` (sqlite-vec) / `fts_chunks` (FTS5) / model_versions | core |
| `docq-retrieve` | BM25 + vector recall → RRF fusion → rerank; returns `SearchHit` + `ScoreExplain` | core + storage + model(rerank) |
| `docq-synth` | Ask: build prompt → LLM → parse `[N]` citations → `Answer` | core + retrieve + model(llm) |
| `docq` | CLI binary (currently empty shell, implemented in P12) | all of the above |

### Layering rules (important — do not break)

- Upper layers may depend on lower layers; **lower layers must not depend on upper layers**.
- `docq-core` does not depend on any other internal crate — it defines all traits that other crates implement. This is the key to the library-first promise: `cargo add docq-core` does not pull in SQLite, llama.cpp, or other heavy stacks.
- `indexer` and `retrieve` do not depend on each other; both operate on data via the `Storage` trait.
- SQLite details are fully isolated within `docq-storage`.
- `docq-model` uses feature flags so consumers enable only the backends they need (avoiding "just want search but must compile llama.cpp").

## Key design decisions (do not overturn without intent)

1. **The `Storage` trait stays in `docq-core`**, not in `docq-storage`. This is dependency inversion: `indexer` and `retrieve` only need `docq-core` + `docq-model` and do not pull `rusqlite`/`sqlite-vec` at compile time. A future `docq-storage-pg` would be a drop-in replacement.
2. **No `InMemoryStorage`**. Tests use `SqliteStorage::open_in_memory()` (SQLite `:memory:` mode, millisecond startup).
3. **`chunks.text` is the original text; `fts_chunks.text` is the jieba-tokenized space-joined text**. `StorageTx::add_fts_chunks(chunk_ids, tokenized_texts)` writes to the FTS table separately — `add_chunks` only writes the `chunks` table. This is an explicit decision in phase.md P3.2; the P6 indexer will call both methods inside one `begin_tx` / `commit` bracket.
4. **All mutations flow through `StorageTx`** — `Storage` is read-only (queries + `init` + `begin_tx`). This enforces transactional writes at the type level: `docq-retrieve` holds `&Storage` and cannot write; `docq-indexer` holds `&mut dyn StorageTx` and all four indexed tables (`documents` / `chunks` / `vec_chunks` / `fts_chunks`) plus `model_versions` commit atomically, so a re-embedding failure cannot leave the store half-written.
5. **`Document.id` is the file-relative path** — renaming the file triggers a reindex, keeping the logic simple.
6. **`Chunk.id` is the SHA-256 of `text`** — naturally enables content-addressed dedup and change detection.
7. **Embedding model upgrades trigger an explicit reindex**: the `model_versions` table records the current model spec for each role; the indexer compares the stored spec with the live one and forces a re-embedding when they differ, avoiding silent staleness.
8. **Invalid citations in the `ask` flow are filtered out**: after the LLM produces `[N]` markers, only those that actually appear in the provided context are kept.
9. **Not in v0.1**: MCP server, PDF/xlsx/docx parsing, Python bindings, file-watcher auto-indexing, `docq model` subcommand; citation precision is limited to "file + byte range" (not heading/page/row).

## Code style

- `rustfmt.toml`: `max_width=120`, `tab_spaces=2`, `chain_width=100`, `reorder_imports=true`, `merge_derives=false`.
- Private struct fields use 2-space indent.
- Error types use `thiserror`; do not hand-write `Display`.
- Async traits use `#[async_trait]`.
- Public APIs get short rustdoc (one-line `//!` module description + field comments only for non-obvious conventions, e.g. "SHA-256 of `text`", "RFC3339 UTC").
- **Never** use type-erasure shims like `as any` / `@ts-ignore` (TS concepts). The Rust equivalents are `unimplemented!()` / `todo!()` — only allowed in stub methods that are clearly annotated "to be implemented in P3", and must be removed before commit.
- **No deep-path references in code**: types like `docq_core::EmbedError::Other` must be flattened to `EmbedError::Other` via `use` at the top of the file. Never nest more than two `::` levels inline — import the item and use the short name. This keeps lines short and makes dependencies explicit at the file top.

## Commit conventions

```
feat: add sqlite-vec integration
fix: handle missing model file in ModelHub
refactor: split Storage trait into sync methods
doc: update phase.md status after P2 completion
```

Before your first commit, read the "Appendix: development conventions" section in `docs/phase.md`. After completing a phase, make sure:
1. `cargo check --workspace` passes.
2. New tests for the phase pass.
3. The `docs/phase.md` status column is updated (change ⬜ to ✅).

## Testing conventions

- Unit tests prefer stub embedders / stub rerankers / stub LLMs to avoid real model downloads.
- Tests that need a real model (e.g. loading a 4.5GB GGUF) are marked `#[ignore]` and run locally via `cargo test -- --ignored`, not in CI.
- Integration tests use `SqliteStorage::open_in_memory()` and need no external resources.

## Known external dependency behavior

- `sqlite-vec` registers itself process-globally via `sqlite3_auto_extension` — guarded by a `std::sync::Once` to avoid duplicate registration. See `ensure_vec_extension()` in `crates/docq-storage/src/sqlite.rs`.
- Vectors are passed to sqlite-vec as packed native-endian `f32` byte streams; KNN queries use `WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance`.
- `llama-cpp-2` requires cmake to compile the `llama.cpp` C++ source; first build is slow. `GGML_METAL=OFF` makes it use the CPU backend (macOS 14.x compatibility).
- `fastembed` pulls in the ONNX runtime and model files; on first run it downloads models to `~/.cache/fastembed` or a similar directory.

## Common task navigation

| You want to | Look at |
|---|---|
| Add a new Storage backend (e.g. PostgreSQL) | `docq-core`'s `Storage` trait + existing `SqliteStorage` as reference |
| Add a new embedding/rerank/LLM backend | `docq-core`'s corresponding trait + `docq-model`'s `fastembed`/`llama-cpp-2` implementations |
| Add a new file format (PDF/xlsx/docx) | `docq-indexer`'s `reader.rs` (P5); add a feature flag for each new extractor |
| Add a CLI subcommand | `docq` crate's `src/main.rs`, using clap derive (P12) |
| Change the schema | `SqliteStorage::init()`'s `execute_batch` + related CRUD methods; consider a migration path. For v0.1 a simple breaking change is fine. |
| Add a unit test | Same module as the code under test, in `#[cfg(test)] mod tests`; see the existing 5 tests in `sqlite.rs` for reference |