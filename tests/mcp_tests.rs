use ctxbridge::mcp::run_server_with_io;
use ctxbridge::paths::BridgePaths;
use serde_json::{json, Value};
use std::fs;
use std::io::Cursor;
use tempfile::tempdir;

fn run_mcp_session(paths: &BridgePaths, requests: Vec<Value>) -> Vec<Value> {
    let input_lines: String = requests
        .into_iter()
        .map(|v| format!("{}\n", serde_json::to_string(&v).unwrap()))
        .collect();
    let reader = Cursor::new(input_lines.into_bytes());
    let mut output = Vec::new();
    run_server_with_io(paths, reader, &mut output).unwrap();
    let out_str = String::from_utf8(output).unwrap();
    out_str
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn mcp_initialize_tools_resources_and_roundtrip() {
    let project_dir = tempdir().unwrap();
    let global_dir = tempdir().unwrap();
    let mut paths = BridgePaths::resolve(project_dir.path()).unwrap();
    paths.global_dir = global_dir.path().to_path_buf();
    paths.global_db = paths.global_dir.join("global.db");
    paths.ensure_project_dir().unwrap();
    fs::write(&paths.project_rules, "- Never panic in library code.").unwrap();

    let responses = run_mcp_session(
        &paths,
        vec![
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
            // Notification (no id) should produce no response
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized", "params": {} }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "memory_save",
                    "arguments": {
                        "content": "Use deterministic SHA-256 content hashes for JSONL sync",
                        "kind": "decision",
                        "priority": 5,
                        "tags": ["sync", "sqlite"]
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "memory_recall",
                    "arguments": { "query": "SHA-256", "scope": "project" }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "capsule_generate",
                    "arguments": { "goal": "Verify MCP capsule", "token_budget": 300 }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": { "name": "context_sync", "arguments": { "existing_only": false } }
            }),
            json!({ "jsonrpc": "2.0", "id": 8, "method": "resources/list" }),
            json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "resources/read",
                "params": { "uri": "ctxbridge://rules" }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "resources/read",
                "params": { "uri": "ctxbridge://status" }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "resources/read",
                "params": { "uri": "ctxbridge://capsule/latest" }
            }),
            json!({ "jsonrpc": "2.0", "id": 12, "method": "unknown/method" }),
        ],
    );

    assert_eq!(responses.len(), 12);

    // 1. initialize
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "ctxbridge");
    assert!(responses[0]["result"]["capabilities"]["resources"].is_object());

    // 2. ping
    assert_eq!(responses[1]["id"], 2);
    assert!(responses[1]["error"].is_null());

    // 3. tools/list
    let tools = responses[2]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);

    // 4. memory_save
    assert_eq!(responses[3]["result"]["isError"], false);
    let save_text = responses[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(save_text.contains("created"));

    // 5. memory_recall
    assert_eq!(responses[4]["result"]["isError"], false);
    let recall_text = responses[4]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(recall_text.contains("SHA-256"));

    // 6. capsule_generate
    assert_eq!(responses[5]["result"]["isError"], false);
    let capsule_text = responses[5]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(capsule_text.contains("ctxbridge_capsule"));

    // 7. context_sync
    assert_eq!(responses[6]["result"]["isError"], false);
    assert!(project_dir.path().join("AGENTS.md").exists());

    // 8. resources/list
    let resources = responses[7]["result"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 3);

    // 9. resources/read ctxbridge://rules
    let rules_text = responses[8]["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(rules_text.contains("Never panic in library code."));

    // 10. resources/read ctxbridge://status
    let status_text = responses[9]["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(status_text.contains("\"project_initialized\": true"));

    // 11. resources/read ctxbridge://capsule/latest
    let latest_cap_text = responses[10]["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(latest_cap_text.contains("Verify MCP capsule"));

    // 12. unknown/method returns JSON-RPC -32601
    assert_eq!(responses[11]["error"]["code"], -32601);
}
