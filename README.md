# Dinoe

Fast, ultra-lightweight, and memory-safe CLI AI agent.

## Features

- **Ultra-fast** - Starts in under 10ms
- **Tiny binary** - Only 2.0MB
- **Memory-safe** - Built with Rust
- **Daily logs** - Automatic daily memory tracking in Markdown
- **Zero config** - Simple onboarding, ready to use
- **Tools** - File operations, shell execution, memory management

## Installation

```bash
cargo install --git https://github.com/mavec-ai/dinoe
```

Or build from source:

```bash
cargo build --release

# Run the binary
./target/release/dinoe
```

## Quick Start

```bash
# First time setup
dinoe onboard

# Chat with AI
dinoe chat

# Send single message
dinoe chat -m "Hello, Dinoe!"
```

## Usage

### Onboarding

```bash
dinoe onboard
```

You'll be prompted for:
- OpenAI API key
- Model (default: gpt-4o)
- Workspace directory (default: ./workspace)

### Interactive Mode

```bash
dinoe chat
```

Type your messages and press Enter. Press Ctrl+D to exit.

### Single Message Mode

```bash
dinoe chat -m "Analyze this file"
```

## Configuration

Config is stored at `~/.dinoe/config.toml`:

```toml
api_key = "your-openai-api-key"
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
max_iterations = 20
max_history = 50
```

## Memory System

Dinoe stores your conversations in Markdown files:

- **Core memory** - `workspace/MEMORY.md`
- **Daily logs** - `workspace/memory/YYYY-MM-DD.md`

Memory is automatically indexed and retrieved for context-aware responses.

## Tools

- **FileRead** - Read file contents
- **FileWrite** - Write files
- **Shell** - Execute shell commands
- **MemoryRead** - Read from memory
- **MemoryWrite** - Write to memory

## License

MIT License - see [LICENSE](LICENSE) for details.

## Architecture

- `core/` - Core library with agent, memory, tools, and providers
- `cli/` - CLI application with onboarding and chat commands
