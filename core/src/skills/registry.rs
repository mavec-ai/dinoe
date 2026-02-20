use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::{Skill, load_skill, skills_dir};

#[derive(Clone)]
pub struct SkillRegistry {
    skills: Arc<Mutex<HashMap<String, Skill>>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn load_from_workspace(workspace_dir: &Path) -> Result<Self> {
        let mut registry = Self::new();
        registry.load_skills(workspace_dir)?;
        Ok(registry)
    }

    pub fn load_skills(&mut self, workspace_dir: &Path) -> Result<()> {
        let skills_path = skills_dir(workspace_dir);

        if !skills_path.exists() {
            tracing::debug!("Skills directory does not exist: {}", skills_path.display());
            return Ok(());
        }

        let entries = fs::read_dir(&skills_path).with_context(|| {
            format!("Failed to read skills directory: {}", skills_path.display())
        })?;

        let mut loaded = 0;
        let mut skipped = 0;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            if is_unsafe_skill_name(name) {
                tracing::warn!("Skipping unsafe skill name: {}", name);
                skipped += 1;
                continue;
            }

            match load_skill(&path) {
                Ok(skill) => {
                    self.skills
                        .lock()
                        .unwrap()
                        .insert(skill.name.clone(), skill);
                    loaded += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to load skill '{}': {}", name, e);
                    skipped += 1;
                }
            }
        }

        tracing::info!(
            loaded,
            skipped,
            path = %skills_path.display(),
            "Skills loaded"
        );

        Ok(())
    }

    pub fn list(&self) -> Vec<Skill> {
        self.skills.lock().unwrap().values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Skill> {
        self.skills.lock().unwrap().get(name).cloned()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.skills.lock().unwrap().contains_key(name)
    }

    pub fn count(&self) -> usize {
        self.skills.lock().unwrap().len()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn is_unsafe_skill_name(name: &str) -> bool {
    name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn registry_loads_skills() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill1_dir = skills_dir.join("skill1");
        fs::create_dir_all(&skill1_dir).unwrap();
        fs::write(skill1_dir.join("SKILL.md"), "# skill1\nFirst skill\n").unwrap();

        let skill2_dir = skills_dir.join("skill2");
        fs::create_dir_all(&skill2_dir).unwrap();
        fs::write(skill2_dir.join("SKILL.md"), "# skill2\nSecond skill\n").unwrap();

        let registry = SkillRegistry::load_from_workspace(tmp.path()).unwrap();
        assert_eq!(registry.count(), 2);
        assert!(registry.contains("skill1"));
        assert!(registry.contains("skill2"));
    }

    #[test]
    fn registry_skips_invalid_skills() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let invalid_dir = skills_dir.join("invalid");
        fs::create_dir_all(&invalid_dir).unwrap();

        let registry = SkillRegistry::load_from_workspace(tmp.path()).unwrap();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn registry_skips_unsafe_names() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let unsafe_dir = skills_dir.join("..bad");
        fs::create_dir_all(&unsafe_dir).unwrap();
        fs::write(unsafe_dir.join("SKILL.md"), "# bad\nUnsafe\n").unwrap();

        let registry = SkillRegistry::load_from_workspace(tmp.path()).unwrap();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn registry_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let registry = SkillRegistry::load_from_workspace(tmp.path()).unwrap();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn registry_get_skill() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("test");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# test\nTest skill\n").unwrap();

        let registry = SkillRegistry::load_from_workspace(tmp.path()).unwrap();
        let skill = registry.get("test");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().version, "0.1.0");
    }
}
