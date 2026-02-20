use anyhow::Result;
use console::style;
use dinoe_core::skills;
use std::path::Path;

pub fn handle_command(command: SkillsCommands, workspace_dir: &Path) -> Result<()> {
    match command {
        SkillsCommands::List => list_skills(workspace_dir),
        SkillsCommands::Install { source } => install_skill(source, workspace_dir),
        SkillsCommands::Remove { name } => remove_skill(name, workspace_dir),
    }
}

fn list_skills(workspace_dir: &Path) -> Result<()> {
    let skills_dir = skills::skills_dir(workspace_dir);

    if !skills_dir.exists() {
        println!("{} No skills directory found", style("!").yellow());
        println!();
        print_create_skill_help(&skills_dir);
        return Ok(());
    }

    let entries = std::fs::read_dir(&skills_dir)?;
    let skills: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();

    if skills.is_empty() {
        println!("{} No skills installed", style("!").yellow());
        println!();
        println!("Install a skill:");
        println!("  dinoe skills install <github-url>");
        println!();
        println!("Or create one:");
        print_create_skill_help(&skills_dir);
        return Ok(());
    }

    println!(
        "{} Installed skills ({})",
        style("✓").green().bold(),
        skills.len()
    );
    println!();

    for entry in skills {
        let skill_dir = entry.path();
        if let Ok(skill) = skills::load_skill(&skill_dir) {
            println!(
                "  {} {} — {}",
                style(&skill.name).white().bold(),
                style(format!("v{}", skill.version)).dim(),
                skill.description
            );

            if !skill.tags.is_empty() {
                println!("    Tags:  {}", skill.tags.join(", "));
            }

            if let Some(author) = &skill.author {
                println!("    Author: {}", author);
            }

            println!();
        }
    }

    Ok(())
}

fn install_skill(source: String, workspace_dir: &Path) -> Result<()> {
    println!("{} Installing from: {}", style("→").cyan(), source);

    let skills_path = skills::skills_dir(workspace_dir);
    std::fs::create_dir_all(&skills_path)?;

    if source.starts_with("https://") || source.starts_with("http://") {
        let output = std::process::Command::new("git")
            .args(["clone", "--depth", "1", &source])
            .current_dir(&skills_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Git clone failed: {}", stderr);
        }

        println!(
            "{} Skill installed successfully!",
            style("✓").green().bold()
        );
    } else {
        let src = std::path::PathBuf::from(&source);
        if !src.exists() {
            anyhow::bail!("Source path does not exist: {}", source);
        }

        let name = src
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("skill");
        let dest = skills_path.join(name);

        copy_dir_recursive(&src, &dest)?;
        println!(
            "{} Skill copied: {}",
            style("✓").green().bold(),
            dest.display()
        );
    }

    Ok(())
}

fn remove_skill(name: String, workspace_dir: &Path) -> Result<()> {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        anyhow::bail!("Invalid skill name: {}", name);
    }

    let skill_path = skills::skills_dir(workspace_dir).join(&name);

    let canonical_skills = skills::skills_dir(workspace_dir)
        .canonicalize()
        .unwrap_or_else(|_| skills::skills_dir(workspace_dir));

    if let Ok(canonical_skill) = skill_path.canonicalize()
        && !canonical_skill.starts_with(&canonical_skills)
    {
        anyhow::bail!("Skill path escapes skills directory: {}", name);
    }

    if !skill_path.exists() {
        anyhow::bail!("Skill not found: {}", name);
    }

    std::fs::remove_dir_all(&skill_path)?;
    println!("{} Skill '{}' removed", style("✓").green().bold(), name);

    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

fn print_create_skill_help(skills_dir: &std::path::Path) {
    println!("  mkdir -p {}/my-skill", skills_dir.display());
    println!(
        "  echo '# My Skill' > {}/my-skill/SKILL.md",
        skills_dir.display()
    );
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum SkillsCommands {
    List,
    Install { source: String },
    Remove { name: String },
}
