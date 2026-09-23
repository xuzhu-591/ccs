# ccs — Claude Code Switcher

A fast, single-binary CLI launcher for [Claude Code](https://github.com/anthropics/claude-code) and [Codex](https://github.com/openai/codex) that lets you **interactively select a launch profile** before each session — no more juggling shell aliases.

```
Select profile
[Recent]   All     ←/→ switch list, ↑/↓ move, Enter select, Esc cancel

  TOOL     PROVIDER   MODEL
> claude   DeepSeek   deepseek-v4-pro
  claude   DeepSeek   deepseek-v4-flash
  claude   Mimo       mimo-v2.5-pro
```

## Features

- **Interactive three-column menu** — tool / provider / model, with Recent / All lists switched by left and right arrows
- **Direct selection** via `-p <id>` to skip the menu
- **Resume support** — `-r` maps to `claude -r` or `codex resume` automatically
- **Dry-run mode** — `-n` prints the exact command and env vars without launching
- **Config-driven** — add or remove profiles by editing `~/.config/ccs/config.toml`; no recompile needed
- **Remembers last choice** — the previous selection is highlighted by default
- **Zero runtime deps** — single static binary, ~1 MB

## Requirements

- macOS or Linux (uses `exec(2)` — Unix only)
- [`claude`](https://github.com/anthropics/claude-code) and/or [`codex`](https://github.com/openai/codex) in `PATH`
- Rust 1.95+ (for building from source)

## Installation

### From crates.io (recommended)

```bash
cargo install ccs-rs
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

### Updating

With [cargo-update](https://github.com/nabijaczleweli/cargo-update) (recommended):

```bash
cargo install cargo-update
cargo install-update ccs-rs   # update ccs-rs only
# cargo install-update -a     # or update all installed crates
```

Without extra tools:

```bash
cargo install ccs-rs --force
```

## Configuration

On first run, `ccs` generates a template config at `~/.config/ccs/config.toml`.  
Edit it to add your API keys and desired profiles.

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

Each `[[providers]]` block defines one **profile**: a provider, model, agent CLI, and launch settings. The existing TOML keys are retained. Profile IDs must be unique; `id`, `provider`, and `model` must not be blank.

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

| Agent | Config | Resulting command |
|---|---|---|
| claude | `supports_resume = true` | `claude … -r` |
| codex | `supports_resume = true`<br>`resume_as_subcommand = true` | `codex resume …` |

## Usage

```text
Claude Code / Codex launcher 🚀
Config: ~/.config/ccs/config.toml

Usage: ccs [OPTIONS] [PASSTHROUGH]... [COMMAND]

Commands:
  list  List configured profiles
  edit  Open the config file in $EDITOR (falls back to vi)
  help  Print this message or the help of the given subcommand(s)

Arguments:
  [PASSTHROUGH]...  Arguments passed through to claude/codex

Options:
  -r, --resume                Resume the last session (passes -r to claude)
  -p, --profile <PROFILE_ID>  Skip the menu and use a specific profile ID
  -n, --dry-run               Print the command that would run, without executing
      --show-secrets          Show full secret values in dry-run / list output (default: masked)
  -h, --help                  Print help
  -V, --version               Print version
```

### Examples

```bash
# Interactive profile selection
ccs

# Resume last session with the same interactive selection
ccs -r

# Jump straight to a specific profile
ccs -p deepseek

# Resume with a specific profile
ccs -p deepseek-pro -r

# Debug: see exactly what command would be executed
ccs -p codex -r -n

# Reveal the real API key in dry-run output (default is masked)
ccs -p deepseek -n --show-secrets

# Pass extra arguments to the underlying tool
ccs -p deepseek -- --print "explain this code"

# Inspect all configured profiles
ccs list

# Show environment configuration and local checks
ccs list --verbose

# Show original environment values
ccs list --verbose --show-secrets

# Edit your config in $EDITOR (falls back to vi)
ccs edit
```

### Recent selections

ccs remembers the **last 3** profile IDs you used and shows them in a
dedicated Recent list. The menu defaults to the most recent profile, and left
and right arrows switch between Recent and All profiles. The list is stored as
plain lines (newest first) in `~/.config/ccs/recent`.

### List profiles

`ccs list` shows only the profile ID, provider, model, and agent:

```text
ID         PROVIDER   MODEL             AGENT
────────────────────────────────────────────────────────
deepseek   DeepSeek   deepseek-v4-pro   Claude Code
codex      OpenAI     gpt-4o            Codex
```

The list preserves configuration order, aligns Unicode text, and uses a stacked layout in narrow terminals. Color is disabled when output is redirected or `NO_COLOR` is set.

`ccs list --verbose` adds each profile's agent path, environment configuration, and local check results. It checks unknown configuration fields and executable permissions using the profile's effective `PATH`, including a configured environment override. It does not execute the agent or check credentials and service connectivity. All local checks passing returns exit code 0; any failed check returns 1. Default `ccs list` only requires a structurally valid configuration and does not check agent availability.

### Environment values

`ccs list` does not display environment variables. `ccs list --verbose` and `--dry-run` mask **every environment value** as `***masked***`. Add `--show-secrets` to display original values. Other profile fields and command arguments are displayed as configured.

### Upgrading from 0.2.x

- Replace `ccs validate` with `ccs list --verbose`.
- Replace `--provider <ID>` with `--profile <PROFILE_ID>`. The short option `-p` is unchanged.
- Existing `[[providers]]` configuration and recent profile IDs are retained. Correct duplicate IDs or blank `id`, `provider`, and `model` fields before use.
- Unknown top-level or profile fields fail verbose checks. Custom keys inside `[providers.env]` remain supported.

### Dry-run output example

```
[dry-run] env:
  ANTHROPIC_AUTH_TOKEN=***masked***
  ANTHROPIC_BASE_URL=***masked***
  ...
[dry-run] cmd:
  claude --dangerously-skip-permissions --print 'explain this code'
```

## Adding a custom profile

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
│   ├── main.rs                # CLI, configuration, menu, and launching
│   ├── listing.rs             # List rendering and local checks
│   └── default_providers.toml # Template config embedded in the binary
├── tests/cli.rs               # CLI integration tests
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
