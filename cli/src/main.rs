use anyhow::Result;
use clap::{Parser, Subcommand};
use dinoe_core::{
    agent, config,
    providers,
    tools::{FileReadTool, FileWriteTool, MemoryReadTool, MemoryWriteTool, ShellTool},
};
mod onboard;
mod skills;
mod templates;
use std::io::Write;
use std::sync::Arc;

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
                    .with_temperature(config.temperature);

            let agent_loop = Arc::new(agent_loop);

            let stream_enabled = config.stream.enabled;

            if let Some(msg) = message {
                println!("\n🤔 Processing...\n");
                if stream_enabled {
                    match agent_loop.process_stream(&msg).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("❌ Error: {}", e);
                            anyhow::bail!("Agent processing failed: {}", e);
                        }
                    }
                } else {
                    match agent_loop.process(&msg).await {
                        Ok(response) => {
                            println!("{}", response);
                        }
                        Err(e) => {
                            eprintln!("❌ Error: {}", e);
                            anyhow::bail!("Agent processing failed: {}", e);
                        }
                    }
                }
            } else {
                println!("🦖 Dinoe");
                println!("Type your message (Ctrl+D to exit):\n");
                use std::io::{self, BufRead};
                let stdin = io::stdin();
                let stdout = io::stdout();
                let mut stdout_lock = stdout.lock();

                loop {
                    print!("> ");
                    let _ = stdout_lock.flush();

                    let mut input = String::new();
                    let mut reader = stdin.lock();

                    match reader.read_line(&mut input) {
                        Ok(0) => {
                            println!("\n👋 Goodbye!");
                            break;
                        }
                        Ok(_) => {
                            let input = input.trim();
                            if input.is_empty() {
                                continue;
                            }

                            println!("\n🤔 Processing...\n");

                            if stream_enabled {
                                match agent_loop.process_stream(input).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("❌ Error: {}", e);
                                    }
                                }
                            } else {
                                match agent_loop.process(input).await {
                                    Ok(response) => {
                                        println!("{}", response);
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Error: {}", e);
                                    }
                                }
                            }

                            println!();
                        }
                        Err(_) => {
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
