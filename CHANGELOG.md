# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- CLI prompts and messages switched from Chinese to English for consistency with documentation.

### Added
- `cargo audit` step in CI to catch known dependency vulnerabilities.
- Automated GitHub Release creation on tag push.
- This CHANGELOG file.

## [0.2.5] - 2026-06-08

### Fixed
- CI publish step: use `CARGO_REGISTRY_TOKEN` secret and `--allow-dirty` flag.

### Added
- Auto-publish to crates.io on tag push (`v*`).
- Update instructions in README (cargo-update / `--force`).

## [0.2.4] - 2026-06-08

### Changed
- Renamed crate to `ccs-rs` (final name on crates.io).

## [0.2.3] - 2026-06-08

### Changed
- Renamed crate to `claude-code-switcher` (name not retained).

## [0.2.2] - 2026-06-08

### Changed
- Renamed crate to `ccs-cli` (name not retained).

## [0.2.1] - 2026-06-08

### Changed
- Renamed crate to `ccs-zx` for crates.io availability (name not retained).

## [0.2.0] - 2026-06-08

### Added
- Unit tests (19 tests covering config parsing, command building, menu display, shell quoting).
- GitHub Actions CI (fmt, clippy, test, build on Linux + macOS).
- `Cargo.toml` crate metadata for publishing.

## [0.1.0] - 2026-06-08

### Added
- Initial release.
- Interactive three-column menu (tool / provider / model).
- Direct provider selection via `-p <id>`.
- Resume support (`-r`) with auto-detection of flag vs subcommand style.
- Dry-run mode (`-n`) to preview commands.
- Config-driven via `~/.config/ccs/config.toml` (XDG-compliant).
- Remembers last-used provider across sessions.
- Single static binary, zero runtime dependencies.

[Unreleased]: https://github.com/xuzhu-591/ccs/compare/v0.2.5...HEAD
[0.2.5]: https://github.com/xuzhu-591/ccs/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/xuzhu-591/ccs/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/xuzhu-591/ccs/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/xuzhu-591/ccs/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/xuzhu-591/ccs/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/xuzhu-591/ccs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/xuzhu-591/ccs/releases/tag/v0.1.0
