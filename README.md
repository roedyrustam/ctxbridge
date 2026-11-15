# `ctxbridge`

**Local-first cross-agent memory & context bridge (CLI + MCP server)** written in Rust.

`ctxbridge` solves context amnesia and rule fragmentation when switching between AI coding tools (**Antigravity**, **Claude Code**, **Cursor**, **OpenAI Codex CLI**, **Gemini CLI**).

## Features
- **Single Static Binary**: Zero runtime dependencies, `<10 ms` startup, 100% offline-capable.
- **Dual-Scope SQLite + FTS5**: Project-local (`.ctxbridge/project.db`) and global (`~/.ctxbridge/global.db`) memory stores with BM25 + priority ranking.
- **Git-Friendly JSONL Sync**: Deterministic `.ctxbridge/memories.jsonl` export/import with SHA-256 content deduplication.
- **Universal Adapter (`ctxbridge sync`)**: Non-destructively syncs `.ctxbridge/rules.md` and pinned decisions into `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `CODEX.md`, and `.gemini/GEMINI.md` using `<!-- ctxbridge:start -->` ... `<!-- ctxbridge:end -->` markers.
- **Handoff Capsule (`ctxbridge capsule`)**: Generates a token-budgeted (`<500` tokens) YAML/Markdown handoff payload for instant context switching between AI tools.
- **MCP Stdio Server (`ctxbridge mcp`)**: Exposes `memory_save`, `memory_recall`, `capsule_generate`, `context_sync`, and `memory_forget` over JSON-RPC 2.0.

## Quick Start
```bash
cargo build --release
./target/release/ctxbridge init
./target/release/ctxbridge save "Use PostgreSQL RLS for tenant isolation" --kind decision --priority 5 --tags db,security
./target/release/ctxbridge recall "tenant isolation"
./target/release/ctxbridge sync
./target/release/ctxbridge capsule --goal "Implement billing webhook" --to claude
```

## MCP Configuration Example
```json
{
  "mcpServers": {
    "ctxbridge": {
      "command": "ctxbridge",
      "args": ["mcp"]
    }
  }
}
```
