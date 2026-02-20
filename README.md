# Dinoe

Fast, ultra-lightweight CLI AI agent with tool execution and skills.

## Features

- **Ultra-fast** - Starts in under 10ms
- **Tiny binary** - Only 2.4MB
- **Memory-safe** - Built with Rust
- **Tool execution** - File operations, shell commands, memory management
- **Skills system** - Extensible with custom skills
- **Daily logs** - Automatic daily memory tracking in Markdown
- **Zero config** - Simple onboarding, ready to use

## Installation

Install from GitHub:

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

### Interactive Chat

```bash
dinoe chat
```

Type your messages and press Enter. Press Ctrl+D to exit.

### Single Message

```bash
dinoe chat -m "What files are in the current directory?"
```

### Using Tools

Dinoe can automatically use tools when needed:

```bash
dinoe chat -m "Read the README.md file"
dinoe chat -m "List all Rust files"
dinoe chat -m "What's the current date?"
```

### Skills

Skills provide additional functionality. Create skills in your workspace:

```
workspace/skills/
├── my-skill/
│   └── SKILL.md
```

Skills are automatically loaded and available during chat.

## Configuration

Config is stored at `~/.dinoe/config.toml`:

```toml
api_key = "your-openai-api-key"
model = "gpt-4o"
max_iterations = 20
max_history = 50
```

## Memory System

Dinoe stores conversations in Markdown files:

- **Core memory** - `workspace/MEMORY.md`
- **Daily logs** - `workspace/memory/YYYY-MM-DD.md`

Memory is automatically indexed and retrieved for context-aware responses.

## Built-in Tools

| Tool | Description |
|------|-------------|
| `file_read` | Read file contents |
| `file_write` | Write or create files |
| `shell` | Execute shell commands |
| `memory_read` | Search memory by keyword |
| `memory_write` | Store information to memory |

## Creating Skills

Skills are Markdown files that provide instructions to the AI:

1. Create a directory in `workspace/skills/your-skill/`
2. Add a `SKILL.md` file with instructions
3. Dinoe automatically loads it

Example `workspace/skills/code-reviewer/SKILL.md`:

```markdown
# Code Reviewer

You are a code reviewer. When asked to review code:
- Check for bugs and edge cases
- Suggest improvements
- Follow Rust best practices
```

## Architecture

```
dinoe/
├── core/          # Core library
│   ├── agent/     # Agent loop, context, registry
│   ├── providers/ # OpenAI integration
│   ├── tools/     # Built-in tools
│   ├── skills/    # Skill system
│   └── memory/    # Memory management
└── cli/           # CLI application
    └── src/
        ├── main.rs
        ├── onboard.rs
        └── skills.rs
```

## License

MIT License - see [LICENSE](LICENSE) for details.
