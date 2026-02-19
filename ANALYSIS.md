# Dinoe Project Analysis

## Overview

Dinoe is a fast, lightweight, and memory-safe CLI AI agent written in Rust. It is designed to be a personal assistant that can help with various tasks, including coding, file management, and information retrieval.

## Architecture

The project is structured as a Rust workspace with two main members:

1.  **Core (`core/`)**: This library contains the core logic of the agent, including:
    *   **Agent**: Handles the conversation loop and context management.
    *   **Memory**: Implements a file-based memory system (Markdown).
    *   **Providers**: Integrates with LLM providers (currently only OpenAI).
    *   **Tools**: Provides tools for file manipulation and shell execution.
    *   **Config**: Manages configuration settings.

2.  **CLI (`cli/`)**: This is the command-line interface application that uses the core library. It handles:
    *   **Onboarding**: Guides the user through the initial setup.
    *   **Chat**: Provides an interactive chat interface.
    *   **Command execution**: Parses command-line arguments and invokes the appropriate core functions.

## Key Components

### Agent Loop (`core/src/agent/loop_.rs`)

The `AgentLoop` struct orchestrates the conversation. It:
1.  Takes a user message.
2.  Retrieves relevant context (memory, system prompt).
3.  Sends the context and message to the LLM provider.
4.  Processes the LLM response.
5.  Executes any tool calls requested by the LLM.
6.  Updates the memory with the conversation history.
7.  Compacts history if it exceeds the limit.

### Memory System (`core/src/memory/markdown.rs`)

Dinoe uses a simple but effective file-based memory system using Markdown files.
*   **Core Memory (`MEMORY.md`)**: Stores long-term, high-level information.
*   **Daily Logs (`memory/YYYY-MM-DD.md`)**: Stores daily conversation logs.

Memory retrieval is based on keyword matching.

### Tools (`core/src/tools/`)

The agent has access to several tools:
*   `FileRead`: Reads file content.
*   `FileWrite`: Writes file content.
*   `Shell`: Executes shell commands.
*   `MemoryRead`: Reads from memory.
*   `MemoryWrite`: Writes to memory.

### Context Builder (`core/src/agent/context.rs`)

The `ContextBuilder` constructs the system prompt dynamically. It includes:
*   **Soul (`SOUL.md`)**: The agent's persona and instructions.
*   **Runtime Context**: Current time and workspace location.
*   **Relevant Memory**: Context retrieved from memory based on the user's message.

## Configuration

Configuration is stored in `~/.dinoe/config.toml`. It includes:
*   API Key (OpenAI)
*   Model (default: gpt-4o)
*   Workspace directory
*   Max iterations and history size

## Strengths

*   **Performance**: Written in Rust, it is fast and memory-efficient.
*   **Simplicity**: The architecture is clean and easy to understand.
*   **Transparency**: Memory is stored in plain text (Markdown), making it easy for users to inspect and modify.
*   **Extensibility**: The modular design allows for easy addition of new tools and providers.

## Limitations

*   **Provider Support**: Currently only supports OpenAI.
*   **Memory Retrieval**: Simple keyword matching might not be sufficient for complex queries.
*   **Tooling**: Limited set of built-in tools compared to more complex agents.
