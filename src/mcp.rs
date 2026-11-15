use crate::adapter::sync_adapters;
use crate::capsule::generate_capsule;
use crate::models::{
    CapsuleFormat, MemoryKind, MemoryScope, RecallQuery, SaveMemoryInput, ScopeFilter,
};
use crate::paths::BridgePaths;
use crate::service::{
    collect_status, forget_from_scope, list_recent_capsules, recall_across_stores,
    save_memory_to_scope,
};
use crate::store::MemoryStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Write};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

pub fn run_stdio_server(paths: &BridgePaths) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_server_with_io(paths, stdin.lock(), stdout.lock())
}

pub fn run_server_with_io<R: BufRead, W: Write>(
    paths: &BridgePaths,
    reader: R,
    mut out: W,
) -> Result<()> {
    for line_res in reader.lines() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {err}"),
                    }),
                };
                writeln!(out, "{}", serde_json::to_string(&resp)?)?;
                out.flush()?;
                continue;
            }
        };

        // Notifications have no id and expect no response
        let Some(id) = req.id.clone() else {
            continue;
        };

        let response = handle_request(paths, id, &req.method, &req.params);
        writeln!(out, "{}", serde_json::to_string(&response)?)?;
        out.flush()?;
    }

    Ok(())
}

fn handle_request(
    paths: &BridgePaths,
    id: Value,
    method: &str,
    params: &Value,
) -> JsonRpcResponse {
    match method {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "ctxbridge",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        },
        "ping" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({})),
            error: None,
        },
        "resources/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({ "resources": resource_definitions() })),
            error: None,
        },
        "resources/read" => {
            let uri = params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            match read_resource(paths, uri) {
                Ok(contents) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(json!({ "contents": [contents] })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Resource error: {err}"),
                    }),
                },
            }
        }
        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({ "tools": tool_definitions() })),
            error: None,
        },
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match execute_tool(paths, name, &args) {
                Ok(text) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": false
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(json!({
                        "content": [{ "type": "text", "text": format!("Error: {err}") }],
                        "isError": true
                    })),
                    error: None,
                },
            }
        }
        other => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {other}"),
            }),
        },
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "memory_save",
            "description": "Save a cross-agent architectural decision, project rule, fact, lesson, or todo into ctxbridge local-first store.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "Memory content to persist" },
                    "kind": { "type": "string", "enum": ["decision", "rule", "fact", "lesson", "todo"], "default": "decision" },
                    "scope": { "type": "string", "enum": ["project", "global"], "default": "project" },
                    "title": { "type": "string", "description": "Optional short summary" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "priority": { "type": "integer", "minimum": 1, "maximum": 5, "default": 3 },
                    "source_agent": { "type": "string", "default": "mcp" }
                },
                "required": ["content"]
            }
        },
        {
            "name": "memory_recall",
            "description": "Recall relevant memories using hybrid SQLite FTS5 + BM25 + priority ranking across project and global stores.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search keywords or topic" },
                    "kind": { "type": "string", "enum": ["decision", "rule", "fact", "lesson", "todo"] },
                    "tag": { "type": "string" },
                    "scope": { "type": "string", "enum": ["all", "project", "global"], "default": "all" },
                    "limit": { "type": "integer", "default": 8 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "capsule_generate",
            "description": "Generate a compact (<500 token) Handoff Capsule for switching between AI tools (Antigravity, Claude Code, Cursor, Codex, Gemini CLI).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "goal": { "type": "string", "description": "Current objective or task being handed off" },
                    "target_agent": { "type": "string", "default": "any" },
                    "token_budget": { "type": "integer", "default": 500 },
                    "format": { "type": "string", "enum": ["yaml", "markdown"], "default": "yaml" }
                },
                "required": ["goal"]
            }
        },
        {
            "name": "context_sync",
            "description": "Synchronize canonical .ctxbridge/rules.md and pinned memories to AGENTS.md, CLAUDE.md, .cursorrules, CODEX.md, and .gemini/GEMINI.md.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "existing_only": { "type": "boolean", "default": false }
                }
            }
        },
        {
            "name": "memory_forget",
            "description": "Permanently delete a memory by its UUID or unique prefix.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Memory ID or prefix" },
                    "scope": { "type": "string", "enum": ["project", "global"], "default": "project" }
                },
                "required": ["id"]
            }
        }
    ])
}

