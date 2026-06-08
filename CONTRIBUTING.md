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

All logic lives in `src/main.rs`:

| Section | Responsibility |
|---|---|
| `Args` | CLI argument parsing (clap) |
| `Provider` / `Config` | TOML deserialization |
| `config_path` / `load_providers` / `parse_config` | Config file loading + parsing |
| `last_id_path` / `read_last_id` / `save_last_id` | Remember last selection |
| `build_menu_items` | Three-column aligned display strings |
| `build_launch_cmd` | Pure command construction (testable) |
| `launch` | Thin exec() wrapper |
| `main` | Glue: parse → select → launch |

## Testing

### Running tests

```bash
cargo test
```

All tests live in `src/main.rs` as a `#[cfg(test)] mod tests` block.

### Test categories

| Category | What it covers | Example |
|---|---|---|
| Config parsing | TOML → struct deserialization, required/optional fields, error cases | `parse_minimal_config`, `parse_invalid_executable` |
| Command building | Arg ordering, resume logic, env injection, passthrough | `build_cmd_claude_with_resume`, `build_cmd_codex_resume_as_subcommand` |
| Menu display | Column alignment, single/multi provider formatting | `menu_items_aligned` |
| Shell quoting | Special character handling for dry-run output | `shell_quote_with_spaces` |

### Writing new tests

When adding or modifying functionality, follow these guidelines:

1. **Test the pure logic, not the side effects.** Use `build_launch_cmd` / `parse_config` instead of testing through `launch` / `load_providers` which call `exec()` or `process::exit()`.
2. **Use the `make_provider` helper** for constructing test fixtures with sensible defaults.
3. **Cover both happy path and error cases.** For parsing: valid TOML + invalid/missing fields. For command building: with/without resume, different executable types.
4. **Test the embedded default config** — `parse_default_config_embedded` ensures the shipped `default_providers.toml` stays valid as you edit it.
5. **Name tests clearly** — `{function}_{scenario}` pattern, e.g. `build_cmd_resume_not_supported`.

### CI

GitHub Actions runs on every push to `main` and every PR:

- `cargo fmt --check` — formatting
- `cargo clippy -- -D warnings` — lints
- `cargo test` — unit tests
- `cargo build --release` — ensures release build compiles

CI must be green before merge.

## Pull request guidelines

- Keep PRs focused — one feature or fix per PR
- Run `cargo fmt`, `cargo clippy`, and `cargo test` before submitting
- Test with `--dry-run` (`-n`) to verify command construction manually
- Do not commit real API keys or tokens — use `YOUR_*` placeholders in `default_providers.toml`
- Update `README.md` if you add a new flag or change config fields
- Add tests for new logic (config parsing, command building, display formatting)

## Reporting issues

Please include the output of `ccs -n -p <provider-id>` (dry-run, safe to share) to help diagnose command-construction issues.
