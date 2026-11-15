use crate::models::{
    compute_content_hash, derive_title, estimate_tokens, normalize_tags, HandoffCapsule,
    MemoryRecord, RecallQuery, RecallResult, SaveMemoryInput, StoreStats, SyncTargetReport,
    SyncTargetState,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use uuid::Uuid;

pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create db directory {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open sqlite db at {}", path.display()))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL UNIQUE,
                scope TEXT NOT NULL,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                source_agent TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 3,
                token_estimate INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memory_tags (
                memory_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (memory_id, tag),
                FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_memories_priority_updated
                ON memories(priority DESC, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_tags_tag
                ON memory_tags(tag);

            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                id UNINDEXED,
                title,
                content,
                tags_text,
                tokenize = 'unicode61'
            );

            CREATE TABLE IF NOT EXISTS capsules (
                id TEXT PRIMARY KEY,
                goal TEXT NOT NULL,
                target_agent TEXT NOT NULL,
                token_budget INTEGER NOT NULL,
                token_used INTEGER NOT NULL,
                rendered_output TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS capsule_items (
                capsule_id TEXT NOT NULL,
                memory_id TEXT NOT NULL,
                rank_order INTEGER NOT NULL,
                PRIMARY KEY (capsule_id, memory_id),
                FOREIGN KEY (capsule_id) REFERENCES capsules(id) ON DELETE CASCADE,
                FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS sync_targets (
                target_name TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                last_checksum TEXT NOT NULL,
                last_synced_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sync_logs (
                id TEXT PRIMARY KEY,
                target_name TEXT NOT NULL,
                status TEXT NOT NULL,
                synced_at TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn save_memory(&self, input: SaveMemoryInput) -> Result<(MemoryRecord, bool)> {
        let trimmed = input.content.trim();
        if trimmed.is_empty() {
            anyhow::bail!("memory content cannot be empty");
        }
        let priority = input.priority.clamp(1, 5);
        let tags = normalize_tags(input.tags);
        let content_hash = compute_content_hash(input.scope, input.kind, trimmed);
        let explicit_title = input
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let title = explicit_title
            .clone()
            .unwrap_or_else(|| derive_title(trimmed));
        let token_estimate = estimate_tokens(trimmed);
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let existing_id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM memories WHERE content_hash = ?1",
                params![content_hash],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing_id {
            if let Some(new_title) = explicit_title {
                self.conn.execute(
                    "UPDATE memories SET priority = MAX(priority, ?1), updated_at = ?2, title = ?3 WHERE id = ?4",
                    params![priority as i64, now_str, new_title, id],
                )?;
            } else {
                self.conn.execute(
                    "UPDATE memories SET priority = MAX(priority, ?1), updated_at = ?2 WHERE id = ?3",
                    params![priority as i64, now_str, id],
                )?;
            }
            for tag in &tags {
                self.conn.execute(
                    "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
                    params![id, tag],
                )?;
            }
            self.refresh_fts(&id)?;
            let record = self.get_by_id(&id)?.context("updated memory missing")?;
            return Ok((record, false));
        }

        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            r#"
            INSERT INTO memories (
                id, content_hash, scope, kind, title, content,
                source_agent, priority, token_estimate, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                id,
                content_hash,
                input.scope.to_string(),
                input.kind.to_string(),
                title,
                trimmed,
                input.source_agent,
                priority as i64,
                token_estimate as i64,
                now_str,
                now_str
            ],
        )?;

        for tag in &tags {
            self.conn.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )?;
        }

        let tags_text = tags.join(" ");
        self.conn.execute(
            "INSERT INTO memories_fts (id, title, content, tags_text) VALUES (?1, ?2, ?3, ?4)",
            params![id, title, trimmed, tags_text],
        )?;

        let record = self.get_by_id(&id)?.context("inserted memory missing")?;
        Ok((record, true))
    }

    fn refresh_fts(&self, id: &str) -> Result<()> {
        let Some(record) = self.get_by_id(id)? else {
            return Ok(());
        };
        let tags_text = record.tags.join(" ");
        self.conn
            .execute("DELETE FROM memories_fts WHERE id = ?1", params![id])?;
        self.conn.execute(
            "INSERT INTO memories_fts (id, title, content, tags_text) VALUES (?1, ?2, ?3, ?4)",
            params![record.id, record.title, record.content, tags_text],
        )?;
        Ok(())
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<MemoryRecord>> {
        let row = self
            .conn
            .query_row(
                r#"
                SELECT id, content_hash, scope, kind, title, content,
                       source_agent, priority, token_estimate, created_at, updated_at
                FROM memories
                WHERE id = ?1
                "#,
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                },
            )
            .optional()?;

        match row {
            Some(r) => Ok(Some(self.hydrate_row(r)?)),
            None => Ok(None),
        }
    }

    fn hydrate_row(
        &self,
        r: (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            i64,
            i64,
            String,
            String,
        ),
    ) -> Result<MemoryRecord> {
        let tags = self.get_tags(&r.0)?;
        Ok(MemoryRecord {
            id: r.0,
            content_hash: r.1,
            scope: r.2.parse()?,
            kind: r.3.parse()?,
            title: r.4,
            content: r.5,
            source_agent: r.6,
            priority: r.7.clamp(1, 5) as u8,
            token_estimate: r.8.max(0) as usize,
            tags,
            created_at: DateTime::parse_from_rfc3339(&r.9)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&r.10)?.with_timezone(&Utc),
        })
    }

    fn get_tags(&self, memory_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag ASC")?;
        let rows = stmt.query_map(params![memory_id], |row| row.get::<_, String>(0))?;
        let mut tags = Vec::new();
        for tag in rows {
            tags.push(tag?);
        }
        Ok(tags)
    }

    pub fn recall(&self, query: &RecallQuery) -> Result<Vec<RecallResult>> {
        let limit = if query.limit == 0 { 10 } else { query.limit };
        let fts_expr = build_fts_query(&query.text);

        if let Some(expr) = fts_expr {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT m.id, (-bm25(memories_fts) + (m.priority * 0.35)) AS rank_score
                FROM memories_fts
                JOIN memories m ON m.id = memories_fts.id
                WHERE memories_fts MATCH ?1
                ORDER BY rank_score DESC, m.updated_at DESC
                LIMIT ?2
                "#,
            )?;
            let rows = stmt.query_map(params![expr, (limit * 3) as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;

            let mut results = Vec::new();
            for row in rows {
                let (id, base_score) = row?;
                if let Some(record) = self.get_by_id(&id)? {
                    if matches_filters(&record, query) {
                        let score = base_score + recency_boost(record.updated_at);
                        results.push(RecallResult {
                            memory: record,
                            score,
                        });
                    }
                }
            }
            if !results.is_empty() {
                results.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                results.truncate(limit);
                return Ok(results);
            }
        }

        // Fallback: substring match or recent priority ordering
        let needle = query.text.trim().to_lowercase();
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM memories ORDER BY priority DESC, updated_at DESC LIMIT 200")?;
        let ids = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut results = Vec::new();
        for id_res in ids {
            let id = id_res?;
            if let Some(record) = self.get_by_id(&id)? {
                if !matches_filters(&record, query) {
                    continue;
                }
                if !needle.is_empty() {
                    let haystack = format!(
                        "{} {} {}",
                        record.title.to_lowercase(),
                        record.content.to_lowercase(),
                        record.tags.join(" ")
                    );
                    if !haystack.contains(&needle) {
                        continue;
                    }
                }
                let score = record.priority as f64 * 0.35 + recency_boost(record.updated_at);
                results.push(RecallResult {
                    memory: record,
                    score,
                });
                if results.len() >= limit {
                    break;
                }
            }
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        Ok(results)
    }

    pub fn list_pinned_and_recent(&self, limit: usize) -> Result<Vec<MemoryRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM memories ORDER BY priority DESC, updated_at DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Some(rec) = self.get_by_id(&r?)? {
                out.push(rec);
            }
        }
        Ok(out)
    }

    pub fn resolve_unique_id(&self, id_or_prefix: &str) -> Result<Option<String>> {
        let needle = id_or_prefix.trim();
        if needle.is_empty() {
            anyhow::bail!("memory id or prefix cannot be empty");
        }
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM memories WHERE id = ?1 OR id LIKE ?2")?;
        let like_pattern = format!("{needle}%");
        let matches: Vec<String> = stmt
            .query_map(params![needle, like_pattern], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        if matches.is_empty() {
            return Ok(None);
        }
        if matches.iter().any(|m| m == needle) {
            return Ok(Some(needle.to_string()));
        }
        if matches.len() > 1 {
            anyhow::bail!(
                "ambiguous id prefix '{needle}' matched {} memories; provide a longer prefix",
                matches.len()
            );
        }
        Ok(Some(matches.into_iter().next().unwrap()))
    }

    pub fn get_by_id_or_prefix(&self, id_or_prefix: &str) -> Result<Option<MemoryRecord>> {
        match self.resolve_unique_id(id_or_prefix)? {
            Some(id) => self.get_by_id(&id),
            None => Ok(None),
        }
    }

    pub fn forget(&self, id_or_prefix: &str) -> Result<usize> {
        let Some(id) = self.resolve_unique_id(id_or_prefix)? else {
            return Ok(0);
        };
        self.conn
            .execute("DELETE FROM memories_fts WHERE id = ?1", params![id])?;
        let deleted = self
            .conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(deleted)
    }

    pub fn export_jsonl(&self, path: &Path) -> Result<usize> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM memories ORDER BY created_at ASC, id ASC")?;
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut file = File::create(path)
            .with_context(|| format!("failed to create export file {}", path.display()))?;
        let mut count = 0;
        for id in ids {
            if let Some(record) = self.get_by_id(&id)? {
                let line = serde_json::to_string(&record)?;
                writeln!(file, "{line}")?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn import_jsonl(&self, path: &Path) -> Result<(usize, usize)> {
        let file = File::open(path)
            .with_context(|| format!("failed to open jsonl file {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut imported = 0;
        let mut skipped = 0;

        let tx = self.conn.unchecked_transaction()?;
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let record: MemoryRecord = serde_json::from_str(trimmed)?;
            let (_, inserted) = self.save_memory(SaveMemoryInput {
                scope: record.scope,
                kind: record.kind,
                title: Some(record.title),
                content: record.content,
                source_agent: record.source_agent,
                priority: record.priority,
                tags: record.tags,
            })?;
            if inserted {
                imported += 1;
            } else {
                skipped += 1;
            }
        }
        tx.commit()?;
        Ok((imported, skipped))
    }

    pub fn record_capsule(&self, capsule: &HandoffCapsule) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO capsules (id, goal, target_agent, token_budget, token_used, rendered_output, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                capsule.id,
                capsule.goal,
                capsule.target_agent,
                capsule.token_budget as i64,
                capsule.token_used as i64,
                capsule.rendered_output,
                capsule.created_at.to_rfc3339()
            ],
        )?;
        for (idx, mem_id) in capsule.included_ids.iter().enumerate() {
            self.conn.execute(
                "INSERT OR IGNORE INTO capsule_items (capsule_id, memory_id, rank_order) SELECT ?1, id, ?3 FROM memories WHERE id = ?2",
                params![capsule.id, mem_id, idx as i64],
            )?;
        }
        Ok(())
    }

    pub fn record_sync_reports(&self, reports: &[SyncTargetReport], checksum: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        for report in reports {
            self.conn.execute(
                r#"
                INSERT INTO sync_targets (target_name, file_path, last_checksum, last_synced_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(target_name) DO UPDATE SET
                    file_path = excluded.file_path,
                    last_checksum = excluded.last_checksum,
                    last_synced_at = excluded.last_synced_at
                "#,
                params![report.target_name, report.file_path, checksum, now],
            )?;
            self.conn.execute(
                "INSERT INTO sync_logs (id, target_name, status, synced_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    report.target_name,
                    report.status.to_string(),
                    now
                ],
            )?;
        }
        Ok(())
    }

    pub fn stats(&self) -> Result<StoreStats> {
        let total_memories: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        let pinned_memories: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE priority >= 4",
            [],
            |row| row.get(0),
        )?;
        let total_capsules: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM capsules", [], |row| row.get(0))?;

        let mut stmt = self
            .conn
            .prepare("SELECT kind, COUNT(*) FROM memories GROUP BY kind ORDER BY kind ASC")?;
        let by_kind = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(StoreStats {
            total_memories: total_memories as usize,
            pinned_memories: pinned_memories as usize,
            total_capsules: total_capsules as usize,
            by_kind,
        })
    }

    pub fn list_sync_targets(&self) -> Result<Vec<SyncTargetState>> {
        let mut stmt = self.conn.prepare(
            "SELECT target_name, file_path, last_checksum, last_synced_at FROM sync_targets ORDER BY target_name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SyncTargetState {
                target_name: row.get(0)?,
                file_path: row.get(1)?,
                last_checksum: row.get(2)?,
                last_synced_at: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_capsules(&self, limit: usize) -> Result<Vec<HandoffCapsule>> {
        let cap_limit = if limit == 0 { 10 } else { limit };
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, goal, target_agent, token_budget, token_used, rendered_output, created_at
            FROM capsules
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![cap_limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;

        let mut capsules = Vec::new();
        for row in rows {
            let (id, goal, target_agent, token_budget, token_used, rendered_output, created_at) =
                row?;
            let mut item_stmt = self.conn.prepare(
                "SELECT memory_id FROM capsule_items WHERE capsule_id = ?1 ORDER BY rank_order ASC",
            )?;
            let included_ids = item_stmt
                .query_map(params![id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            capsules.push(HandoffCapsule {
                id,
                goal,
                target_agent,
                token_budget: token_budget.max(0) as usize,
                token_used: token_used.max(0) as usize,
                rendered_output,
                included_ids,
                created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
            });
        }
        Ok(capsules)
    }

    pub fn integrity_check(&self) -> Result<bool> {
        let status: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        Ok(status.eq_ignore_ascii_case("ok"))
    }
}

fn recency_boost(updated_at: DateTime<Utc>) -> f64 {
    let age_secs = (Utc::now() - updated_at).num_seconds().max(0) as f64;
    let age_days = age_secs / 86_400.0;
    0.25 / (1.0 + age_days * 0.1)
}

fn matches_filters(record: &MemoryRecord, query: &RecallQuery) -> bool {
    if let Some(kind) = query.kind {
        if record.kind != kind {
            return false;
        }
    }
    if let Some(tag) = &query.tag {
        let normalized = tag.trim().to_lowercase();
        if !normalized.is_empty() && !record.tags.iter().any(|t| t == &normalized) {
            return false;
        }
    }
    true
}

fn build_fts_query(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" OR "))
    }
}
