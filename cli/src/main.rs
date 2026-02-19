use anyhow::Result;
use clap::{Parser, Subcommand};
use dinoe_core::{agent, config, providers, tools};
mod onboard;
use std::io::Write;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "dinoe")]
#[command(about = "dinoe - Fast, ultra-lightweight, and memory safe", long_about = None)]
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
        Commands::Chat { message } => {
            let config = config::load_config()?;

            let mut provider = providers::OpenAIProvider::new(config.api_key);
            provider = provider.with_model(config.model);
            if let Some(base_url) = config.base_url {
                provider = provider.with_base_url(base_url);
            }

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

            if let Err(e) = onboard::ensure_soul_file(&config.workspace_dir) {
                eprintln!("❌ Error: Could not create SOUL.md: {}", e);
                return Err(e);
            }

            let memory = dinoe_core::memory::create_memory(&config.workspace_dir)?;
            let context_builder =
                agent::ContextBuilder::new(&config.workspace_dir).with_memory(memory.clone());
            let tool_registry = Arc::new(agent::ToolRegistry::new());
            let provider_arc = Arc::new(provider);

            tool_registry.register(Arc::new(tools::FileReadTool::new(&config.workspace_dir)));
            tool_registry.register(Arc::new(tools::FileWriteTool::new(&config.workspace_dir)));
            tool_registry.register(Arc::new(tools::ShellTool::new(&config.workspace_dir)));
            tool_registry.register(Arc::new(tools::MemoryReadTool::new(memory.clone())));
            tool_registry.register(Arc::new(tools::MemoryWriteTool::new(memory)));

            let agent_loop =
                agent::AgentLoop::new(provider_arc.clone(), context_builder, tool_registry)
                    .with_max_iterations(config.max_iterations)
                    .with_max_history(config.max_history);

            let agent_loop = Arc::new(agent_loop);

            if let Some(msg) = message {
                println!("\n🤔 Processing...\n");
                match agent_loop.process(&msg).await {
                    Ok(response) => {
                        println!("{}", response);
                    }
                    Err(e) => {
                        eprintln!("❌ Error: {}", e);
                        anyhow::bail!("Agent processing failed: {}", e);
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

                            match agent_loop.process(input).await {
                                Ok(response) => {
                                    println!("{}", response);
                                }
                                Err(e) => {
                                    eprintln!("❌ Error: {}", e);
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
