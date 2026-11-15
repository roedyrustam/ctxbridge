use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn run_cli(project_dir: &Path, global_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ctxbridge"))
        .arg("--project")
        .arg(project_dir)
        .args(args)
        .env("CTXBRIDGE_HOME", global_dir)
        .output()
        .expect("failed to execute ctxbridge binary")
}

#[test]
fn cli_full_workflow_end_to_end() {
    let project_dir = tempdir().unwrap();
    let global_dir = tempdir().unwrap();
    let root = project_dir.path();
    let home = global_dir.path();

    // 1. init creates .ctxbridge/, rules.md, and .gitignore
    let init_out = run_cli(root, home, &["init"]);
    assert!(init_out.status.success(), "{:?}", init_out);
    assert!(root.join(".ctxbridge").join("rules.md").exists());
    assert!(root.join(".ctxbridge").join(".gitignore").exists());

    // 2. sync --check fails before sync, succeeds after sync
    let check_before = run_cli(root, home, &["sync", "--check"]);
    assert!(!check_before.status.success());

    let sync_out = run_cli(root, home, &["sync"]);
    assert!(sync_out.status.success());

    let check_after = run_cli(root, home, &["sync", "--check"]);
    assert!(check_after.status.success());

    // 3. save + recall --json
    let save_out = run_cli(
        root,
        home,
        &[
            "save",
            "Use PostgreSQL RLS for tenant isolation",
            "--kind",
            "decision",
            "--priority",
            "5",
            "--tags",
            "db,security",
        ],
    );
    assert!(save_out.status.success());

    let recall_out = run_cli(root, home, &["recall", "tenant", "--scope", "project", "--json"]);
    assert!(recall_out.status.success());
    let recalled: Value = serde_json::from_slice(&recall_out.stdout).unwrap();
    let arr = recalled.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let mem_id = arr[0]["memory"]["id"].as_str().unwrap().to_string();

    // 4. promote to global and recall --scope global
    let promote_out = run_cli(root, home, &["promote", &mem_id[..8]]);
    assert!(promote_out.status.success());

    let global_recall = run_cli(root, home, &["recall", "tenant", "--scope", "global", "--json"]);
    assert!(global_recall.status.success());
    let global_arr: Value = serde_json::from_slice(&global_recall.stdout).unwrap();
    assert_eq!(global_arr.as_array().unwrap().len(), 1);

    // 5. capsule + capsules --json
    let cap_out = run_cli(
        root,
        home,
        &["capsule", "--goal", "Audit RLS policies", "--to", "claude", "--budget", "300"],
    );
    assert!(
        cap_out.status.success(),
        "capsule failed: {}",
        String::from_utf8_lossy(&cap_out.stderr)
    );

    let caps_list = run_cli(root, home, &["capsules", "--json"]);
    assert!(caps_list.status.success());
    let caps_json: Value = serde_json::from_slice(&caps_list.stdout).unwrap();
    assert_eq!(caps_json.as_array().unwrap().len(), 1);

    // 6. export + import
    let export_out = run_cli(root, home, &["export"]);
    assert!(export_out.status.success());
    assert!(root.join(".ctxbridge").join("memories.jsonl").exists());

    let import_out = run_cli(root, home, &["import"]);
    assert!(import_out.status.success());

    // 7. status --json & doctor --json (note: sync again since we saved a p5 decision)
    let resync = run_cli(root, home, &["sync"]);
    assert!(resync.status.success());

    let status_out = run_cli(root, home, &["status", "--json"]);
    assert!(status_out.status.success());
    let status_json: Value = serde_json::from_slice(&status_out.stdout).unwrap();
    assert_eq!(status_json["project_initialized"], true);

    let doctor_out = run_cli(root, home, &["doctor", "--json"]);
    assert!(doctor_out.status.success());
    let doctor_json: Value = serde_json::from_slice(&doctor_out.stdout).unwrap();
    assert_eq!(doctor_json["healthy"], true);
}
