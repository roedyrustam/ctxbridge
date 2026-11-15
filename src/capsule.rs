use crate::models::{
    estimate_tokens, CapsuleFormat, HandoffCapsule, MemoryKind, MemoryRecord, RecallQuery,
};
use crate::store::MemoryStore;
use anyhow::Result;
use chrono::Utc;
use std::collections::HashSet;
use uuid::Uuid;

pub fn generate_capsule(
    project_store: &MemoryStore,
    global_store: Option<&MemoryStore>,
    goal: &str,
    target_agent: &str,
    token_budget: usize,
    format: CapsuleFormat,
) -> Result<HandoffCapsule> {
    let budget = token_budget.max(120);
    let now = Utc::now();

    let mut candidates: Vec<MemoryRecord> = Vec::new();
    let mut seen_ids = HashSet::new();

    // 1. Recall goal-relevant memories from project store
    if !goal.trim().is_empty() {
        let recalled = project_store.recall(&RecallQuery {
            text: goal.to_string(),
            kind: None,
            tag: None,
            limit: 12,
        })?;
        for res in recalled {
            if seen_ids.insert(res.memory.id.clone()) {
                candidates.push(res.memory);
            }
        }
    }

    // 2. Add pinned & recent project memories
    for mem in project_store.list_pinned_and_recent(16)? {
        if seen_ids.insert(mem.id.clone()) {
            candidates.push(mem);
        }
    }

    // 3. Add top global memories if available
    if let Some(global) = global_store {
        if !goal.trim().is_empty() {
            for res in global.recall(&RecallQuery {
                text: goal.to_string(),
                kind: None,
                tag: None,
                limit: 5,
            })? {
                if seen_ids.insert(res.memory.id.clone()) {
                    candidates.push(res.memory);
                }
            }
        }
        for mem in global.list_pinned_and_recent(5)? {
            if seen_ids.insert(mem.id.clone()) {
                candidates.push(mem);
            }
        }
    }

    // Sort deterministically: rules/pinned first, then priority desc, then updated_at desc
    candidates.sort_by(|a, b| {
        let a_rule = usize::from(a.kind == MemoryKind::Rule || a.priority == 5);
        let b_rule = usize::from(b.kind == MemoryKind::Rule || b.priority == 5);
        b_rule
            .cmp(&a_rule)
            .then_with(|| b.priority.cmp(&a.priority))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });

    let mut included_ids = Vec::new();
    let mut selected = Vec::new();

    let header = render_header(goal, target_agent, budget, now.to_rfc3339(), format);
    let mut used_tokens = estimate_tokens(&header) + 10;

    for mem in candidates {
        let entry = format_entry(&mem, format);
        let entry_tokens = estimate_tokens(&entry);
        if used_tokens + entry_tokens > budget {
            continue;
        }
        used_tokens += entry_tokens;
        included_ids.push(mem.id.clone());
        selected.push(entry);
    }

    let rendered_output = assemble_capsule(&header, &selected, format);
    let actual_tokens = estimate_tokens(&rendered_output);

    let capsule = HandoffCapsule {
        id: Uuid::new_v4().to_string(),
        goal: goal.to_string(),
        target_agent: target_agent.to_string(),
        token_budget: budget,
        token_used: actual_tokens,
        rendered_output,
        included_ids,
        created_at: now,
    };

    project_store.record_capsule(&capsule)?;
    Ok(capsule)
}

fn render_header(
    goal: &str,
    target_agent: &str,
    budget: usize,
    timestamp: String,
    format: CapsuleFormat,
) -> String {
    let clean_goal = if goal.trim().is_empty() {
        "Continue active development"
    } else {
        goal.trim()
    };
    match format {
        CapsuleFormat::Yaml => format!(
            "ctxbridge_capsule:\n  target_agent: \"{}\"\n  goal: \"{}\"\n  token_budget: {budget}\n  generated_at: \"{timestamp}\"\n  memories:",
            escape_yaml_double_quoted(target_agent),
            escape_yaml_double_quoted(clean_goal)
        ),
        CapsuleFormat::Markdown => format!(
            "# ctxbridge Handoff Capsule\n- **Target Agent:** `{target_agent}`\n- **Goal:** {clean_goal}\n- **Token Budget:** {budget}\n- **Generated:** `{timestamp}`\n\n## Active Context"
        ),
    }
}

