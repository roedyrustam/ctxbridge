# `ctxbridge` — Technical Documentation

## 1. Overview
`ctxbridge` is a local-first Rust CLI and MCP server for synchronizing AI coding context, architectural decisions, and agent rules across Antigravity, Claude Code, Cursor, Codex CLI, and Gemini CLI.

## 2. Directory & Storage Layout
- **Project Store**: `<repo>/.ctxbridge/`
  - `project.db` — SQLite FTS5 database for project-scoped memories & capsules.
  - `memories.jsonl` — Git-friendly append/sync snapshot of project memories.
  - `rules.md` — Canonical project rules source synced to all AI tool instruction files.
  - `.gitignore` — Excludes `project.db*` from version control while keeping `memories.jsonl` and `rules.md` trackable.
- **Global Store**: `~/.ctxbridge/` (or OS data/home dir)
  - `global.db` — Cross-project developer preferences, patterns, and global lessons.

## 3. CLI Reference

Global flag: `--project <PATH>` overrides the project root directory (defaults to the nearest `.ctxbridge` or `.git` ancestor).

| Command | Purpose |
| :--- | :--- |
| `ctxbridge init` | Initialize `.ctxbridge/` in the current repository and create canonical `rules.md` and `.gitignore`. |
| `ctxbridge save <content>` | Save a memory (`--kind decision\|rule\|fact\|lesson\|todo` [default: `decision`], `--title`, `--tags`, `--priority 1..5` [default: `3`], `--global`, `--agent` [default: `cli`]). |
| `ctxbridge recall [query]` | Hybrid FTS5 + BM25 + priority + recency decay search across project & global memories (`--limit` [default: `8`], `--kind`, `--tag`, `--scope all\|project\|global` [default: `all`], `--json`). |
| `ctxbridge forget <id>` | Permanently delete a memory by UUID or unique prefix (`--global` to target global store). |
| `ctxbridge sync` | Sync canonical rules & pinned memories into `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `CODEX.md`, and `.gemini/GEMINI.md` using non-destructive `<!-- ctxbridge:start -->` ... `<!-- ctxbridge:end -->` blocks (`--existing-only`, `--check` to exit non-zero on drift). |
| `ctxbridge capsule` | Generate a `<500` token Handoff Capsule (`--goal` [default: `"Continue active development"`], `--to` [default: `"any"`], `--budget` [default: `500`], `--format yaml\|markdown` [alias `md`, default: `yaml`]). |
| `ctxbridge capsules` | List recently generated Handoff Capsules (`--limit` [default: `10`], `--json`). |
| `ctxbridge promote <id>` | Promote a project-scoped memory by ID or unique prefix to the global store (`~/.ctxbridge/global.db`). |
| `ctxbridge export` | Export project memories deterministically to `.ctxbridge/memories.jsonl`. |
| `ctxbridge import` | Import and deduplicate memories from `.ctxbridge/memories.jsonl` into `project.db`. |
| `ctxbridge status` | Show project and global store statistics, memory kind breakdown, and adapter sync state (`--json`). |
| `ctxbridge doctor` | Run health, SQLite integrity, `.gitignore`, and adapter sync diagnostics (`--json`). |
| `ctxbridge mcp` | Start the JSON-RPC 2.0 MCP server over `stdin`/`stdout`. |

## 4. MCP Server Tools & Resources (`ctxbridge mcp`)
- **Tools**:
  - `memory_save`: Persist a `decision`, `rule`, `fact`, `lesson`, or `todo` (`scope`: `project` | `global`, `priority`: `1..5`, `tags`, `title`, `source_agent`).
  - `memory_recall`: Search project and global memories with BM25 + priority + recency ranking (`query`, `kind`, `tag`, `scope`: `all` | `project` | `global`, `limit`).
  - `capsule_generate`: Produce a token-budgeted Handoff Capsule (`goal`, `target_agent`, `token_budget`, `format`: `yaml` | `markdown`).
  - `context_sync`: Trigger Universal Adapter sync to repo instruction files (`existing_only`).
  - `memory_forget`: Remove an obsolete or incorrect memory by UUID or prefix (`id`, `scope`: `project` | `global`).
- **Resources**:
  - `ctxbridge://rules` (`text/markdown`): Read canonical `.ctxbridge/rules.md`.
  - `ctxbridge://status` (`application/json`): Read JSON store statistics and adapter sync state.
  - `ctxbridge://capsule/latest` (`text/plain`): Read the most recently generated Handoff Capsule.

## 5. Privacy & Data Deletion
`ctxbridge` runs 100% locally with zero telemetry or cloud transmission. Users can delete individual memories with `ctxbridge forget <id>` or wipe all stored data by removing `.ctxbridge/` and `~/.ctxbridge/`.