fn execute_tool(paths: &BridgePaths, name: &str, args: &Value) -> Result<String> {
    match name {
        "memory_save" => {
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing 'content'"))?
                .to_string();
            let scope: MemoryScope = args
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("project")
                .parse()?;
            let kind: MemoryKind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("decision")
                .parse()?;
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let tags = args
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.as_str().map(ToString::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let priority = args
                .get("priority")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u8;
            let source_agent = args
                .get("source_agent")
                .and_then(|v| v.as_str())
                .unwrap_or("mcp")
                .to_string();

            let (record, inserted) = save_memory_to_scope(
                paths,
                SaveMemoryInput {
                    scope,
                    kind,
                    title,
                    content,
                    source_agent,
                    priority,
                    tags,
                },
            )?;
            Ok(serde_json::to_string_pretty(&json!({
                "status": if inserted { "created" } else { "deduplicated_updated" },
                "memory": record
            }))?)
        }
        "memory_recall" => {
            let text = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(str::parse)
                .transpose()?;
            let tag = args
                .get("tag")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let scope_filter: ScopeFilter = args
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("all")
                .parse()?;
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(8) as usize;

            let query = RecallQuery {
                text,
                kind,
                tag,
                limit,
            };

            let combined = recall_across_stores(paths, &query, scope_filter)?;
            Ok(serde_json::to_string_pretty(&combined)?)
        }
        "capsule_generate" => {
            let goal = args
                .get("goal")
                .and_then(|v| v.as_str())
                .unwrap_or("Continue active development");
            let target_agent = args
                .get("target_agent")
                .and_then(|v| v.as_str())
                .unwrap_or("any");
            let token_budget = args
                .get("token_budget")
                .and_then(|v| v.as_u64())
                .unwrap_or(500) as usize;
            let format: CapsuleFormat = args
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("yaml")
                .parse()?;

            paths.ensure_project_dir()?;
            let project_store = MemoryStore::open(&paths.project_db)?;
            let global_store = if paths.global_db.exists() {
                Some(MemoryStore::open(&paths.global_db)?)
            } else {
                None
            };
            let capsule = generate_capsule(
                &project_store,
                global_store.as_ref(),
                goal,
                target_agent,
                token_budget,
                format,
            )?;
            Ok(capsule.rendered_output)
        }
        "context_sync" => {
            let existing_only = args
                .get("existing_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            paths.ensure_project_dir()?;
            let project_store = MemoryStore::open(&paths.project_db)?;
            let (reports, checksum) = sync_adapters(
                &paths.project_root,
                &paths.project_rules,
                &project_store,
                existing_only,
            )?;
            Ok(serde_json::to_string_pretty(&json!({
                "checksum": checksum,
                "targets": reports
            }))?)
        }
        "memory_forget" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing 'id'"))?;
            let scope: MemoryScope = args
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("project")
                .parse()?;
            let deleted = forget_from_scope(paths, id, scope)?;
            Ok(json!({ "deleted": deleted }).to_string())
        }
        other => anyhow::bail!("unknown tool: {other}"),
    }
}

fn resource_definitions() -> Value {
    json!([
        {
            "uri": "ctxbridge://rules",
            "name": "Canonical Project Rules",
            "description": "Canonical .ctxbridge/rules.md synced across AI coding assistants.",
            "mimeType": "text/markdown"
        },
        {
            "uri": "ctxbridge://status",
            "name": "ctxbridge Store Status",
            "description": "Project and global memory counts, kind breakdown, and adapter sync state.",
            "mimeType": "application/json"
        },
        {
            "uri": "ctxbridge://capsule/latest",
            "name": "Latest Handoff Capsule",
            "description": "Most recently generated token-budgeted Handoff Capsule for this project.",
            "mimeType": "text/plain"
        }
    ])
}

fn read_resource(paths: &BridgePaths, uri: &str) -> Result<Value> {
    match uri {
        "ctxbridge://rules" => {
            let text = if paths.project_rules.exists() {
                fs::read_to_string(&paths.project_rules)?
            } else {
                String::from("# No canonical rules found (.ctxbridge/rules.md not created yet).")
            };
            Ok(json!({
                "uri": uri,
                "mimeType": "text/markdown",
                "text": text
            }))
        }
        "ctxbridge://status" => {
            let report = collect_status(paths)?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&report)?
            }))
        }
        "ctxbridge://capsule/latest" => {
            let capsules = list_recent_capsules(paths, 1)?;
            let text = capsules
                .into_iter()
                .next()
                .map(|c| c.rendered_output)
                .unwrap_or_else(|| "# No Handoff Capsules generated yet.".to_string());
            Ok(json!({
                "uri": uri,
                "mimeType": "text/plain",
                "text": text
            }))
        }
        other => anyhow::bail!("unknown resource uri: {other}"),
    }
}
