use ctxbridge::adapter::{merge_managed_block, sync_adapters, MARKER_END, MARKER_START};
use ctxbridge::capsule::generate_capsule;
use ctxbridge::models::{CapsuleFormat, MemoryKind, MemoryScope, RecallQuery, SaveMemoryInput};
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
