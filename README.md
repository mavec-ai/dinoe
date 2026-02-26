# Dinoe

Fast, ultra-lightweight CLI AI agent with tool execution, skills, and multi-provider support.

## Features

- **Ultra-fast** - Sub-second startup
- **Tiny binary** - Only 2.3MB
- **Memory-safe** - Built with Rust
- **Multi-provider** - OpenAI, OpenRouter, Ollama, Z.AI (GLM)
- **Streaming** - Real-time response streaming
- **Smart loop detection** - Prevents infinite tool loops
- **Tool execution** - File ops, search, shell, git, web fetch, memory
- **Skills system** - Extensible with custom skills
- **Daily logs** - Automatic daily memory tracking in Markdown
- **Zero config** - Simple 5-step onboarding wizard
- **Security layer** - Path validation, symlink protection, rate limiting, SSRF prevention

## Installation

Install from GitHub:

```bash
cargo install --git https://github.com/mavec-ai/dinoe
```

Or build from source:

```bash
git clone https://github.com/mavec-ai/dinoe
cd dinoe
cargo build --release

./target/release/dinoe
```

## Quick Start

```bash
dinoe onboard
dinoe chat
```

## Usage

### Onboarding

```bash
dinoe onboard
```

5-step wizard:
1. Select provider (OpenAI, OpenRouter, Ollama, Z.AI)
2. Enter API key (skipped for Ollama)
3. Select endpoint (for Ollama/Z.AI)
4. Select model (live fetch for Ollama/OpenRouter)
5. Confirm configuration

### Interactive Chat

```bash
dinoe chat
```

Type messages and press Enter. Press Ctrl+D to exit.

### Single Message

```bash
dinoe chat -m "Hello, Dinoe!"
```

### Skills Management

```bash
dinoe skills list
dinoe skills install https://github.com/user/my-skill
dinoe skills install /path/to/local/skill
dinoe skills remove my-skill
```

Or create manually:

```bash
mkdir -p ~/.dinoe/workspace/skills/my-skill
echo '# My Skill' > ~/.dinoe/workspace/skills/my-skill/SKILL.md
```

## Configuration

Config stored at `~/.dinoe/config.toml`:

```toml
provider = "openai"
api_key = "sk-..."
model = "gpt-4o"
max_iterations = 20
max_history = 50
temperature = 1.0
parallel_tools = true

[stream]
enabled = true
```

## Workspace Structure

```
~/.dinoe/
├── config.toml             # Configuration
└── workspace/
    ├── SOUL.md             # Agent personality
    ├── TOOLS.md            # Tool usage guidelines
    ├── USER.md             # User preferences
    ├── memory/             # Memory & logs
    │   ├── MEMORY.md       # Long-term memory
    │   └── 2025-02-22.md   # Short-term memory
    └── skills/             # Custom skills
        └── my-skill/
            └── SKILL.md
```

## Built-in Tools

| Tool | Description |
|------|-------------|
| `file_read` | Read file contents |
| `file_write` | Write or create files |
| `file_edit` | Search and replace in files |
| `glob_search` | Find files by pattern |
| `content_search` | Search content with regex |
| `shell` | Execute shell commands |
| `git_operations` | Git commands (status, diff, log) |
| `web_fetch` | Fetch web content as markdown |
| `http_request` | HTTP API calls (GET, POST, etc) |
| `memory_read` | Search memory by keyword |
| `memory_write` | Store information to memory |

## Architecture

```
dinoe/
├── core/
│   ├── agent/       # Agent loop, context, registry
│   ├── providers/   # OpenAI, GLM, Ollama, OpenRouter
│   ├── tools/       # Built-in tools
│   ├── skills/      # Skill system
│   ├── memory/      # Memory management
│   ├── config/      # Configuration
│   └── traits/      # Core traits
└── cli/
    ├── main.rs      # Entry point
    ├── onboard.rs   # Onboarding wizard
    ├── skills.rs    # Skills CLI
    └── templates.rs # Default templates
```

## Performance

| Metric | Value |
|--------|-------|
| Binary size | 2.3 MB |
| Cold start | ~500 ms |
| Peak memory | ~2 MB |
| Architecture | arm64 / x86_64 |
| Code size | 8,625 lines Rust |

## License

[MIT License](LICENSE)
