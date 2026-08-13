# docq

**Local-first search and answers for your documents.**

`docq` (short for **document query**) is a local, offline-ready tool that helps you find information in your personal document collections and get cited answers. Everything stays on your machine: indexes, models, and queries.

## What it does

- **`search`** — Find the most relevant passages across your files. Fast, offline, no API calls.
- **`ask`** — Get a natural-language answer synthesized from the retrieved passages, with inline citations pointing back to the source files.

Use it for notes, documentation, research papers, or any text-heavy directory.

## Supported document formats

`docq` can index plain text and common document formats:

- **Markdown** (`.md`) and plain text (`.txt`)
- **PDF** (`.pdf`) — enabled by default via the `pdf` feature
- **Microsoft Word** (`.docx`) — enabled by default via the `docx` feature

You can disable optional format support at build time with `--no-default-features`.

## Install

```bash
cargo install --path crates/docq
```

Or, once published:

```bash
cargo install docq
```

## Quick start

```bash
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

## Example with the bundled test data

The repository includes sample documents under `testdata/`. These excerpts are from the public tutorial **Distributed System Illustrated** by [codedump.info](https://www.codedump.info/dist-system-en/?ref=docq). You can try the CLI without preparing your own files:

```bash
docq init
docq add testdata/ --name notes
docq index

# Search for passages
docq search "Multi-Paxos improvements"

# Ask a question and get a cited answer
docq ask "What are the improvements of Multi-Paxos over the Paxos algorithm?"

# See step-by-step timing
docq ask "What are the improvements of Multi-Paxos over the Paxos algorithm?" -v
```

you can ask the same question in Chinese because docq also support Chinese:

```shell
docq  ask "multi paxos 相比 paxos 算法的改进点？"
```



## Global options

Every command accepts these flags:

- `--workspace <path>` — Use a different workspace directory. The workspace stores the index database.
- `--config <path>` / `-c <path>` — Use a custom configuration file instead of the default.
- `--model-cache <path>` — Store downloaded models in a custom location.

Examples:

```bash
docq --workspace ./project-kb init
docq --workspace ./project-kb --config ./project-kb/docq.toml add ./docs --name docs
docq --workspace ./project-kb search "deployment checklist" --json
```

## Output formats

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

## Configuration

The global configuration file is located at:

- macOS / Linux: `~/.config/docq/config.toml`
- Windows: `%LOCALAPPDATA%\docq\config.toml`

It is created automatically the first time you run `docq`. You can override it with `--config`.

## First-use downloads

The first time you index, search, or ask, `docq` downloads the required local models to `--model-cache` (`~/.cache/docq/models` by default). After that, everything works offline.

## Status

Early development. The CLI and configuration may change before 1.0.

## License

MIT OR Apache-2.0
