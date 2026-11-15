# Entity Relationship Diagram (ERD): `ctxbridge`

## 1. Conceptual Data Model

```mermaid
erDiagram
    MEMORIES ||--o{ MEMORY_TAGS : "has"
    MEMORIES ||--o{ CAPSULE_ITEMS : "included_in"
    CAPSULES ||--o{ CAPSULE_ITEMS : "contains"
    SYNC_TARGETS ||--o{ SYNC_LOGS : "records"

    MEMORIES {
        TEXT id PK "UUID v4"
        TEXT content_hash UK "SHA-256 of normalized scope+kind+content"
        TEXT scope "global | project"
        TEXT kind "decision | rule | fact | lesson | todo"
        TEXT title "Short summary (<=120 chars)"
        TEXT content "Full memory body"
        TEXT source_agent "antigravity | claude | cursor | codex | gemini | cli"
        INTEGER priority "1 (low) to 5 (pinned/critical)"
        INTEGER token_estimate "Approx token count (chars/4)"
        TEXT created_at "ISO-8601 UTC"
        TEXT updated_at "ISO-8601 UTC"
    }

    MEMORY_TAGS {
        TEXT memory_id FK
        TEXT tag
    }

    MEMORIES_FTS {
        TEXT id UNINDEXED
        TEXT title
        TEXT content
        TEXT tags_text
    }

    CAPSULES {
        TEXT id PK "UUID v4"
        TEXT goal "Current task/handoff objective"
        TEXT target_agent "Target AI tool"
        INTEGER token_budget "Default 500"
        TEXT rendered_output "Compact handoff payload"
        TEXT created_at "ISO-8601 UTC"
    }

    CAPSULE_ITEMS {
        TEXT capsule_id FK
        TEXT memory_id FK
        INTEGER rank_order
    }

    SYNC_TARGETS {
        TEXT target_name PK "agents_md | claude_md | cursorrules | codex_md | gemini_md"
        TEXT file_path "Relative path in repo"
        TEXT last_checksum "SHA-256 of last written block"
        TEXT last_synced_at "ISO-8601 UTC"
    }

    SYNC_LOGS {
        TEXT id PK
        TEXT target_name FK
        TEXT status "updated | unchanged | skipped"
        TEXT synced_at "ISO-8601 UTC"
    }
```

## 2. Search & Ranking Strategy
1. **SQLite FTS5 Virtual Table (`memories_fts`)**: Synchronized via triggers (`INSERT`, `UPDATE`, `DELETE`) on `memories` using `unicode61` tokenizer.
2. **Hybrid Scoring**: Combines FTS5 `bm25()` relevance with `priority` boost and recency decay, supporting optional tag filtering (`MEMORY_TAGS`) and scope filtering (`project` vs `global`).
3. **Content-Hash Deduplication**: `content_hash = sha256(scope + "|" + kind + "|" + normalized_content)` ensures idempotent JSONL imports across Git branches and machines.
