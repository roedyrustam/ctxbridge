use ctxbridge::adapter::{
    merge_managed_block, sync_adapters, sync_adapters_with_mode, MARKER_END, MARKER_START,
};
use ctxbridge::capsule::generate_capsule;
use ctxbridge::models::{
    CapsuleFormat, MemoryKind, MemoryScope, RecallQuery, SaveMemoryInput, ScopeFilter, SyncStatus,
};
use ctxbridge::paths::BridgePaths;
use ctxbridge::service::{collect_status, recall_across_stores, save_memory_to_scope};
use ctxbridge::store::MemoryStore;
use std::fs;
use tempfile::tempdir;

#[test]
fn save_dedup_recall_and_forget_work() {
    let store = MemoryStore::open_in_memory().unwrap();

    let (first, inserted) = store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: Some("Use SQLite FTS5".into()),
            content: "Use local SQLite with FTS5 and BM25 ranking for offline recall".into(),
            source_agent: "antigravity".into(),
            priority: 4,
            tags: vec!["sqlite".into(), "search".into()],
        })
        .unwrap();
    assert!(inserted);

    // Duplicate normalized content should deduplicate and bump priority if higher
    let (second, inserted_again) = store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: Some("Use SQLite FTS5".into()),
            content: "Use   local SQLite with FTS5 and BM25 ranking for offline recall".into(),
            source_agent: "claude".into(),
            priority: 5,
            tags: vec!["offline".into()],
        })
        .unwrap();
    assert!(!inserted_again);
    assert_eq!(first.id, second.id);
    assert_eq!(second.priority, 5);
    assert!(second.tags.contains(&"offline".to_string()));

    let results = store
        .recall(&RecallQuery {
            text: "BM25 offline".into(),
            kind: Some(MemoryKind::Decision),
            tag: Some("sqlite".into()),
            limit: 5,
        })
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].memory.id, first.id);

    let prefix = &first.id[..8];
    let deleted = store.forget(prefix).unwrap();
    assert_eq!(deleted, 1);

    let after = store
        .recall(&RecallQuery {
            text: "BM25".into(),
            kind: None,
            tag: None,
            limit: 5,
        })
        .unwrap();
    assert!(after.is_empty());
}

#[test]
fn jsonl_export_and_idempotent_import() {
    let dir = tempdir().unwrap();
    let jsonl_path = dir.path().join("memories.jsonl");

    let store_a = MemoryStore::open_in_memory().unwrap();
    store_a
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Rule,
            title: None,
            content: "Never commit .env secrets to Git".into(),
            source_agent: "cursor".into(),
            priority: 5,
            tags: vec!["security".into()],
        })
        .unwrap();

    let exported = store_a.export_jsonl(&jsonl_path).unwrap();
    assert_eq!(exported, 1);

    let store_b = MemoryStore::open_in_memory().unwrap();
    let (imp1, skip1) = store_b.import_jsonl(&jsonl_path).unwrap();
    assert_eq!((imp1, skip1), (1, 0));

    let (imp2, skip2) = store_b.import_jsonl(&jsonl_path).unwrap();
    assert_eq!((imp2, skip2), (0, 1));
}

#[test]
fn adapter_sync_is_non_destructive_and_idempotent() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let rules_path = root.join(".ctxbridge").join("rules.md");
    fs::create_dir_all(rules_path.parent().unwrap()).unwrap();
    fs::write(&rules_path, "- Always run cargo test before commit.").unwrap();

    // Existing CLAUDE.md with custom user header & footer
    let claude_path = root.join("CLAUDE.md");
    fs::write(
        &claude_path,
        format!("# My Custom Header\n\n{MARKER_START}\nold\n{MARKER_END}\n\n# Footer\n"),
    )
    .unwrap();

    let store = MemoryStore::open_in_memory().unwrap();
    store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: Some("Rust Single Binary".into()),
            content: "Compile as a single static Rust binary with bundled SQLite".into(),
            source_agent: "antigravity".into(),
            priority: 5,
            tags: vec!["rust".into()],
        })
        .unwrap();

    let (reports1, _) = sync_adapters(root, &rules_path, &store, false).unwrap();
    assert_eq!(reports1.len(), 5);

    let claude_after = fs::read_to_string(&claude_path).unwrap();
    assert!(claude_after.contains("# My Custom Header"));
    assert!(claude_after.contains("# Footer"));
    assert!(claude_after.contains("Always run cargo test before commit."));
    assert!(claude_after.contains("Rust Single Binary"));

    // Second sync should report unchanged for all 5 targets
    let (reports2, _) = sync_adapters(root, &rules_path, &store, false).unwrap();
    assert!(reports2
        .iter()
        .all(|r| format!("{}", r.status) == "unchanged"));

    let merged = merge_managed_block("custom", &format!("{MARKER_START}\nnew\n{MARKER_END}"));
    assert!(merged.starts_with("custom\n\n"));
}

