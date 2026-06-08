# Contributing to ccs

Thank you for your interest in contributing!

## Development setup

```bash
git clone https://github.com/xuzhu-591/ccs.git
cd ccs
cargo build          # debug build
cargo build --release
```

Run the binary directly from the build output:

```bash
./target/debug/ccs --help
./target/debug/ccs -p deepseek -n   # dry-run — safe to test without real keys
```

> ⚠️  Install as `ccs`, **never as `cc`** — `cc` is the system C compiler and shadowing it will
> break `cargo build` for every Rust project on your machine.

## Adding a new provider (config only — no code change)

Most contributions can be done purely in config. Add your provider example to
`src/default_providers.toml` with placeholder keys:

```toml
[[providers]]
id              = "my-provider"
provider        = "MyService"
model           = "my-model-v1"
executable      = "claude"
supports_resume = true
base_args       = ["--dangerously-skip-permissions"]

[providers.env]
ANTHROPIC_BASE_URL   = "https://api.myservice.com/anthropic"
ANTHROPIC_AUTH_TOKEN = "YOUR_MYSERVICE_API_KEY"
```

Use `YOUR_*` placeholders — **never commit real API keys**.

## Code structure

All logic lives in `src/main.rs` (~270 lines):

| Section | Lines | Responsibility |
|---|---|---|
| `Args` | top | CLI argument parsing (clap) |
| `Provider` / `Config` | structs | TOML deserialization |
| `config_path` / `load_providers` | fn | Config file loading + first-run generation |
| `last_id_path` / `read_last_id` / `save_last_id` | fn | Remember last selection |
| `build_menu_items` | fn | Three-column aligned display strings |
| `launch` | fn | Build command, handle dry-run, exec() |
| `main` | fn | Glue: parse → select → launch |

## Pull request guidelines

- Keep PRs focused — one feature or fix per PR
- Run `cargo clippy` and fix any warnings before submitting
- Test with `--dry-run` (`-n`) to verify command construction
- Do not commit real API keys or tokens — use `YOUR_*` placeholders in `default_providers.toml`
- Update `README.md` if you add a new flag or change config fields

## Reporting issues

Please include the output of `ccs -n -p <provider-id>` (dry-run, safe to share) to help diagnose command-construction issues.
