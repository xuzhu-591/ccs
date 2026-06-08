# ccs — Claude Code Switcher

A fast, single-binary CLI launcher for [Claude Code](https://github.com/anthropics/claude-code) and [Codex](https://github.com/openai/codex) that lets you **interactively select a model provider** before each session — no more juggling shell aliases.

```
  TOOL     PROVIDER   MODEL
? 选择 ›
❯ claude   DeepSeek   deepseek-v4-pro
  claude   DeepSeek   deepseek-v4-flash
  claude   Mimo       mimo-v2.5-pro
  codex    OpenAI     gpt-4o
```

## Features

- **Interactive three-column menu** — tool / provider / model, navigated with arrow keys
- **Direct selection** via `-p <id>` to skip the menu
- **Resume support** — `-r` maps to `claude -r` or `codex resume` automatically
- **Dry-run mode** — `-n` prints the exact command and env vars without launching
- **Config-driven** — add or remove providers by editing `~/.config/ccs/config.toml`; no recompile needed
- **Remembers last choice** — the previous selection is highlighted by default
- **Zero runtime deps** — single static binary, ~1 MB

## Requirements

- macOS or Linux (uses `exec(2)` — Unix only)
- [`claude`](https://github.com/anthropics/claude-code) and/or [`codex`](https://github.com/openai/codex) in `PATH`
- Rust 1.70+ (for building from source)

## Installation

### From crates.io (recommended)

```bash
cargo install claude-code-switcher
```

### From source

```bash
git clone https://github.com/xuzhu-591/ccs.git
cd ccs
cargo build --release
cp target/release/ccs ~/.local/bin/   # or any directory in $PATH
```

> ⚠️  **Do not install as `cc`** — that name is reserved for the system C compiler and will break your Rust/C toolchain.

### Verify

```bash
ccs --version
```

## Configuration

On first run, `ccs` generates a template config at `~/.config/ccs/config.toml`.  
Edit it to add your API keys and desired providers.

```toml
# ~/.config/ccs/config.toml

[[providers]]
id              = "deepseek-pro"
provider        = "DeepSeek"
model           = "deepseek-v4-pro"
executable      = "claude"
supports_resume = true
base_args       = ["--dangerously-skip-permissions"]

[providers.env]
ANTHROPIC_BASE_URL             = "https://api.deepseek.com/anthropic"
ANTHROPIC_AUTH_TOKEN           = "YOUR_DEEPSEEK_API_KEY"
ANTHROPIC_MODEL                = "deepseek-v4-pro"
ANTHROPIC_DEFAULT_OPUS_MODEL   = "deepseek-v4-pro"
ANTHROPIC_DEFAULT_SONNET_MODEL = "deepseek-v4-pro"
ANTHROPIC_DEFAULT_HAIKU_MODEL  = "deepseek-v4-flash"
CLAUDE_CODE_SUBAGENT_MODEL     = "deepseek-v4-pro"
CLAUDE_CODE_EFFORT_LEVEL       = "max"
```

### Config fields

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | ✅ | Unique identifier, used with `-p` |
| `provider` | string | ✅ | Service name shown in column 2 |
| `model` | string | ✅ | Model name shown in column 3 |
| `executable` | `"claude"` \| `"codex"` | ✅ | Binary to launch |
| `supports_resume` | bool | — | Enable `-r` / resume (default: `false`) |
| `resume_as_subcommand` | bool | — | Use `codex resume` style instead of `-r` flag (default: `false`) |
| `base_args` | string[] | — | Args always prepended to the command |
| `[providers.env]` | table | — | Environment variables injected into the subprocess |

### Resume behaviour

| Provider type | Config | Resulting command |
|---|---|---|
| claude | `supports_resume = true` | `claude … -r` |
| codex | `supports_resume = true`<br>`resume_as_subcommand = true` | `codex resume …` |

## Usage

```
Usage: ccs [OPTIONS] [PASSTHROUGH]...

Arguments:
  [PASSTHROUGH]...  Arguments passed through to claude/codex

Options:
  -r, --resume         Resume the last session
  -p, --provider <ID>  Skip the menu and use a specific provider ID
  -n, --dry-run        Print the command that would run, without executing
  -h, --help           Print help
  -V, --version        Print version
```

### Examples

```bash
# Interactive provider selection
ccs

# Resume last session with the same interactive selection
ccs -r

# Jump straight to a specific provider
ccs -p deepseek

# Resume with a specific provider
ccs -p deepseek-pro -r

# Debug: see exactly what command would be executed
ccs -p codex -r -n

# Pass extra arguments to the underlying tool
ccs -p deepseek -- --print "explain this code"
```

### Dry-run output example

```
[dry-run] env:
  ANTHROPIC_AUTH_TOKEN=sk-...
  ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic
  ...
[dry-run] cmd:
  claude --dangerously-skip-permissions --print 'explain this code'
```

## Adding a custom provider

Add a new `[[providers]]` block to `~/.config/ccs/config.toml`:

```toml
[[providers]]
id              = "my-provider"
provider        = "MyService"
model           = "my-model-v1"
executable      = "claude"
supports_resume = true
base_args       = ["--dangerously-skip-permissions"]

[providers.env]
ANTHROPIC_BASE_URL   = "https://api.myservice.com/v1"
ANTHROPIC_AUTH_TOKEN = "sk-..."
```

No recompile needed — changes take effect immediately on the next run.

## Project structure

```
ccs/
├── src/
│   ├── main.rs                  # All logic (~270 lines)
│   └── default_providers.toml  # Template config embedded in the binary
├── Cargo.toml
├── Cargo.lock
├── LICENSE
├── README.md
└── CONTRIBUTING.md
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)
