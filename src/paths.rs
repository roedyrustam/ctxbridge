use anyhow::{Context, Result};
use directories::BaseDirs;
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_DIR_NAME: &str = ".ctxbridge";
pub const PROJECT_DB_NAME: &str = "project.db";
pub const GLOBAL_DB_NAME: &str = "global.db";
pub const JSONL_EXPORT_NAME: &str = "memories.jsonl";
pub const RULES_FILE_NAME: &str = "rules.md";

#[derive(Debug, Clone)]
pub struct BridgePaths {
    pub project_root: PathBuf,
    pub project_dir: PathBuf,
    pub project_db: PathBuf,
    pub project_jsonl: PathBuf,
    pub project_rules: PathBuf,
    pub global_dir: PathBuf,
    pub global_db: PathBuf,
}

impl BridgePaths {
    pub fn resolve(project_root: &Path) -> Result<Self> {
        let project_dir = project_root.join(PROJECT_DIR_NAME);
        let global_dir = default_global_dir()?;
        Ok(Self {
            project_root: project_root.to_path_buf(),
            project_db: project_dir.join(PROJECT_DB_NAME),
            project_jsonl: project_dir.join(JSONL_EXPORT_NAME),
            project_rules: project_dir.join(RULES_FILE_NAME),
            project_dir,
            global_db: global_dir.join(GLOBAL_DB_NAME),
            global_dir,
        })
    }

    pub fn ensure_project_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.project_dir)
            .with_context(|| format!("failed to create {}", self.project_dir.display()))
    }

    pub fn ensure_global_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.global_dir)
            .with_context(|| format!("failed to create {}", self.global_dir.display()))
    }
}

pub fn default_global_dir() -> Result<PathBuf> {
    if let Ok(custom) = std::env::var("CTXBRIDGE_HOME") {
        if !custom.trim().is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }
    let base = BaseDirs::new().context("unable to determine user home directory")?;
    Ok(base.home_dir().join(PROJECT_DIR_NAME))
}

pub fn find_project_root(start: &Path) -> PathBuf {
    let mut current = start.to_path_buf();
    loop {
        if current.join(PROJECT_DIR_NAME).is_dir() || current.join(".git").exists() {
            return current;
        }
        if !current.pop() {
            return start.to_path_buf();
        }
    }
}
