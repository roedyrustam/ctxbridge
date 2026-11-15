use ctxbridge::capsule::generate_capsule;
use ctxbridge::models::{CapsuleFormat, MemoryKind, MemoryScope, RecallQuery, SaveMemoryInput};
use ctxbridge::paths::BridgePaths;
use ctxbridge::service::{collect_status, run_doctor};
use ctxbridge::store::MemoryStore;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn utf8_and_special_chars_fts5_recall() {
    let store = MemoryStore::open_in_memory().unwrap();

    let (utf8_mem, inserted_utf8) = store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Fact,
            title: Some("日本語テスト café 🚀".into()),
            content: "Supports 日本語テスト, café résumé, привет мир, and 🚀 emoji recall".into(),
            source_agent: "verifier".into(),
            priority: 4,
            tags: vec!["i18n".into(), "utf8".into()],
        })
        .unwrap();
    assert!(inserted_utf8);

    let (special_mem, inserted_special) = store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: Some("Special FTS5 chars".into()),
            content:
                r#"Handle "quoted" paths C:\src\main.rs, foo:bar, C++, a*b, (unclosed, 100%, user@example.com"#
                    .into(),
            source_agent: "verifier".into(),
            priority: 5,
            tags: vec!["fts5".into()],
        })
        .unwrap();
    assert!(inserted_special);

    // Multi-byte UTF-8 recall
    let jp_hits = store
        .recall(&RecallQuery {
            text: "日本語テスト".into(),
            kind: None,
            tag: None,
            limit: 5,
        })
        .unwrap();
    assert_eq!(jp_hits.len(), 1);
    assert_eq!(jp_hits[0].memory.id, utf8_mem.id);

    let accented_hits = store
        .recall(&RecallQuery {
            text: "café".into(),
            kind: None,
            tag: None,
            limit: 5,
        })
        .unwrap();
    assert_eq!(accented_hits.len(), 1);
    assert_eq!(accented_hits[0].memory.id, utf8_mem.id);

    // Special FTS5 syntax characters should never cause an SQL/FTS5 parse error
    for query_text in [
        r#""foo:bar""#,
        "C++",
        "a*b",
        "(unclosed",
        r"C:\src\main.rs",
        "100%",
        "user@example.com",
    ] {
        let hits = store
            .recall(&RecallQuery {
                text: query_text.into(),
                kind: None,
                tag: None,
                limit: 5,
            })
            .unwrap();
        assert!(
            hits.iter().any(|h| h.memory.id == special_mem.id),
            "expected special_mem for query {query_text:?}, got {hits:?}"
        );
    }
}

#[test]
fn yaml_capsule_escapes_newlines_quotes_backslashes_and_utf8() {
    let store = MemoryStore::open_in_memory().unwrap();

    let long_utf8 = format!(
        "Line1 \"quoted\" \\path\\to\\file\twith\ttabs\nLine2 {}",
        "漢".repeat(250)
    );

    store
        .save_memory(SaveMemoryInput {
            scope: MemoryScope::Project,
            kind: MemoryKind::Rule,
            title: Some("Escaping & UTF-8".into()),
            content: long_utf8,
            source_agent: "verifier".into(),
            priority: 5,
            tags: vec![r#"tag"quote"#.into(), " ".into(), r"a\b".into()],
        })
        .unwrap();

    let capsule = generate_capsule(
        &store,
        None,
        "Fix \"auth\" bug in C:\\app\\main.rs\nwith\ttabs",
        "agent\\1\"alpha\"\nline2",
        1000,
        CapsuleFormat::Yaml,
    )
    .unwrap();

    let out = &capsule.rendered_output;
    assert!(out.contains(r#"target_agent: "agent\\1\"alpha\"\nline2""#));
    assert!(out.contains(r#"goal: "Fix \"auth\" bug in C:\\app\\main.rs\nwith\ttabs""#));

    let summary_line = out
        .lines()
        .find(|line| line.trim_start().starts_with("summary: "))
        .expect("YAML capsule should contain a summary line");

    assert!(summary_line.contains(r#"\"quoted\""#));
    assert!(summary_line.contains(r"\\path\\to\\file"));
    assert!(summary_line.contains(r#"tag\"quote"#));
    assert!(summary_line.contains(r"a\\b"));
    assert!(summary_line.contains("..."));
    // Empty/whitespace-only tag must be omitted
    assert!(!summary_line.contains(", ]"));

    // Every YAML double-quoted field line should be self-contained on a single line
    for prefix in ["target_agent: ", "goal: ", "summary: "] {
        let line = out
            .lines()
            .find(|l| l.trim_start().starts_with(prefix))
            .unwrap();
        assert!(line.trim_end().ends_with('"'), "unclosed quote on: {line}");
    }
}

#[test]
fn doctor_and_status_on_uninitialized_repo() {
    let project_dir = tempdir().unwrap();
    let global_dir = tempdir().unwrap();
    let root = project_dir.path();
    let home = global_dir.path();

    // Library-level checks on uninitialized BridgePaths
    let mut paths = BridgePaths::resolve(root).unwrap();
    paths.global_dir = home.to_path_buf();
    paths.global_db = home.join("global.db");
    let status = collect_status(&paths).unwrap();
    assert!(!status.project_initialized);
    assert!(status.project_stats.is_none());
    assert!(status.global_stats.is_none());
    assert!(status.sync_targets.is_empty());

    let doctor = run_doctor(&paths).unwrap();
    assert!(!doctor.healthy);
    assert!(!doctor.project_initialized);
    assert!(!doctor.project_db_ok);
    assert_eq!(doctor.global_db_ok, None);
    assert!(!doctor.gitignore_present);
    assert!(!doctor.rules_present);
    assert!(doctor.out_of_sync_targets.is_empty());

    // CLI-level checks on uninitialized repo
    let status_out = Command::new(env!("CARGO_BIN_EXE_ctxbridge"))
        .arg("--project")
        .arg(root)
        .args(["status", "--json"])
        .env("CTXBRIDGE_HOME", home)
        .output()
        .expect("failed to execute ctxbridge status");
    assert!(status_out.status.success());
    let status_json: Value = serde_json::from_slice(&status_out.stdout).unwrap();
    assert_eq!(status_json["project_initialized"], false);
    assert!(status_json["project_stats"].is_null());

    let doctor_out = Command::new(env!("CARGO_BIN_EXE_ctxbridge"))
        .arg("--project")
        .arg(root)
        .args(["doctor", "--json"])
        .env("CTXBRIDGE_HOME", home)
        .output()
        .expect("failed to execute ctxbridge doctor");
    assert!(!doctor_out.status.success());
    let doctor_json: Value = serde_json::from_slice(&doctor_out.stdout).unwrap();
    assert_eq!(doctor_json["healthy"], false);
    assert_eq!(doctor_json["project_initialized"], false);
    assert_eq!(doctor_json["project_db_ok"], false);
    assert_eq!(doctor_json["gitignore_present"], false);
    assert_eq!(doctor_json["rules_present"], false);
}
