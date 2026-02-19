use anyhow::{Context, Result};
use console::style;
use dialoguer::{Input, Select};
use dinoe_core::config::Config;
use std::path::Path;

pub const DEFAULT_SOUL: &str = r#"# Soul

I am dinoe 🦖, a fast, ultra-lightweight, and memory-safe AI assistant.

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

const BANNER: &str = r"
    -------------------------------------

    ██████╗ ██╗███╗   ██╗ ██████╗ ███████╗
    ██╔══██╗██║████╗  ██║██╔═══██╗██╔════╝
    ██║  ██║██║██╔██╗ ██║██║   ██║█████╗  
    ██║  ██║██║██║╚██╗██║██║   ██║██╔══╝  
    ██████╔╝██║██║ ╚████║╚██████╔╝███████╗
    ╚═════╝ ╚═╝╚═╝  ╚═══╝ ╚═════╝ ╚══════╝

    Fast. Ultra-Lightweight. Memory Safe. 100% Rust.

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

fn create_soul_file(workspace: &Path) -> Result<()> {
    std::fs::create_dir_all(workspace)?;
    let soul_path = workspace.join("SOUL.md");
    if !soul_path.exists() {
        std::fs::write(&soul_path, DEFAULT_SOUL)?;
    }
    Ok(())
}

pub fn ensure_soul_file(workspace: &Path) -> Result<()> {
    create_soul_file(workspace)
}

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

    print_step(1, 4, "API Key Setup");
    let api_key = setup_api_key()?;

    print_step(2, 3, "Model Selection");
    let model = setup_model()?;

    let config = Config {
        api_key,
        model,
        ..Default::default()
    };

    print_step(3, 3, "Workspace Setup");
    if let Err(e) = create_soul_file(&config.workspace_dir) {
        eprintln!(
            "  {} Warning: Could not create SOUL.md: {}",
            style("!").yellow(),
            e
        );
    } else {
        println!(
            "  {} SOUL.md created at {}",
            style("✓").green(),
            style(config.workspace_dir.join("SOUL.md").display()).cyan()
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
