use anyhow::{Context, Result};
use clap::Parser;
use ctxbridge::adapter::sync_adapters_with_mode;
use ctxbridge::capsule::generate_capsule;
use ctxbridge::cli::{Cli, Commands};
use ctxbridge::mcp::run_stdio_server;
use ctxbridge::models::{
    CapsuleFormat, MemoryKind, MemoryScope, RecallQuery, SaveMemoryInput, ScopeFilter, SyncStatus,
};
use ctxbridge::paths::{find_project_root, BridgePaths};
use ctxbridge::service::{
    collect_status, forget_from_scope, list_recent_capsules, promote_memory, recall_across_stores,
    run_doctor, save_memory_to_scope,
};
use ctxbridge::store::MemoryStore;
use std::fs;

const DEFAULT_RULES_TEMPLATE: &str = r#"# Canonical Project Rules (ctxbridge)

- Keep changes focused, well-tested, and free of placeholder stubs.
- Record major architectural decisions with `ctxbridge save --kind decision`.
"#;

const DEFAULT_GITIGNORE_TEMPLATE: &str = "project.db*\n";

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let project_root = cli.project.unwrap_or_else(|| find_project_root(&cwd));
    let paths = BridgePaths::resolve(&project_root)?;

    match cli.command {
        Commands::Init => {
            paths.ensure_project_dir()?;
            MemoryStore::open(&paths.project_db)?;
            if !paths.project_rules.exists() {
                fs::write(&paths.project_rules, DEFAULT_RULES_TEMPLATE)?;
            }
            let gitignore_path = paths.project_dir.join(".gitignore");
            if !gitignore_path.exists() {
                fs::write(&gitignore_path, DEFAULT_GITIGNORE_TEMPLATE)?;
            }
            println!("Initialized ctxbridge at {}", paths.project_dir.display());
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

            let (record, inserted) = save_memory_to_scope(
                &paths,
                SaveMemoryInput {
                    scope,
                    kind: parsed_kind,
                    title,
                    content,
                    source_agent: agent,
                    priority,
                    tags: parsed_tags,
                },
            )?;
            let action = if inserted {
                "Saved"
            } else {
                "Updated (deduplicated)"
            };
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
            scope,
            limit,
            json,
        } => {
            let parsed_kind = kind.as_deref().map(str::parse).transpose()?;
            let scope_filter: ScopeFilter = scope.parse()?;
            let q = RecallQuery {
                text: query,
                kind: parsed_kind,
                tag,
                limit,
            };

            let results = recall_across_stores(&paths, &q, scope_filter)?;

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
            let scope = if global {
                MemoryScope::Global
            } else {
                MemoryScope::Project
            };
            let deleted = forget_from_scope(&paths, &id, scope)?;
            println!("Deleted {deleted} memory record(s).");
        }
        Commands::Sync {
            existing_only,
            check,
        } => {
            if check && !paths.project_db.exists() {
                anyhow::bail!("project store not initialized (run `ctxbridge init`)");
            }
            if !check {
                paths.ensure_project_dir()?;
            }
            let store = if paths.project_db.exists() {
                MemoryStore::open(&paths.project_db)?
            } else {
                MemoryStore::open_in_memory()?
            };
            let (reports, checksum) = sync_adapters_with_mode(
                &paths.project_root,
                &paths.project_rules,
                &store,
                existing_only,
                check,
            )?;
            let label = if check {
                "Sync check"
            } else {
                "Synced adapters"
            };
            println!("{label} (checksum {}):", &checksum[..12]);
            let mut out_of_sync = false;
            for r in reports {
                if matches!(r.status, SyncStatus::Created | SyncStatus::Updated) {
                    out_of_sync = true;
                }
                println!("  - {:<18} [{}]", r.file_path, r.status);
            }
            if check && out_of_sync {
                anyhow::bail!("adapter instruction files are out of sync; run `ctxbridge sync`");
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
            println!("Imported {imported} new memories ({skipped} duplicates merged/skipped).");
        }
        Commands::Status { json } => {
            let report = collect_status(&paths)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
                return Ok(());
            }

            println!("Project root: {}", report.project_root);
            if let Some(p_stats) = &report.project_stats {
                println!(
                    "Project store: {} memories ({} pinned), {} capsules",
                    p_stats.total_memories, p_stats.pinned_memories, p_stats.total_capsules
                );
                if !p_stats.by_kind.is_empty() {
                    let breakdown: Vec<String> = p_stats
                        .by_kind
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    println!("  By kind:     {}", breakdown.join(", "));
                }
            } else {
                println!("Project store: not initialized (run `ctxbridge init`)");
            }

            if let Some(g_stats) = &report.global_stats {
                println!(
                    "Global store:  {} memories ({} pinned)",
                    g_stats.total_memories, g_stats.pinned_memories
                );
            } else {
                println!("Global store:  empty");
            }

            if !report.sync_targets.is_empty() {
                println!("Adapter sync:");
                for t in &report.sync_targets {
                    println!(
                        "  - {:<18} checksum={} synced_at={}",
                        t.file_path,
                        &t.last_checksum[..12.min(t.last_checksum.len())],
                        t.last_synced_at
                    );
                }
            }
        }
        Commands::Promote { id } => {
            let (record, inserted) = promote_memory(&paths, &id)?;
            let action = if inserted {
                "Promoted to global"
            } else {
                "Updated existing global"
            };
            println!(
                "{action} [{}|p{}] {} ({})",
                record.kind,
                record.priority,
                record.title,
                &record.id[..8]
            );
        }
        Commands::Capsules { limit, json } => {
            let capsules = list_recent_capsules(&paths, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&capsules)?);
            } else if capsules.is_empty() {
                println!("No capsules generated yet.");
            } else {
                for c in capsules {
                    println!(
                        "[{}] to={} tokens={}/{} memories={} at={}\n  goal: {}",
                        &c.id[..8],
                        c.target_agent,
                        c.token_used,
                        c.token_budget,
                        c.included_ids.len(),
                        c.created_at.to_rfc3339(),
                        c.goal
                    );
                }
            }
        }
        Commands::Doctor { json } => {
            let report = run_doctor(&paths)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "ctxbridge doctor: {}",
                    if report.healthy {
                        "HEALTHY"
                    } else {
                        "ISSUES FOUND"
                    }
                );
                println!("  - project initialized: {}", report.project_initialized);
                println!("  - project.db ok:       {}", report.project_db_ok);
                println!(
                    "  - global.db ok:        {}",
                    report
                        .global_db_ok
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "not created".into())
                );
                println!("  - .gitignore present:  {}", report.gitignore_present);
                println!("  - rules.md present:    {}", report.rules_present);
                if report.out_of_sync_targets.is_empty() {
                    println!("  - adapters in sync:    yes");
                } else {
                    println!(
                        "  - adapters out of sync: {}",
                        report.out_of_sync_targets.join(", ")
                    );
                }
            }
            if !report.healthy {
                anyhow::bail!("doctor check failed");
            }
        }
        Commands::Mcp => {
            run_stdio_server(&paths)?;
        }
    }

    Ok(())
}
