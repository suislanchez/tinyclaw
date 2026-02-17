<h1 align="center">TinyClaw</h1>

<p align="center">
  <strong>Tiny footprint. Maximum capability. 100% Rust.</strong><br>
  Ultra-efficient AI assistant with streaming TUI, parallel tools, and tiered builds.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/suislanchez/tinyclaw"><img src="https://img.shields.io/github/stars/suislanchez/tinyclaw?style=flat" alt="Stars" /></a>
</p>

```
~3.5MB binary · <10ms startup · 2,235 tests · 22+ providers · streaming TUI · parallel tools
```

---

## What is TinyClaw?

TinyClaw is a high-performance AI assistant built in Rust, forked from [ZeroClaw](https://github.com/theonlyhennygod/zeroclaw) with significant new capabilities:

| Feature | ZeroClaw | TinyClaw |
|---------|----------|----------|
| **Interface** | CLI only | CLI + ratatui TUI with markdown rendering |
| **Streaming** | None | Real-time SSE token streaming |
| **Token Tracking** | Stubbed | Live cost tracking across all providers |
| **Tool Execution** | Sequential | Parallel via `tokio::spawn` |
| **Sessions** | Lost on exit | Auto-saved to disk, resumable |
| **TUI Commands** | None | `/help`, `/cost`, `/clear`, `/model`, `/sessions`, `/export` |
| **Build Tiers** | Monolithic | 3-tier feature flags (tiny/standard/full) |

## Quick Start

```bash
git clone https://github.com/suislanchez/tinyclaw.git
cd tinyclaw
cargo build --release

# Setup
tinyclaw onboard --api-key sk-... --provider openrouter

# Launch TUI (recommended)
tinyclaw tui

# Or single message
tinyclaw agent -m "Hello!"

# Interactive CLI
tinyclaw agent
```

## Build Tiers

Choose the right build for your hardware:

```bash
# Tiny (~3.5MB) — CLI agent only, minimal deps
cargo build --release --no-default-features --features tiny

# Standard (~3.7MB) — adds TUI with ratatui
cargo build --release --no-default-features --features standard

# Full (~4.6MB) — everything: gateway, daemon, channels, OTel, skillforge
cargo build --release  # default
```

| Tier | Size | Includes |
|------|------|----------|
| **tiny** | ~3.5MB | CLI agent, providers, tools, memory, security |
| **standard** | ~3.7MB | + ratatui TUI with streaming & markdown |
| **full** | ~4.6MB | + gateway, daemon, channels, OTel, skillforge, tunnel |

## TUI Features

The TUI provides a rich terminal interface:

- **Real-time streaming** — tokens appear as they're generated via SSE
- **Markdown rendering** — bold, italic, code blocks, headings, lists
- **Live cost tracking** — token count, request count, estimated USD in status bar
- **Session persistence** — conversations auto-save and can be resumed
- **Slash commands:**

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/cost` | Detailed token usage breakdown |
| `/clear` | Clear history (keeps system prompt) |
| `/model` | Show current model |
| `/sessions` | List saved sessions |
| `/session` | Show current session ID |
| `/export` | Export conversation as markdown |
| `/quit` | Exit |

## Architecture

Every subsystem is a trait — swap implementations with a config change.

| Subsystem | Trait | Ships with |
|-----------|-------|------------|
| **AI Models** | `Provider` | 22+ providers (OpenRouter, Anthropic, OpenAI, Ollama, Groq, Mistral, xAI, DeepSeek, etc.) |
| **Channels** | `Channel` | CLI, Telegram, Discord, Slack, iMessage, Matrix, WhatsApp, Email |
| **Memory** | `Memory` | SQLite (hybrid FTS5 + vector search), Markdown |
| **Tools** | `Tool` | shell, file_read, file_write, memory (store/recall/forget), browser, composio |
| **Observability** | `Observer` | Noop, Log, OpenTelemetry |
| **Runtime** | `RuntimeAdapter` | Native (Mac/Linux/Pi) |
| **Security** | `SecurityPolicy` | Pairing, sandbox, allowlists, rate limits, encrypted secrets |

### Token Tracking

All providers (OpenRouter, OpenAI, Anthropic, Compatible) report token usage to a shared `UsageTracker` with atomic counters. The TUI status bar shows live metrics:

```
[1,247 tokens, 3 reqs, ~$0.0142]
```

### Parallel Tool Execution

When the LLM requests multiple tools in one response, TinyClaw executes them concurrently via `tokio::spawn` instead of sequentially. Results are collected in order.

### Streaming

OpenRouter supports real-time SSE streaming. The `Provider` trait includes `chat_with_history_stream` with a default fallback to non-streaming. The `ReliableProvider` wrapper tries streaming providers first and falls back automatically.

## Configuration

Config: `~/.tinyclaw/config.toml` (created by `onboard`)

```toml
api_key = "sk-..."
default_provider = "openrouter"
default_model = "anthropic/claude-sonnet-4-20250514"
default_temperature = 0.7

[memory]
backend = "sqlite"
auto_save = true

[autonomy]
level = "supervised"
workspace_only = true
allowed_commands = ["git", "npm", "cargo", "ls", "cat", "grep"]

[runtime]
kind = "native"

[browser]
enabled = false
allowed_domains = ["docs.rs"]
```

## Supported Providers

OpenRouter, Anthropic, OpenAI, Ollama, Gemini, Venice, Groq, Mistral, xAI/Grok, DeepSeek, Together AI, Fireworks AI, Perplexity, Cohere, GitHub Copilot, Moonshot, MiniMax, Bedrock, Cloudflare AI, Vercel AI, and any OpenAI-compatible endpoint via `custom:https://your-api.com`.

## Commands

| Command | Description |
|---------|-------------|
| `tinyclaw tui` | Launch TUI interface |
| `tinyclaw agent -m "..."` | Single message mode |
| `tinyclaw agent` | Interactive CLI mode |
| `tinyclaw onboard` | Setup wizard |
| `tinyclaw status` | System status + build tier |
| `tinyclaw gateway` | Start webhook server |
| `tinyclaw daemon` | Autonomous runtime |
| `tinyclaw doctor` | System diagnostics |

## Development

```bash
cargo build              # Dev build
cargo build --release    # Release build
cargo test               # 2,235 tests
cargo clippy             # Lint
cargo fmt                # Format
```

## License

MIT — see [LICENSE](LICENSE)

## Credits

Forked from [ZeroClaw](https://github.com/theonlyhennygod/zeroclaw) by [@theonlyhennygod](https://github.com/theonlyhennygod).

---

**TinyClaw** — Tiny footprint. Maximum capability. Deploy anywhere.
