use anyhow::{Context, Result};
use console::style;
use dialoguer::{Input, Select};
use dinoe_core::config::Config;
use std::path::Path;

const BANNER: &str = r"
    -------------------------------------

    ██████╗ ██╗███╗   ██╗ ██████╗ ███████╗
    ██╔══██╗██║████╗  ██║██╔═══██╗██╔════╝
    ██║  ██║██║██╔██╗ ██║██║   ██║█████╗  
    ██║  ██║██║██║╚██╗██║██║   ██║██╔══╝  
    ██████╔╝██║██║ ╚████║╚██████╔╝███████╗
    ╚═════╝ ╚═╝╚═╝  ╚═══╝ ╚═════╝ ╚══════╝

    -------------------------------------
";

fn print_step(step: usize, total: usize, title: &str) {
    println!();
    println!(
        "{}",
        style(format!("[{}/{}] {}", step, total, title))
            .cyan()
            .bold()
    );
    println!();
}

fn ensure_file(path: &Path, content: &str) -> Result<bool> {
    if !path.exists() {
        std::fs::write(path, content)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn create_bootstrap_files(workspace: &Path) -> Result<()> {
    std::fs::create_dir_all(workspace)?;

    ensure_file(&workspace.join("SOUL.md"), DEFAULT_SOUL)?;
    ensure_file(&workspace.join("TOOLS.md"), DEFAULT_TOOLS)?;
    ensure_file(&workspace.join("USER.md"), DEFAULT_USER)?;

    Ok(())
}

fn init_skills_dir(workspace: &Path) -> Result<()> {
    dinoe_core::skills::init_skills_dir(workspace)?;
    Ok(())
}

pub fn ensure_bootstrap_files(workspace: &Path) -> Result<()> {
    create_bootstrap_files(workspace)
}

pub const DEFAULT_SOUL: &str = r#"# SOUL.md — Who You Are

You are dinoe 🦖, a Fast, ultra-lightweight AI Assistant.

## Core Identity

- **Name**: dinoe
- **Purpose**: Help users accomplish tasks efficiently with code, files, and knowledge
- **Built with**: Rust (100% memory safe, fast, and reliable)

## Personality

- Helpful and friendly
- Concise and to the point
- Technical and precise
- Curious and eager to learn
- Practical and action-oriented

## Values

- **Accuracy over speed**: Get it right the first time
- **User privacy and safety**: Never expose sensitive information
- **Transparency in actions**: Always explain what you're doing before using tools
- **Efficiency**: Use the most appropriate tool for the task
- **Code quality**: Write clean, maintainable, and idiomatic code

## Communication Style

- Be clear and direct
- Explain reasoning when helpful, but don't over-explain obvious things
- Ask clarifying questions when the request is ambiguous
- Use code blocks for code and file paths
- Provide concrete examples when helpful
- Admit when you don't know something

## Problem-Solving Approach

1. **Understand**: Clarify the user's intent if needed
2. **Plan**: Briefly outline the approach before taking action
3. **Execute**: Use tools efficiently to accomplish the task
4. **Verify**: Check that the solution works as expected
5. **Learn**: Remember important information in memory

## When Using Tools

- **Before using tools**: Briefly explain what you're about to do
- **While using tools**: Provide updates on progress for long-running operations
- **After using tools**: Summarize results and any important findings

## Tool Preferences

- **File operations**: Use file tools to read, write, and edit files
- **Shell commands**: Use shell for system operations, git commands, etc.
- **Memory**: Store important information in MEMORY.md for long-term recall

## Code Conventions

When writing code:
- Follow existing code style and conventions in the project
- Use idiomatic patterns for the language
- Add comments only when the code is unclear
- Prefer simple solutions over complex ones
- Consider performance and memory efficiency (especially in Rust)

## Memory Management

Store information in memory when:
- User preferences are learned
- Important project context is established
- Repeated questions or topics come up
- Critical decisions are made

Don't store:
- Temporary or one-off information
- Transient state
- Information already in files

## Handling Errors

When encountering errors:
- Read the error message carefully
- Identify the root cause
- Propose a specific solution
- If unsure, explain what you understand and ask for guidance
- Never proceed without understanding the error

## Continuous Improvement

- Learn from user feedback
- Adapt to user preferences over time
- Improve explanations based on user responses
- Remember successful patterns and approaches

---

*This file defines dinoe's core personality and behavior patterns. Edit to customize the agent's identity.*"#;

pub const DEFAULT_TOOLS: &str = r#"# TOOLS.md — Local Notes

Skills define HOW tools work. This file is for YOUR specifics — the stuff that's unique to your setup.

## What Goes Here

Things like:
- SSH hosts and aliases
- Device nicknames
- Preferred voices for TTS
- Anything environment-specific

## Built-in Tools

- **shell** — Execute terminal commands
- **file_read** — Read file contents
- **file_write** — Write or create files
- **memory_read** — Retrieve information from memory
- **memory_write** — Store information in memory

## Tips

- Keep this file focused on YOUR environment
- Don't duplicate tool documentation here (that's in the code)
- Update as your environment changes

---

*Edit this file to add your local tool preferences and environment-specific notes.*"#;

pub const DEFAULT_USER: &str = r#"# USER.md — Who You're Helping

This file contains information about the user you're helping. Customize it to provide context about their preferences, goals, and working style.

## User Profile

- **Name**: [User's name]
- **Role**: [Developer / Student / Creator / etc.]
- **Primary language**: [English / Indonesian / etc.]

## Communication Preferences

- Preferred level of detail: [High-level / Detailed / Just-the-facts]
- Preferred response style: [Concise / Conversational / Formal]
- Do they like examples: [Yes / No]
- Do they like explanations: [Yes / No]

## Working Style

- Do they prefer: [Step-by-step guidance / Autonomy / Mixed]
- Decision-making: [They decide / Ask first / Suggest options]
- Error tolerance: [Low / Medium / High]

## Common Topics

List topics they frequently ask about:
- [Topic 1]
- [Topic 2]
- [Topic 3]

## Things to Remember

- [Important preference or habit]
- [Recurring project or goal]
- [Anything else that helps you help them better]

---

*Edit this file to provide context about the user you're assisting.*"#;

fn setup_api_key() -> Result<String> {
    let api_key: String = Input::new()
        .with_prompt("Enter your OpenAI API key")
        .interact_text()
        .context("Failed to read API key")?;

    if api_key.is_empty() {
        return Err(anyhow::anyhow!("API key cannot be empty"));
    }

    Ok(api_key)
}

fn setup_model() -> Result<String> {
    let models = vec!["gpt-5", "gpt-5-mini", "gpt-4o", "gpt-4o-mini"];

    let selection = Select::new()
        .with_prompt("Select your model")
        .items(&models)
        .default(0)
        .interact()
        .context("Failed to select model")?;

    Ok(models[selection].to_string())
}

pub fn run_onboard() -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());

    println!("  {}", style("Welcome to Dinoe!").white().bold());
    println!(
        "  {}",
        style("This wizard will configure your agent in under 30 seconds.").dim()
    );
    println!();

    print_step(1, 3, "API Key Setup");
    let api_key = setup_api_key()?;

    print_step(2, 3, "Model Selection");
    let model = setup_model()?;

    let config = Config {
        api_key,
        model,
        ..Default::default()
    };

    print_step(3, 3, "Workspace Setup");
    if let Err(e) = create_bootstrap_files(&config.workspace_dir) {
        eprintln!(
            "  {} Warning: Could not create bootstrap files: {}",
            style("!").yellow(),
            e
        );
    } else {
        println!(
            "  {} Bootstrap files created at {}",
            style("✓").green(),
            style(config.workspace_dir.display()).cyan()
        );
        println!("  {} - SOUL.md", style("  ").dim());
        println!("  {} - TOOLS.md", style("  ").dim());
        println!("  {} - USER.md", style("  ").dim());
    }

    if let Err(e) = init_skills_dir(&config.workspace_dir) {
        eprintln!(
            "  {} Warning: Could not create skills directory: {}",
            style("!").yellow(),
            e
        );
    } else {
        println!(
            "  {} Skills directory ready at {}",
            style("✓").green(),
            style(config.workspace_dir.join("skills").display()).cyan()
        );
    }

    println!();
    println!("  {} Configuration complete!", style("✓").green().bold());
    println!(
        "  {} Config saved to {}",
        style("→").green(),
        style(dinoe_core::config::get_config_path().display()).cyan()
    );
    println!();
    println!(
        "  {} You can now run: {}",
        style("→").green(),
        style("dinoe chat").cyan().bold()
    );
    println!();

    Ok(config)
}
