# `ctxbridge` — Technical Documentation

## 1. Overview
`ctxbridge` is a local-first Rust CLI and MCP server for synchronizing AI coding context, architectural decisions, and agent rules across Antigravity, Claude Code, Cursor, Codex CLI, and Gemini CLI.

## 2. Directory & Storage Layout
- **Project Store**: `<repo>/.ctxbridge/`
  - `project.db` — SQLite FTS5 database for project-scoped memories & capsules.
  - `memories.jsonl` — Git-friendly append/sync snapshot of project memories.
  - `rules.md` — Canonical project rules source synced to all AI tool instruction files.
- **Global Store**: `~/.ctxbridge/` (or OS config/data dir)
  - `global.db` — Cross-project developer preferences, patterns, and global lessons.

## 3. CLI Reference
| Command | Purpose |
| :--- | :--- |
| `ctxbridge init` | Initialize `.ctxbridge/` in the current repository and create canonical `rules.md`. |
| `ctxbridge save <content>` | Save a memory (`--kind decision\|rule\|fact\|lesson\|todo`, `--title`, `--tags`, `--priority 1..5`, `--global`, `--agent`). |
| `ctxbridge recall <query>` | Hybrid FTS5 + BM25 + priority search across project & global memories (`--limit`, `--kind`, `--json`). |
| `ctxbridge forget <id>` | Permanently delete a memory by ID or prefix. |
| `ctxbridge sync` | Sync canonical rules & pinned memories into `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `CODEX.md`, and `GEMINI.md` using non-destructive marker blocks. |
| `ctxbridge capsule` | Generate a `<500` token Handoff Capsule (`--goal`, `--to`, `--budget`, `--format yaml\|md`). |
| `ctxbridge export` | Export project memories deterministically to `.ctxbridge/memories.jsonl`. |
| `ctxbridge import` | Import and deduplicate memories from `.ctxbridge/memories.jsonl` into `project.db`. |
| `ctxbridge status` | Show store statistics, memory counts, and adapter sync state. |
| `ctxbridge mcp` | Start the JSON-RPC 2.0 MCP server over `stdin`/`stdout`. |

## 4. MCP Server Tools (`ctxbridge mcp`)
- `memory_save`: Persist a decision, rule, fact, lesson, or todo.
- `memory_recall`: Search project and global memories with BM25 + priority ranking.
- `capsule_generate`: Produce a token-budgeted Handoff Capsule for switching agents.
- `context_sync`: Trigger Universal Adapter sync to repo instruction files.
- `memory_forget`: Remove an obsolete or incorrect memory.

## 5. Privacy & Data Deletion
`ctxbridge` runs 100% locally with zero telemetry or cloud transmission. Users can delete individual memories with `ctxbridge forget <id>` or wipe all stored data by removing `.ctxbridge/` and `~/.ctxbridge/`.
