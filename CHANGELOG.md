# Changelog

All notable changes to `ctxbridge` are documented in this file.

## [0.2.0] - 2026-11-15

### Added
- **Service Layer (`src/service.rs`)**: Shared application orchestration for CLI and MCP server operations (`save_memory_to_scope`, `recall_across_stores`, `forget_from_scope`, `collect_status`, `promote_memory`, `list_recent_capsules`, `run_doctor`).
- **Recency Decay Ranking**: Memory recall now combines SQLite FTS5 BM25 relevance, priority weighting, and time-based recency decay.
- **New CLI Commands**:
  - `ctxbridge promote <id>` to promote a project memory to the global store (`~/.ctxbridge/global.db`).
  - `ctxbridge capsules` (`--limit`, `--json`) to inspect recently generated Handoff Capsules.
  - `ctxbridge doctor` (`--json`) to run health, SQLite integrity, `.gitignore`, and adapter sync diagnostics.
- **CLI Flags & Modes**:
  - `ctxbridge sync --check` to verify instruction files are in sync without modifying them (exits non-zero on drift).
  - `ctxbridge recall --scope all|project|global` to filter recall across stores.
  - `--json` output support for `status`, `doctor`, `capsules`, and `recall`.
- **MCP Resources (`ctxbridge mcp`)**:
  - `ctxbridge://rules` (`text/markdown`)
  - `ctxbridge://status` (`application/json`)
  - `ctxbridge://capsule/latest` (`text/plain`)
- **CI/CD Workflows**:
  - `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` across Linux, Windows, and macOS.
  - `.github/workflows/release.yml` building release binaries for Linux (`x86_64`), Windows (`x86_64`), and macOS (`x86_64`/`aarch64`) with SHA-256 checksums.

### Fixed & Hardened
- Proper YAML double-quoted string escaping in Handoff Capsule rendering (`render_header` and `format_entry`) for backslashes, newlines, tabs, and quotes.
- UTF-8 safe string truncation in `compact_body` and `derive_title` to prevent panics on multi-byte characters.
- Edge-case handling for empty/whitespace tags, relative project root resolution, and FTS5 special-character queries.

## [0.1.0] - 2026-11-14

### Added
- Initial local-first Rust CLI and JSON-RPC 2.0 MCP stdio server (`ctxbridge`).
- Dual-scope SQLite + FTS5 storage (`.ctxbridge/project.db` and `~/.ctxbridge/global.db`) with BM25 + priority ranking.
- Core CLI commands: `init`, `save`, `recall`, `forget`, `sync`, `capsule`, `export`, `import`, `status`, and `mcp`.
- Git-friendly deterministic `.ctxbridge/memories.jsonl` export/import with SHA-256 content deduplication.
- Universal Adapter (`ctxbridge sync`) syncing `.ctxbridge/rules.md` and pinned memories into `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `CODEX.md`, and `.gemini/GEMINI.md` via non-destructive markers.
- Token-budgeted Handoff Capsule generator (`yaml` and `markdown` formats).
- MCP tools: `memory_save`, `memory_recall`, `capsule_generate`, `context_sync`, and `memory_forget`.