fn format_entry(mem: &MemoryRecord, format: CapsuleFormat) -> String {
    let body = compact_body(&mem.content, 220);
    let non_empty_tags: Vec<&str> = mem
        .tags
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let tags = if non_empty_tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", non_empty_tags.join(","))
    };
    match format {
        CapsuleFormat::Yaml => format!(
            "    - kind: {}\n      scope: {}\n      p: {}\n      summary: \"{}{}\"",
            mem.kind,
            mem.scope,
            mem.priority,
            escape_yaml_double_quoted(&body),
            escape_yaml_double_quoted(&tags)
        ),
        CapsuleFormat::Markdown => format!(
            "- **[{}|p{}|{}]** {}{}",
            mem.kind, mem.priority, mem.scope, body, tags
        ),
    }
}

fn assemble_capsule(header: &str, entries: &[String], format: CapsuleFormat) -> String {
    if entries.is_empty() {
        match format {
            CapsuleFormat::Yaml => format!("{header} []\n"),
            CapsuleFormat::Markdown => {
                format!("{header}\n- _(no matching memories within budget)_\n")
            }
        }
    } else {
        format!("{header}\n{}\n", entries.join("\n"))
    }
}

fn compact_body(content: &str, max_chars: usize) -> String {
    let single_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        single_line
    } else if max_chars <= 3 {
        single_line.chars().take(max_chars).collect()
    } else {
        let truncated: String = single_line.chars().take(max_chars - 3).collect();
        format!("{truncated}...")
    }
}

fn escape_yaml_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => {
                let code = c as u32;
                if code <= 0xFF {
                    out.push_str(&format!("\\x{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CapsuleFormat, MemoryKind, MemoryRecord, MemoryScope};
    use chrono::Utc;

    fn sample_record(content: &str, tags: Vec<&str>) -> MemoryRecord {
        let now = Utc::now();
        MemoryRecord {
            id: "mem-1".to_string(),
            content_hash: "hash-1".to_string(),
            scope: MemoryScope::Project,
            kind: MemoryKind::Decision,
            title: "Sample".to_string(),
            content: content.to_string(),
            source_agent: "tester".to_string(),
            priority: 4,
            token_estimate: estimate_tokens(content),
            tags: tags.into_iter().map(String::from).collect(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn render_header_escapes_yaml_special_chars() {
        let goal = "Fix \"quoted\" path C:\\src\\main.rs\nand\ttabs";
        let target = "agent\\1\"alpha\"\nline2";
        let header = render_header(
            goal,
            target,
            500,
            "2026-01-01T00:00:00Z".to_string(),
            CapsuleFormat::Yaml,
        );

        assert!(header.contains(r#"target_agent: "agent\\1\"alpha\"\nline2""#));
        assert!(header.contains(r#"goal: "Fix \"quoted\" path C:\\src\\main.rs\nand\ttabs""#));
        // Header lines should not have unescaped newlines inside quoted values
        assert_eq!(header.lines().count(), 6);
    }

    #[test]
    fn format_entry_escapes_yaml_body_and_tags() {
        let rec = sample_record(
            r#"Use "strict" mode in C:\app\config"#,
            vec!["tag\"1", "path\\a", "line\nbreak", "  ", "tab\there"],
        );
        let entry = format_entry(&rec, CapsuleFormat::Yaml);

        assert!(entry.contains(
            r#"summary: "Use \"strict\" mode in C:\\app\\config [tag\"1,path\\a,line\nbreak,tab\there]""#
        ));
        assert_eq!(entry.lines().count(), 4);
    }

    #[test]
    fn compact_body_handles_multibyte_utf8_and_small_limits() {
        // 10 multi-byte emoji / CJK characters
        let text = "🚀🔥✨日本語テスト漢字";
        let compacted = compact_body(text, 8);
        assert_eq!(compacted.chars().count(), 8);
        assert_eq!(compacted, "🚀🔥✨日本...");

        // Small max_chars values must never underflow or panic
        assert_eq!(compact_body(text, 3), "🚀🔥✨");
        assert_eq!(compact_body(text, 2), "🚀🔥");
        assert_eq!(compact_body(text, 0), "");

        // Whitespace normalization with UTF-8
        assert_eq!(compact_body("  αβγ \n\t δεζ  ", 20), "αβγ δεζ");
    }
}
