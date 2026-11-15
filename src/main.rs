use anyhow::{Context, Result};
use clap::Parser;
use ctxbridge::adapter::sync_adapters;
use ctxbridge::capsule::generate_capsule;
use ctxbridge::cli::{Cli, Commands};
use ctxbridge::mcp::run_stdio_server;
use ctxbridge::models::{CapsuleFormat, MemoryKind, MemoryScope, RecallQuery, SaveMemoryInput};
use ctxbridge::paths::{find_project_root, BridgePaths};
use ctxbridge::store::MemoryStore;
use std::fs;

const DEFAULT_RULES_TEMPLATE: &str = r#"# Canonical Project Rules (ctxbridge)

- Keep changes focused, well-tested, and free of placeholder stubs.
- Record major architectural decisions with `ctxbridge save --kind decision`.
"#;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let project_root = cli
        .project
        .unwrap_or_else(|| find_project_root(&cwd));
    let paths = BridgePaths::resolve(&project_root)?;

    match cli.command {
        Commands::Init => {
            paths.ensure_project_dir()?;
            MemoryStore::open(&paths.project_db)?;
            if !paths.project_rules.exists() {
                fs::write(&paths.project_rules, DEFAULT_RULES_TEMPLATE)?;
            }
            println!(
                "Initialized ctxbridge at {}",
                paths.project_dir.display()
            );
            println!("Canonical rules: {}", paths.project_rules.display());
            println!("Run `ctxbridge sync` to update AGENTS.md, CLAUDE.md, .cursorrules, CODEX.md, and .gemini/GEMINI.md.");
        }
        Commands::Save {
            content,
            kind,
            title,
            tags,
            priority,
            global,
            agent,
        } => {
            let scope = if global {
                MemoryScope::Global
            } else {
                MemoryScope::Project
            };
            let parsed_kind: MemoryKind = kind.parse()?;
            let parsed_tags: Vec<String> = tags
                .map(|t| t.split(',').map(ToString::to_string).collect())
                .unwrap_or_default();

            let db_path = match scope {
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
            let (record, inserted) = store.save_memory(SaveMemoryInput {
                scope,
                kind: parsed_kind,
                title,
                content,
                source_agent: agent,
                priority,
                tags: parsed_tags,
            })?;
            let action = if inserted { "Saved" } else { "Updated (deduplicated)" };
            println!(
                "{action} [{}|{}|p{}] {} ({})",
                record.scope,
                record.kind,
                record.priority,
                record.title,
                &record.id[..8]
            );
        }
        Commands::Recall {
            query,
            kind,
            tag,
            limit,
            json,
        } => {
            let parsed_kind = kind.as_deref().map(str::parse).transpose()?;
            let q = RecallQuery {
                text: query,
                kind: parsed_kind,
                tag,
                limit,
            };

            let mut results = Vec::new();
            if paths.project_db.exists() {
                let p_store = MemoryStore::open(&paths.project_db)?;
                results.extend(p_store.recall(&q)?);
            }
            if paths.global_db.exists() {
                let g_store = MemoryStore::open(&paths.global_db)?;
                results.extend(g_store.recall(&q)?);
            }
            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            results.truncate(limit);

            if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else if results.is_empty() {
                println!("No matching memories found.");
            } else {
                for r in results {
                    let m = r.memory;
                    let tags = if m.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", m.tags.join(", "))
                    };
                    println!(
                        "[{}|{}|p{}|{}] {}{}\n  {}",
                        &m.id[..8],
                        m.scope,
                        m.priority,
                        m.kind,
                        m.title,
                        tags,
                        m.content
                    );
                }
            }
        }
        Commands::Forget { id, global } => {
            let db_path = if global {
                &paths.global_db
            } else {
                &paths.project_db
            };
            if !db_path.exists() {
                println!("Store not initialized; 0 memories deleted.");
                return Ok(());
            }
            let store = MemoryStore::open(db_path)?;
            let deleted = store.forget(&id)?;
            println!("Deleted {deleted} memory record(s).");
        }
        Commands::Sync { existing_only } => {
            paths.ensure_project_dir()?;
            let store = MemoryStore::open(&paths.project_db)?;
            let (reports, checksum) = sync_adapters(
                &paths.project_root,
                &paths.project_rules,
                &store,
                existing_only,
            )?;
            println!("Synced adapters (checksum {}):", &checksum[..12]);
            for r in reports {
                println!("  - {:<18} [{}]", r.file_path, r.status);
            }
        }
        Commands::Capsule {
            goal,
            to,
            budget,
            format,
        } => {
            let fmt: CapsuleFormat = format.parse()?;
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
                &goal,
                &to,
                budget,
                fmt,
            )?;
            print!("{}", capsule.rendered_output);
            eprintln!(
                "--- capsule id={} tokens={}/{} memories={} ---",
                &capsule.id[..8],
                capsule.token_used,
                capsule.token_budget,
                capsule.included_ids.len()
            );
        }
        Commands::Export => {
            paths.ensure_project_dir()?;
            let store = MemoryStore::open(&paths.project_db)?;
            let count = store.export_jsonl(&paths.project_jsonl)?;
            println!(
                "Exported {count} memories to {}",
                paths.project_jsonl.display()
            );
        }
        Commands::Import => {
            paths.ensure_project_dir()?;
            let store = MemoryStore::open(&paths.project_db)?;
            let (imported, skipped) = store.import_jsonl(&paths.project_jsonl)?;
            println!(
                "Imported {imported} new memories ({skipped} duplicates merged/skipped)."
            );
        }
        Commands::Status => {
            println!("Project root: {}", paths.project_root.display());
            if paths.project_db.exists() {
                let p_stats = MemoryStore::open(&paths.project_db)?.stats()?;
                println!(
                    "Project store: {} memories ({} pinned), {} capsules",
                    p_stats.total_memories, p_stats.pinned_memories, p_stats.total_capsules
                );
            } else {
                println!("Project store: not initialized (run `ctxbridge init`)");
            }
            if paths.global_db.exists() {
                let g_stats = MemoryStore::open(&paths.global_db)?.stats()?;
                println!(
                    "Global store:  {} memories ({} pinned)",
                    g_stats.total_memories, g_stats.pinned_memories
                );
            } else {
                println!("Global store:  empty");
            }
        }
        Commands::Mcp => {
            run_stdio_server(&paths)?;
        }
    }

    Ok(())
}
