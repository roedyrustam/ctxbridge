# Product Requirements Document (PRD): `ctxbridge`

## 1. Executive Summary
`ctxbridge` is a **local-first, single-binary Rust CLI and Model Context Protocol (MCP) server** that bridges episodic memory, architectural decisions, and project rules across AI coding assistants and IDEs (Antigravity, Claude Code, Cursor, OpenAI Codex CLI, Gemini CLI).

## 2. Problem Statement
AI developers frequently switch between multiple AI coding tools depending on model strengths, rate limits, or environment. Every switch causes:
1. **Context Amnesia**: Architectural decisions, constraints, and debugging lessons stay trapped in one tool's session history.
2. **Rule Fragmentation**: Maintaining separate `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `CODEX.md`, and `GEMINI.md` files leads to drift and contradictions.
3. **Token Waste**: Re-explaining project background across tools wastes thousands of tokens per session.

## 3. Target Audience
- Individual AI-native software engineers using 2+ AI coding tools/IDEs.
- Engineering teams sharing repo-level AI conventions and architectural decision records via Git.

## 4. Core Features (v1.0)
| ID | Feature | Description |
| :--- | :--- | :--- |
| **F1** | **Local-First Dual-Scope SQLite Store** | Global (`~/.ctxbridge/global.db`) and project-scoped (`.ctxbridge/project.db`) SQLite databases with FTS5 full-text + BM25 ranking. |
| **F2** | **Git-Friendly JSONL Sync** | Deterministic export/import (`.ctxbridge/memories.jsonl`) with SHA-256 content-hash deduplication for clean Git diffs and team sharing. |
| **F3** | **Universal Adapter (`ctxbridge sync`)** | Bidirectional/canonical sync from `.ctxbridge/rules.md` and top pinned memories into `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `CODEX.md`, and `GEMINI.md` using managed marker blocks. |
| **F4** | **Handoff Capsule (`ctxbridge capsule`)** | Generates a compact (`<500` token budget) structured YAML/Markdown capsule containing active goal, pinned rules, recent architectural decisions, and next actions for instant tool switching. |
| **F5** | **MCP Stdio Server (`ctxbridge mcp`)** | JSON-RPC 2.0 MCP server exposing `memory_save`, `memory_recall`, `capsule_generate`, `context_sync`, and `memory_forget` tools. |

## 5. Non-Functional Requirements (NFRs)
- **Startup Latency**: `< 10 ms` cold CLI startup (single static Rust binary, zero runtime dependencies).
- **Offline Capability**: 100% functional offline with SQLite FTS5 + BM25; zero mandatory external API keys.
- **Data Ownership & Privacy**: All data remains strictly local on user disk; users can inspect, export, or permanently purge any memory (`ctxbridge forget <id>` or deleting `.ctxbridge/`).
- **Non-Destructive File Sync**: Adapter sync only updates content inside `<!-- ctxbridge:start -->` ... `<!-- ctxbridge:end -->` markers, preserving user-authored sections.

## 6. Out of Scope (v1.0)
- Cloud-hosted multi-tenant sync server.
- External paid embedding API integrations.
- GUI desktop application.
