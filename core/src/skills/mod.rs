pub mod manifest;
pub mod registry;

pub use manifest::{Skill, load_skill};
pub use registry::SkillRegistry;

use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn skills_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("skills")
}

pub fn init_skills_dir(workspace_dir: &Path) -> Result<()> {
    let dir = skills_dir(workspace_dir);
    std::fs::create_dir_all(&dir)?;
    Ok(())
}
