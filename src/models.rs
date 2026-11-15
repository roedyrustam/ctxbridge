use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    Global,
    Project,
}

impl fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryScope::Global => write!(f, "global"),
            MemoryScope::Project => write!(f, "project"),
        }
    }
}

impl FromStr for MemoryScope {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "global" => Ok(MemoryScope::Global),
            "project" => Ok(MemoryScope::Project),
            other => anyhow::bail!("invalid memory scope: {other}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Decision,
    Rule,
    Fact,
    Lesson,
    Todo,
}

impl fmt::Display for MemoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryKind::Decision => write!(f, "decision"),
            MemoryKind::Rule => write!(f, "rule"),
            MemoryKind::Fact => write!(f, "fact"),
            MemoryKind::Lesson => write!(f, "lesson"),
            MemoryKind::Todo => write!(f, "todo"),
        }
    }
}

impl FromStr for MemoryKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "decision" => Ok(MemoryKind::Decision),
            "rule" => Ok(MemoryKind::Rule),
            "fact" => Ok(MemoryKind::Fact),
            "lesson" => Ok(MemoryKind::Lesson),
            "todo" => Ok(MemoryKind::Todo),
            other => anyhow::bail!("invalid memory kind: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub content_hash: String,
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub title: String,
    pub content: String,
    pub source_agent: String,
    pub priority: u8,
    pub token_estimate: usize,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SaveMemoryInput {
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub title: Option<String>,
    pub content: String,
    pub source_agent: String,
    pub priority: u8,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RecallQuery {
    pub text: String,
    pub kind: Option<MemoryKind>,
    pub tag: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    pub memory: MemoryRecord,
    pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapsuleFormat {
    Yaml,
    Markdown,
}

impl FromStr for CapsuleFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "yaml" | "yml" => Ok(CapsuleFormat::Yaml),
            "md" | "markdown" => Ok(CapsuleFormat::Markdown),
            other => anyhow::bail!("invalid capsule format: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffCapsule {
    pub id: String,
    pub goal: String,
    pub target_agent: String,
    pub token_budget: usize,
    pub token_used: usize,
    pub rendered_output: String,
    pub included_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    Created,
    Updated,
    Unchanged,
    Skipped,
}

impl fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncStatus::Created => write!(f, "created"),
            SyncStatus::Updated => write!(f, "updated"),
            SyncStatus::Unchanged => write!(f, "unchanged"),
            SyncStatus::Skipped => write!(f, "skipped"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeFilter {
    #[default]
    All,
    Project,
    Global,
}

impl FromStr for ScopeFilter {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "all" | "" => Ok(ScopeFilter::All),
            "project" => Ok(ScopeFilter::Project),
            "global" => Ok(ScopeFilter::Global),
            other => anyhow::bail!("invalid scope filter: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTargetReport {
    pub target_name: String,
    pub file_path: String,
    pub status: SyncStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTargetState {
    pub target_name: String,
    pub file_path: String,
    pub last_checksum: String,
    pub last_synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreStats {
    pub total_memories: usize,
    pub pinned_memories: usize,
    pub total_capsules: usize,
    pub by_kind: Vec<(String, usize)>,
}

pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.chars().count().div_ceil(4)
    }
}

pub fn derive_title(content: &str) -> String {
    let first_line = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    let max_chars = 80;
    if first_line.chars().count() <= max_chars {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_chars - 3).collect();
        format!("{truncated}...")
    }
}

pub fn compute_content_hash(scope: MemoryScope, kind: MemoryKind, content: &str) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(scope.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(kind.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut cleaned: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    cleaned.sort();
    cleaned.dedup();
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_handles_empty_ascii_and_utf8() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        // Multi-byte UTF-8 counts chars, not raw bytes
        assert_eq!(estimate_tokens("🚀🔥✨🎉"), 1);
        assert_eq!(estimate_tokens("日本語テスト"), 2);
    }

    #[test]
    fn derive_title_handles_utf8_truncation_and_blank_lines() {
        assert_eq!(derive_title(""), "");
        assert_eq!(derive_title("   \n\t\n  "), "");
        assert_eq!(
            derive_title("\n  \n  First real line  \nSecond"),
            "First real line"
        );

        // Exactly 80 multi-byte characters should not truncate
        let exact_80: String = "漢".repeat(80);
        assert_eq!(derive_title(&exact_80), exact_80);

        // 85 multi-byte characters should truncate to 77 chars + "..." = 80 chars
        let over_80: String = "🚀".repeat(85);
        let title = derive_title(&over_80);
        assert_eq!(title.chars().count(), 80);
        assert!(title.ends_with("..."));
        assert_eq!(title, format!("{}...", "🚀".repeat(77)));
    }

    #[test]
    fn normalize_tags_trims_lowercases_dedups_and_drops_empty() {
        let input = vec![
            "".to_string(),
            "   ".to_string(),
            "\t\n".to_string(),
            "  Rust ".to_string(),
            "rust".to_string(),
            "CLI".to_string(),
            "cli".to_string(),
            "  Alpha  ".to_string(),
        ];
        assert_eq!(normalize_tags(input), vec!["alpha", "cli", "rust"]);
    }
}
