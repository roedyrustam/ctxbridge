use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "ctxbridge",
    version,
    about = "Local-first cross-agent memory & context bridge (CLI + MCP server)"
)]
pub struct Cli {
    /// Override project root directory (defaults to nearest .ctxbridge or .git ancestor)
    #[arg(long, global = true)]
    pub project: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Initialize .ctxbridge/ in the current repository
    Init,

    /// Save a decision, rule, fact, lesson, or todo
    Save {
        /// Memory content text
        content: String,

        /// Memory kind: decision | rule | fact | lesson | todo
        #[arg(short, long, default_value = "decision")]
        kind: String,

        /// Optional short title
        #[arg(long)]
        title: Option<String>,

        /// Comma-separated tags
        #[arg(short, long)]
        tags: Option<String>,

        /// Priority 1 (low) to 5 (pinned/critical)
        #[arg(short, long, default_value_t = 3)]
        priority: u8,

        /// Store in global (~/.ctxbridge/global.db) instead of project store
        #[arg(long)]
        global: bool,

        /// Source agent name (e.g. antigravity, claude, cursor, codex, gemini)
        #[arg(long, default_value = "cli")]
        agent: String,
    },

    /// Recall memories using hybrid SQLite FTS5 + BM25 + priority search
    Recall {
        /// Search query (leave empty to list top/recent memories)
        #[arg(default_value = "")]
        query: String,

        /// Filter by kind
        #[arg(short, long)]
        kind: Option<String>,

        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,

        /// Maximum number of results
        #[arg(short, long, default_value_t = 8)]
        limit: usize,

        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },

    /// Permanently delete a memory by UUID or unique prefix
    Forget {
        /// Memory ID or prefix
        id: String,

        /// Delete from global store instead of project store
        #[arg(long)]
        global: bool,
    },

    /// Sync canonical rules & pinned memories to AGENTS.md, CLAUDE.md, .cursorrules, CODEX.md, .gemini/GEMINI.md
    Sync {
        /// Only update instruction files that already exist
        #[arg(long)]
        existing_only: bool,
    },

    /// Generate a compact (<500 token) Handoff Capsule for switching AI tools
    Capsule {
        /// Current goal or task being handed off
        #[arg(short, long, default_value = "Continue active development")]
        goal: String,

        /// Target AI agent (e.g. claude, cursor, codex, gemini, antigravity)
        #[arg(long, default_value = "any")]
        to: String,

        /// Maximum token budget
        #[arg(short, long, default_value_t = 500)]
        budget: usize,

        /// Output format: yaml | markdown
        #[arg(short, long, default_value = "yaml")]
        format: String,
    },

    /// Export project memories deterministically to .ctxbridge/memories.jsonl
    Export,

    /// Import and deduplicate memories from .ctxbridge/memories.jsonl
    Import,

    /// Show project and global store statistics
    Status,

    /// Start the JSON-RPC 2.0 MCP server over stdio
    Mcp,
}
