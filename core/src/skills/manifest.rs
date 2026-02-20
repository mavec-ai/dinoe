use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct FrontMatter {
    name: String,
    description: String,
    #[serde(default = "default_version")]
    version: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub location: Option<PathBuf>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

pub fn load_skill(skill_dir: &Path) -> Result<Skill> {
    let md_path = skill_dir.join("SKILL.md");

    if md_path.exists() {
        load_skill_md(&md_path)
    } else {
        anyhow::bail!("No SKILL.md found in {}", skill_dir.display());
    }
}

fn load_skill_md(path: &Path) -> Result<Skill> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let lines: Vec<&str> = content.lines().collect();

    if lines.len() >= 3 && lines[0].trim() == "---" {
        let closing_index = lines[1..].iter().position(|l| l.trim() == "---");

        if let Some(pos) = closing_index {
            let frontmatter_str = lines[1..=pos].join("\n");

            if let Ok(frontmatter) = serde_yaml::from_str::<FrontMatter>(&frontmatter_str) {
                return Ok(Skill {
                    name: frontmatter.name,
                    description: frontmatter.description,
                    version: frontmatter.version,
                    author: frontmatter.author,
                    tags: frontmatter.tags,
                    location: Some(path.to_path_buf()),
                });
            }
        }
    }

    let first_line = content.lines().next().unwrap_or("");
    let name = first_line.trim_start_matches('#').trim().to_string();

    let description = content
        .lines()
        .find(|l| !(l.starts_with('#') || l.trim().is_empty()))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "No description".to_string());

    Ok(Skill {
        name: if name.is_empty() {
            "unnamed".to_string()
        } else {
            name
        },
        description,
        version: default_version(),
        author: None,
        tags: vec![],
        location: Some(path.to_path_buf()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_md_skill() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        fs::write(
            skill_dir.join("SKILL.md"),
            "# Test Skill\nThis is a test description.\n",
        )
        .unwrap();

        let skill = load_skill(&skill_dir).unwrap();
        assert_eq!(skill.name, "Test Skill");
        assert_eq!(skill.description, "This is a test description.");
        assert_eq!(skill.version, "0.1.0");
    }

    #[test]
    fn no_skill_file() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("empty");
        fs::create_dir_all(&skill_dir).unwrap();

        assert!(load_skill(&skill_dir).is_err());
    }
}
