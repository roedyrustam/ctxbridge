use crate::adapter::sync_adapters_with_mode;
use crate::models::{
    HandoffCapsule, MemoryRecord, MemoryScope, RecallQuery, RecallResult, SaveMemoryInput,
    ScopeFilter, StoreStats, SyncStatus, SyncTargetState,
};
use crate::paths::BridgePaths;
use crate::store::MemoryStore;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatusReport {
    pub project_root: String,
    pub project_initialized: bool,
    pub project_stats: Option<StoreStats>,
    pub global_stats: Option<StoreStats>,
    pub sync_targets: Vec<SyncTargetState>,
}

pub fn save_memory_to_scope(
    paths: &BridgePaths,
    input: SaveMemoryInput,
) -> Result<(MemoryRecord, bool)> {
    let db_path = match input.scope {
        MemoryScope::Project => {
            paths.ensure_project_dir()?;
            &paths.project_db
        }
        MemoryScope::Global => {
            paths.ensure_global_dir()?;
            &paths.global_db
        }
    };
    let store = MemoryStore::open(db_path)?;
    store.save_memory(input)
}

pub fn recall_across_stores(
    paths: &BridgePaths,
    query: &RecallQuery,
    scope_filter: ScopeFilter,
) -> Result<Vec<RecallResult>> {
    let limit = if query.limit == 0 { 8 } else { query.limit };
    let mut results = Vec::new();

    if matches!(scope_filter, ScopeFilter::All | ScopeFilter::Project) && paths.project_db.exists()
    {
        let p_store = MemoryStore::open(&paths.project_db)?;
        results.extend(p_store.recall(query)?);
    }
    if matches!(scope_filter, ScopeFilter::All | ScopeFilter::Global) && paths.global_db.exists() {
        let g_store = MemoryStore::open(&paths.global_db)?;
        results.extend(g_store.recall(query)?);
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    Ok(results)
}

pub fn forget_from_scope(
    paths: &BridgePaths,
    id_or_prefix: &str,
    scope: MemoryScope,
) -> Result<usize> {
    let db_path = match scope {
        MemoryScope::Project => &paths.project_db,
        MemoryScope::Global => &paths.global_db,
    };
    if !db_path.exists() {
        return Ok(0);
    }
    let store = MemoryStore::open(db_path)?;
    store.forget(id_or_prefix)
}

pub fn collect_status(paths: &BridgePaths) -> Result<BridgeStatusReport> {
    let project_initialized = paths.project_db.exists();
    let (project_stats, sync_targets) = if project_initialized {
        let store = MemoryStore::open(&paths.project_db)?;
        (Some(store.stats()?), store.list_sync_targets()?)
    } else {
        (None, Vec::new())
    };

    let global_stats = if paths.global_db.exists() {
        Some(MemoryStore::open(&paths.global_db)?.stats()?)
    } else {
        None
    };

    Ok(BridgeStatusReport {
        project_root: paths.project_root.display().to_string(),
        project_initialized,
        project_stats,
        global_stats,
        sync_targets,
    })
}

pub fn promote_memory(paths: &BridgePaths, id_or_prefix: &str) -> Result<(MemoryRecord, bool)> {
    if !paths.project_db.exists() {
        anyhow::bail!("project store not initialized (run `ctxbridge init`)");
    }
    let project_store = MemoryStore::open(&paths.project_db)?;
    let Some(mem) = project_store.get_by_id_or_prefix(id_or_prefix)? else {
        anyhow::bail!("no project memory matched id or prefix '{id_or_prefix}'");
    };

    save_memory_to_scope(
        paths,
        SaveMemoryInput {
            scope: MemoryScope::Global,
            kind: mem.kind,
            title: Some(mem.title),
            content: mem.content,
            source_agent: mem.source_agent,
            priority: mem.priority,
            tags: mem.tags,
        },
    )
}

pub fn list_recent_capsules(paths: &BridgePaths, limit: usize) -> Result<Vec<HandoffCapsule>> {
    if !paths.project_db.exists() {
        return Ok(Vec::new());
    }
    let store = MemoryStore::open(&paths.project_db)?;
    store.list_capsules(limit)
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub healthy: bool,
    pub project_initialized: bool,
    pub project_db_ok: bool,
    pub global_db_ok: Option<bool>,
    pub gitignore_present: bool,
    pub rules_present: bool,
    pub out_of_sync_targets: Vec<String>,
}

pub fn run_doctor(paths: &BridgePaths) -> Result<DoctorReport> {
    let project_initialized = paths.project_db.exists();
    let project_db_ok = if project_initialized {
        MemoryStore::open(&paths.project_db)?.integrity_check()?
    } else {
        false
    };

    let global_db_ok = if paths.global_db.exists() {
        Some(MemoryStore::open(&paths.global_db)?.integrity_check()?)
    } else {
        None
    };

    let gitignore_present = paths.project_dir.join(".gitignore").exists();
    let rules_present = paths.project_rules.exists();

    let out_of_sync_targets = if project_initialized {
        let store = MemoryStore::open(&paths.project_db)?;
        let (reports, _) = sync_adapters_with_mode(
            &paths.project_root,
            &paths.project_rules,
            &store,
            false,
            true,
        )?;
        reports
            .into_iter()
            .filter(|r| matches!(r.status, SyncStatus::Created | SyncStatus::Updated))
            .map(|r| r.file_path)
            .collect()
    } else {
        Vec::new()
    };

    let healthy = project_initialized
        && project_db_ok
        && global_db_ok.unwrap_or(true)
        && rules_present
        && out_of_sync_targets.is_empty();

    Ok(DoctorReport {
        healthy,
        project_initialized,
        project_db_ok,
        global_db_ok,
        gitignore_present,
        rules_present,
        out_of_sync_targets,
    })
}