#[test]
fn capsule_respects_token_budget_and_prioritizes_rules() {
    let store = MemoryStore::open_in_memory().unwrap();
    store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Rule,
            title: Some("Critical Rule".into()),
            content: "Always enforce strict error handling without unwrap in library code.".into(),
            source_agent: "antigravity".into(),
            priority: 5,
            tags: vec!["rust".into()],
        })
        .unwrap();

    for i in 0..20 {
        store
            .save_memory(SaveMemoryInput {
                scope: MemoryScope::Project,
                kind: MemoryKind::Fact,
                title: Some(format!("Background fact {i}")),
                content: format!(
                    "Long background note number {i} explaining historical context that should be trimmed when token budget is tight."
                ),
                source_agent: "codex".into(),
                priority: 1,
                tags: vec![],
            })
            .unwrap();
    }

    let capsule =
        generate_capsule(&store, None, "Refactor MCP handler", "claude", 250, CapsuleFormat::Yaml)
            .unwrap();

    assert!(capsule.token_used <= 250);
    assert!(capsule.rendered_output.contains("Critical Rule") || capsule.rendered_output.contains("strict error handling"));
}

#[test]
fn dedup_preserves_custom_title_when_none_and_rejects_ambiguous_forget() {
    let store = MemoryStore::open_in_memory().unwrap();
    let (first, _) = store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: Some("Custom Architectural Title".into()),
            content: "Use WAL mode for concurrent SQLite reads".into(),
            source_agent: "antigravity".into(),
            priority: 3,
            tags: vec![],
        })
        .unwrap();

    // Second save with title: None should preserve the existing custom title
    let (second, inserted) = store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: None,
            content: "Use WAL mode for concurrent SQLite reads".into(),
            source_agent: "claude".into(),
            priority: 4,
            tags: vec!["sqlite".into()],
        })
        .unwrap();
    assert!(!inserted);
    assert_eq!(second.id, first.id);
    assert_eq!(second.title, "Custom Architectural Title");
    assert_eq!(second.priority, 4);

    // Save a second memory so empty prefix / shared prefix can be tested
    store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Fact,
            title: None,
            content: "Another distinct memory".into(),
            source_agent: "cli".into(),
            priority: 2,
            tags: vec![],
        })
        .unwrap();

    // Empty query fallback lists both memories ordered by priority desc
    let all = store
        .recall(&RecallQuery {
            text: "".into(),
            kind: None,
            tag: None,
            limit: 10,
        })
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].memory.id, first.id);
}

#[test]
fn sync_check_mode_does_not_write_files_and_status_reports_targets() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let paths = BridgePaths::resolve(root).unwrap();
    paths.ensure_project_dir().unwrap();
    fs::write(&paths.project_rules, "- Always verify with cargo test.").unwrap();
    let store = MemoryStore::open(&paths.project_db).unwrap();

    // Check mode should report Created without actually creating files
    let (check_reports, _) =
        sync_adapters_with_mode(root, &paths.project_rules, &store, false, true).unwrap();
    assert!(check_reports
        .iter()
        .all(|r| r.status == SyncStatus::Created));
    assert!(!root.join("AGENTS.md").exists());
    assert!(store.list_sync_targets().unwrap().is_empty());

    // Actual sync writes files and records sync_targets
    let (sync_reports, _) =
        sync_adapters_with_mode(root, &paths.project_rules, &store, false, false).unwrap();
    assert!(sync_reports
        .iter()
        .all(|r| r.status == SyncStatus::Created));
    assert!(root.join("AGENTS.md").exists());

    let status = collect_status(&paths).unwrap();
    assert!(status.project_initialized);
    assert_eq!(status.sync_targets.len(), 5);

    // Subsequent check mode should report all Unchanged
    let (recheck, _) =
        sync_adapters_with_mode(root, &paths.project_rules, &store, false, true).unwrap();
    assert!(recheck.iter().all(|r| r.status == SyncStatus::Unchanged));
}

#[test]
fn service_recall_respects_scope_filter() {
    let project_dir = tempdir().unwrap();
    let global_dir = tempdir().unwrap();
    let mut paths = BridgePaths::resolve(project_dir.path()).unwrap();
    paths.global_dir = global_dir.path().to_path_buf();
    paths.global_db = paths.global_dir.join("global.db");

    save_memory_to_scope(
        &paths,
        SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: Some("Project Auth".into()),
            content: "Project uses JWT auth".into(),
            source_agent: "cli".into(),
            priority: 4,
            tags: vec![],
        },
    )
    .unwrap();

    save_memory_to_scope(
        &paths,
        SaveMemoryInput {
            scope: MemoryScope::Global,
            kind: MemoryKind::Lesson,
            title: Some("Global Auth".into()),
            content: "Always rotate auth signing keys".into(),
            source_agent: "cli".into(),
            priority: 4,
            tags: vec![],
        },
    )
    .unwrap();

    let query = RecallQuery {
        text: "auth".into(),
        kind: None,
        tag: None,
        limit: 10,
    };

    let only_project = recall_across_stores(&paths, &query, ScopeFilter::Project).unwrap();
    assert_eq!(only_project.len(), 1);
    assert_eq!(only_project[0].memory.scope, MemoryScope::Project);

    let only_global = recall_across_stores(&paths, &query, ScopeFilter::Global).unwrap();
    assert_eq!(only_global.len(), 1);
    assert_eq!(only_global[0].memory.scope, MemoryScope::Global);

    let both = recall_across_stores(&paths, &query, ScopeFilter::All).unwrap();
    assert_eq!(both.len(), 2);
}
