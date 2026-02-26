use anyhow::Result;
use clap::{Parser, Subcommand};
use dinoe_core::{
    agent, config,
    providers,
    tools::{ContentSearchTool, FileEditTool, FileReadTool, FileWriteTool, GitOperationsTool, GlobSearchTool, HttpRequestTool, MemoryReadTool, MemoryWriteTool, ShellTool, WebFetchTool},
};
mod onboard;
mod repl;
mod skills;
mod templates;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "dinoe")]
#[command(about = "dinoe - Fast, ultra-lightweight CLI AI agent", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Onboard,
    Chat {
        #[arg(short, long)]
        message: Option<String>,
    },
    Skills {
        #[command(subcommand)]
        skill_command: skills::SkillsCommands,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let command = cli.command.unwrap_or_else(|| {
        if !config::config_exists() {
            Commands::Onboard
        } else {
            Commands::Chat { message: None }
        }
    });

    match command {
        Commands::Onboard => {
            let onboard_config = onboard::run_onboard().map_err(|e| {
                eprintln!("❌ Onboarding failed: {}", e);
                anyhow::anyhow!("Onboarding failed: {}", e)
            })?;
            config::save_config(&onboard_config)?;
        }
        Commands::Skills { skill_command } => {
            let config = config::load_config()?;
            skills::handle_command(skill_command, &config.workspace_dir)?;
        }
        Commands::Chat { message } => {
            let config = config::load_config()?;

            let provider_box = providers::create_provider(&config)?;

            if !config.workspace_dir.exists()
                && let Err(e) = std::fs::create_dir_all(&config.workspace_dir)
            {
                eprintln!(
                    "❌ Error: Could not create workspace at {}: {}",
                    config.workspace_dir.display(),
                    e
                );
                eprintln!("Please check your permissions and try again.");
                return Err(e.into());
            }

            if let Err(e) = onboard::ensure_bootstrap_files(&config.workspace_dir) {
                eprintln!("❌ Error: Could not create bootstrap files: {}", e);
                return Err(e);
            }

            let memory = dinoe_core::memory::create_memory(&config.workspace_dir)?;
            let skill_registry =
                dinoe_core::skills::SkillRegistry::load_from_workspace(&config.workspace_dir)?;
            let skills = skill_registry.list();

            let tool_registry = Arc::new(agent::ToolRegistry::new());
            let provider_arc: Arc<dyn dinoe_core::traits::Provider> = Arc::from(provider_box);

            tool_registry.register(Box::new(FileReadTool::new(&config.workspace_dir)));
            tool_registry.register(Box::new(FileWriteTool::new(&config.workspace_dir)));
            tool_registry.register(Box::new(ShellTool::new(&config.workspace_dir)));
            tool_registry.register(Box::new(MemoryReadTool::new(memory.clone())));
            tool_registry.register(Box::new(MemoryWriteTool::new(memory.clone())));
            tool_registry.register(Box::new(WebFetchTool::new()));
            tool_registry.register(Box::new(HttpRequestTool::new()));
            tool_registry.register(Box::new(GlobSearchTool::new(&config.workspace_dir)));
            tool_registry.register(Box::new(ContentSearchTool::new(&config.workspace_dir)));
            tool_registry.register(Box::new(FileEditTool::new(&config.workspace_dir)));
            tool_registry.register(Box::new(GitOperationsTool::new(&config.workspace_dir)));

            let tool_specs = tool_registry.get_specs();

            let context_builder = agent::ContextBuilder::new(&config.workspace_dir)
                .with_memory(memory.clone())
                .with_skills(skills)
                .with_tool_specs(tool_specs);

            let agent_loop =
                agent::AgentLoop::new(provider_arc.clone(), context_builder, tool_registry)
                    .with_max_iterations(config.max_iterations)
                    .with_max_history(config.max_history)
                    .with_model_name(config.model.clone())
                    .with_temperature(config.temperature)
                    .with_parallel_tools(config.parallel_tools);

            let agent_loop = Arc::new(agent_loop);

            if let Some(msg) = message {
                println!();
                let printer = agent::StatusPrinter::new();
                let (status_tx, mut status_rx) = mpsc::channel::<agent::StatusUpdate>(64);
                let agent = agent_loop.clone();
                let msg = msg.clone();
                let handle = tokio::spawn(async move {
                    agent.process_with_status(&msg, Some(status_tx)).await
                });

                while let Some(status) = status_rx.recv().await {
                    printer.print(&status);
                }

                let result = handle.await??;
                let width = crossterm::terminal::size()
                    .map(|(w, _)| w as usize)
                    .unwrap_or(80);
                let sep_width = width.min(80);
                eprintln!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(sep_width));
                repl::print_markdown(&result);
            } else {
                let mut handle = repl::start();

                loop {
                    match handle.recv().await {
                        Some(repl::ReplCommand::Input(input)) => {
                            println!();
                            let printer = agent::StatusPrinter::new();
                            let (status_tx, mut status_rx) = mpsc::channel::<agent::StatusUpdate>(64);
                            let agent = agent_loop.clone();
                            let input_clone = input.clone();
                            let process_handle = tokio::spawn(async move {
                                agent.process_with_status(&input_clone, Some(status_tx)).await
                            });

                            while let Some(status) = status_rx.recv().await {
                                printer.print(&status);
                            }

                            match process_handle.await? {
                                Ok(response) => {
                                    let width = crossterm::terminal::size()
                                        .map(|(w, _)| w as usize)
                                        .unwrap_or(80);
                                    let sep_width = width.min(80);
                                    eprintln!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(sep_width));
                                    repl::print_markdown(&response);
                                }
                                Err(e) => {
                                    eprintln!("❌ Error: {}", e);
                                }
                            }
                            println!();
                            handle.signal_done().await;
                        }
                        Some(repl::ReplCommand::Quit) | None => {
                            println!("\n👋 Goodbye!");
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
