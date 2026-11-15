use crate::models::{MemoryKind, MemoryRecord, SyncStatus, SyncTargetReport};
use crate::store::MemoryStore;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub const MARKER_START: &str = "<!-- ctxbridge:start -->";
pub const MARKER_END: &str = "<!-- ctxbridge:end -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterTarget {
    AgentsMd,
    ClaudeMd,
    CursorRules,
    CodexMd,
    GeminiMd,
}

impl AdapterTarget {
    pub fn all() -> &'static [AdapterTarget] {
        &[
            AdapterTarget::AgentsMd,
            AdapterTarget::ClaudeMd,
            AdapterTarget::CursorRules,
            AdapterTarget::CodexMd,
            AdapterTarget::GeminiMd,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            AdapterTarget::AgentsMd => "agents_md",
            AdapterTarget::ClaudeMd => "claude_md",
            AdapterTarget::CursorRules => "cursorrules",
            AdapterTarget::CodexMd => "codex_md",
            AdapterTarget::GeminiMd => "gemini_md",
        }
    }

    pub fn relative_path(&self) -> &'static str {
        match self {
            AdapterTarget::AgentsMd => "AGENTS.md",
            AdapterTarget::ClaudeMd => "CLAUDE.md",
            AdapterTarget::CursorRules => ".cursorrules",
            AdapterTarget::CodexMd => "CODEX.md",
            AdapterTarget::GeminiMd => ".gemini/GEMINI.md",
        }
    }
}

pub fn render_canonical_block(rules_md: &str, pinned_memories: &[MemoryRecord]) -> String {
    let mut lines = Vec::new();
    lines.push(MARKER_START.to_string());
    lines.push("## ctxbridge Shared Context (Managed — Do Not Edit Inside Markers)".to_string());

    let trimmed_rules = rules_md.trim();
    if !trimmed_rules.is_empty() {
        lines.push(String::new());
        lines.push("### Project Rules".to_string());
        lines.push(trimmed_rules.to_string());
    }

    let decisions_and_rules: Vec<&MemoryRecord> = pinned_memories
        .iter()
        .filter(|m| m.priority >= 4 || m.kind == MemoryKind::Rule)
        .take(12)
        .collect();

    if !decisions_and_rules.is_empty() {
        lines.push(String::new());
        lines.push("### Pinned Architectural Decisions & Lessons".to_string());
        for mem in decisions_and_rules {
            let tag_suffix = if mem.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", mem.tags.join(", "))
            };
            lines.push(format!(
                "- **[{}] {}**{}: {}",
                mem.kind, mem.title, tag_suffix, mem.content
            ));
        }
    }

    lines.push(String::new());
    lines.push(MARKER_END.to_string());
    lines.join("\n")
}

pub fn merge_managed_block(existing: &str, managed_block: &str) -> String {
    if let Some(start_idx) = existing.find(MARKER_START) {
        if let Some(rel_end) = existing[start_idx..].find(MARKER_END) {
            let end_idx = start_idx + rel_end + MARKER_END.len();
            let prefix = &existing[..start_idx];
            let suffix = &existing[end_idx..];
            return format!("{prefix}{managed_block}{suffix}");
        }
    }

    let trimmed = existing.trim_end();
    if trimmed.is_empty() {
        format!("{managed_block}\n")
    } else {
        format!("{trimmed}\n\n{managed_block}\n")
    }
}

pub fn sync_adapters(
    project_root: &Path,
    rules_path: &Path,
    store: &MemoryStore,
    existing_only: bool,
) -> Result<(Vec<SyncTargetReport>, String)> {
    sync_adapters_with_mode(project_root, rules_path, store, existing_only, false)
}

pub fn sync_adapters_with_mode(
    project_root: &Path,
    rules_path: &Path,
    store: &MemoryStore,
    existing_only: bool,
    check_only: bool,
) -> Result<(Vec<SyncTargetReport>, String)> {
    let rules_md = if rules_path.exists() {
        fs::read_to_string(rules_path)
            .with_context(|| format!("failed to read {}", rules_path.display()))?
    } else {
        String::new()
    };

    let pinned = store.list_pinned_and_recent(20)?;
    let block = render_canonical_block(&rules_md, &pinned);
    let mut hasher = Sha256::new();
    hasher.update(block.as_bytes());
    let checksum = format!("{:x}", hasher.finalize());

    let mut reports = Vec::new();
    for target in AdapterTarget::all() {
        let rel = target.relative_path();
        let full_path = project_root.join(rel);
        let existed = full_path.exists();

        if existing_only && !existed {
            reports.push(SyncTargetReport {
                target_name: target.name().to_string(),
                file_path: rel.to_string(),
                status: SyncStatus::Skipped,
            });
            continue;
        }

        let current_content = if existed {
            fs::read_to_string(&full_path)
                .with_context(|| format!("failed to read {}", full_path.display()))?
        } else {
            String::new()
        };

        let next_content = merge_managed_block(&current_content, &block);
        if existed && current_content == next_content {
            reports.push(SyncTargetReport {
                target_name: target.name().to_string(),
                file_path: rel.to_string(),
                status: SyncStatus::Unchanged,
            });
            continue;
        }

        if !check_only {
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&full_path, next_content)
                .with_context(|| format!("failed to write {}", full_path.display()))?;
        }

        reports.push(SyncTargetReport {
            target_name: target.name().to_string(),
            file_path: rel.to_string(),
            status: if existed {
                SyncStatus::Updated
            } else {
                SyncStatus::Created
            },
        });
    }

    if !check_only {
        store.record_sync_reports(&reports, &checksum)?;
    }
    Ok((reports, checksum))
}
