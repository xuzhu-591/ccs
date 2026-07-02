use clap::{Parser, Subcommand};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Attribute, Color, Stylize, style},
    terminal::{self, ClearType},
};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_CONFIG: &str = include_str!("default_providers.toml");

/// Max number of recently-used provider ids remembered.
const RECENT_MAX: usize = 3;

/// Env-key substrings (case-insensitive) whose values are masked by default.
const MASK_KEYWORDS: &[&str] = &["token", "key", "secret", "password"];

#[derive(Parser)]
#[command(
    name = "ccs",
    about = "Claude Code / Codex launcher 🚀\nConfig: ~/.config/ccs/config.toml",
    version
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Resume the last session (passes -r to claude)
    #[arg(short = 'r', long = "resume")]
    resume: bool,

    /// Skip the menu and use a specific provider ID
    #[arg(short = 'p', long = "provider", value_name = "ID")]
    provider: Option<String>,

    /// Print the command that would run, without executing
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,

    /// Show full secret values in dry-run / list output (default: masked)
    #[arg(long = "show-secrets")]
    show_secrets: bool,

    /// Arguments passed through to claude/codex
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    passthrough: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all configured providers
    List,
    /// Validate config and check that executables exist in PATH
    Validate,
    /// Open the config file in $EDITOR (falls back to vi)
    Edit,
}

#[derive(Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum Executable {
    Claude,
    Codex,
}

impl Executable {
    fn as_str(&self) -> &'static str {
        match self {
            Executable::Claude => "claude",
            Executable::Codex => "codex",
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
struct Provider {
    id: String,
    /// Service provider name, e.g. "DeepSeek", "OpenAI"
    provider: String,
    /// Model name, e.g. "claude-opus-4-6"
    model: String,
    executable: Executable,
    #[serde(default)]
    supports_resume: bool,
    /// If true, resume is a subcommand inserted before base_args (e.g. `codex resume`)
    /// If false (default), resume appends `-r` after base_args (e.g. `claude -r`)
    #[serde(default)]
    resume_as_subcommand: bool,
    #[serde(default)]
    base_args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct Config {
    providers: Vec<Provider>,
}

// ── Config ────────────────────────────────────────────────────────────────────

fn ccs_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(|d| PathBuf::from(d).join("ccs"))
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config").join("ccs")))
        .unwrap_or_else(|_| PathBuf::from("./ccs-config"))
}

fn config_path() -> PathBuf {
    ccs_dir().join("config.toml")
}

fn parse_config(content: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(content)
}

fn load_providers() -> Vec<Provider> {
    let path = config_path();

    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&path, DEFAULT_CONFIG) {
            eprintln!("⚠️  Failed to write default config {}: {e}", path.display());
        } else {
            eprintln!("📝 Default config created: {}\n", path.display());
        }
    }

    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("❌ Failed to read config {}: {e}", path.display());
        std::process::exit(1);
    });

    let config: Config = parse_config(&content).unwrap_or_else(|e| {
        eprintln!("❌ Failed to parse config ({}): {e}", path.display());
        std::process::exit(1);
    });

    if config.providers.is_empty() {
        eprintln!("❌ No providers defined in config");
        std::process::exit(1);
    }

    config.providers
}

// ── Recent selection ──────────────────────────────────────────────────────────

fn recent_path() -> PathBuf {
    ccs_dir().join("recent")
}

/// Pure LRU update used by `push_recent`; testable without touching disk.
fn update_recent_list(mut list: Vec<String>, id: &str) -> Vec<String> {
    list.retain(|x| x != id);
    list.insert(0, id.to_string());
    list.truncate(RECENT_MAX);
    list
}

fn read_recent() -> Vec<String> {
    let Ok(content) = fs::read_to_string(recent_path()) else {
        return Vec::new();
    };
    content
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(RECENT_MAX)
        .map(str::to_string)
        .collect()
}

fn push_recent(id: &str) {
    let path = recent_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut list = read_recent();
    list = update_recent_list(list, id);
    let body = if list.is_empty() {
        String::new()
    } else {
        list.join("\n") + "\n"
    };
    let _ = fs::write(path, body);
}

// ── Display formatting ────────────────────────────────────────────────────────

/// Compute column widths for the interactive menu / list table.
/// Returns `(exe_w, prov_w)` — the longest executable-name and provider-name lengths.
fn compute_widths(providers: &[Provider]) -> (usize, usize) {
    let exe_w = providers
        .iter()
        .map(|p| p.executable.as_str().len())
        .max()
        .unwrap_or(6);
    let prov_w = providers
        .iter()
        .map(|p| p.provider.len())
        .max()
        .unwrap_or(8);
    (exe_w, prov_w)
}

#[cfg(test)]
fn build_menu_items(providers: &[Provider]) -> Vec<String> {
    let (exe_w, prov_w) = compute_widths(providers);

    providers
        .iter()
        .map(|p| {
            format!(
                "{:<exe_w$}   {:<prov_w$}   {}",
                p.executable.as_str(),
                p.provider,
                p.model
            )
        })
        .collect()
}

// ── Interactive menu ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderList {
    Recent,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProviderMenuState {
    list: ProviderList,
    recent_index: usize,
    all_index: usize,
}

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(err) = execute!(io::stderr(), cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(err);
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}

fn build_recent_provider_indices(providers: &[Provider], recent: &[String]) -> Vec<usize> {
    let mut indices = Vec::new();
    for id in recent {
        if let Some(idx) = providers.iter().position(|p| p.id == *id)
            && !indices.contains(&idx)
        {
            indices.push(idx);
        }
    }
    indices
}

fn default_provider_menu_state(recent_indices: &[usize]) -> ProviderMenuState {
    if recent_indices.is_empty() {
        ProviderMenuState {
            list: ProviderList::All,
            recent_index: 0,
            all_index: 0,
        }
    } else {
        ProviderMenuState {
            list: ProviderList::Recent,
            recent_index: 0,
            all_index: recent_indices[0],
        }
    }
}

fn selected_provider_index(state: &ProviderMenuState, recent_indices: &[usize]) -> usize {
    match state.list {
        ProviderList::Recent => recent_indices[state.recent_index],
        ProviderList::All => state.all_index,
    }
}

fn switch_provider_list(
    state: &mut ProviderMenuState,
    target: ProviderList,
    recent_indices: &[usize],
    provider_count: usize,
) {
    if target == ProviderList::Recent && recent_indices.is_empty() {
        return;
    }

    let current_provider = selected_provider_index(state, recent_indices);
    state.list = target;

    match target {
        ProviderList::Recent => {
            state.recent_index = recent_indices
                .iter()
                .position(|idx| *idx == current_provider)
                .unwrap_or(0);
        }
        ProviderList::All => {
            state.all_index = if current_provider < provider_count {
                current_provider
            } else {
                0
            };
        }
    }
}

fn move_provider_menu_selection(
    state: &mut ProviderMenuState,
    delta: isize,
    recent_indices: &[usize],
    provider_count: usize,
) {
    let (current, len) = match state.list {
        ProviderList::Recent => (state.recent_index, recent_indices.len()),
        ProviderList::All => (state.all_index, provider_count),
    };
    if len == 0 {
        return;
    }

    let next = (current as isize + delta).rem_euclid(len as isize) as usize;
    match state.list {
        ProviderList::Recent => {
            state.recent_index = next;
            state.all_index = recent_indices[next];
        }
        ProviderList::All => {
            state.all_index = next;
        }
    }
}

fn provider_list_label(list: ProviderList) -> &'static str {
    match list {
        ProviderList::Recent => "Recent",
        ProviderList::All => "All",
    }
}

fn truncate_to_width(s: &str, max_width: usize) -> String {
    let len = s.chars().count();
    if len <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    s.chars().take(max_width - 3).collect::<String>() + "..."
}

fn write_menu_line(out: &mut impl Write, line: &str, width: u16) -> io::Result<()> {
    let width = width.saturating_sub(1).max(1) as usize;
    let line = truncate_to_width(line, width);
    write!(out, "{line}\r\n")
}

#[derive(Clone, Copy)]
struct MenuStyle {
    color: Color,
    bold: bool,
}

impl MenuStyle {
    const fn new(color: Color, bold: bool) -> Self {
        Self { color, bold }
    }
}

const STYLE_DEFAULT: MenuStyle = MenuStyle::new(Color::White, false);
const STYLE_DEFAULT_BOLD: MenuStyle = MenuStyle::new(Color::White, true);
const STYLE_DIM: MenuStyle = MenuStyle::new(Color::DarkGrey, false);
const STYLE_DIM_BOLD: MenuStyle = MenuStyle::new(Color::DarkGrey, true);
const STYLE_CYAN: MenuStyle = MenuStyle::new(Color::Cyan, false);
const STYLE_CYAN_BOLD: MenuStyle = MenuStyle::new(Color::Cyan, true);
const STYLE_GREEN_BOLD: MenuStyle = MenuStyle::new(Color::Green, true);
const STYLE_YELLOW: MenuStyle = MenuStyle::new(Color::Yellow, false);
const STYLE_YELLOW_BOLD: MenuStyle = MenuStyle::new(Color::Yellow, true);
const STYLE_PROVIDER_BOLD: MenuStyle = MenuStyle::new(Color::Blue, true);
const STYLE_MODEL_BOLD: MenuStyle = MenuStyle::new(Color::Magenta, true);

fn menu_colors_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn write_styled_menu_line(
    out: &mut impl Write,
    segments: &[(&str, MenuStyle)],
    width: u16,
    colors_enabled: bool,
) -> io::Result<()> {
    let mut remaining = width.saturating_sub(1).max(1) as usize;

    for (text, menu_style) in segments {
        if remaining == 0 {
            break;
        }

        let visible_text = truncate_to_width(text, remaining);
        remaining = remaining.saturating_sub(visible_text.chars().count());

        if colors_enabled {
            let mut styled = style(visible_text).with(menu_style.color);
            if menu_style.bold {
                styled = styled.attribute(Attribute::Bold);
            }
            write!(out, "{styled}")?;
        } else {
            write!(out, "{visible_text}")?;
        }
    }

    write!(out, "\r\n")
}

fn render_provider_menu(
    out: &mut impl Write,
    providers: &[Provider],
    recent_indices: &[usize],
    state: &ProviderMenuState,
    previous_line_count: u16,
) -> io::Result<u16> {
    if previous_line_count > 0 {
        execute!(out, cursor::MoveUp(previous_line_count))?;
    }
    execute!(out, terminal::Clear(ClearType::FromCursorDown))?;

    let width = terminal::size().map(|(width, _)| width).unwrap_or(100);
    let colors_enabled = menu_colors_enabled();
    let list_label = format!("[{}]", provider_list_label(state.list));
    write_styled_menu_line(
        out,
        &[
            ("?", STYLE_CYAN_BOLD),
            (" Select ", STYLE_DEFAULT),
            (&list_label, STYLE_YELLOW_BOLD),
            (" ", STYLE_DEFAULT),
            ("›", STYLE_GREEN_BOLD),
        ],
        width,
        colors_enabled,
    )?;

    let (exe_w, prov_w) = compute_widths(providers);
    let header = format!("  {:<exe_w$}   {:<prov_w$}   MODEL", "TOOL", "PROVIDER");
    write_styled_menu_line(out, &[(&header, STYLE_DIM_BOLD)], width, colors_enabled)?;

    let active_indices: Vec<usize> = match state.list {
        ProviderList::Recent => recent_indices.to_vec(),
        ProviderList::All => (0..providers.len()).collect(),
    };
    let line_count = active_indices.len().saturating_add(4) as u16;
    let selected = selected_provider_index(state, recent_indices);

    for idx in active_indices {
        let provider = &providers[idx];
        let is_selected = idx == selected;
        let marker = if is_selected { "❯ " } else { "  " };
        let exe = format!("{:<exe_w$}", provider.executable.as_str());
        let provider_name = format!("{:<prov_w$}", provider.provider);
        let row_style = if is_selected {
            STYLE_DEFAULT_BOLD
        } else {
            STYLE_DIM
        };
        let provider_style = if is_selected {
            STYLE_PROVIDER_BOLD
        } else {
            STYLE_DIM
        };
        let model_style = if is_selected {
            STYLE_MODEL_BOLD
        } else {
            STYLE_DIM
        };

        write_styled_menu_line(
            out,
            &[
                (marker, STYLE_GREEN_BOLD),
                (&exe, row_style),
                ("   ", STYLE_DEFAULT),
                (&provider_name, provider_style),
                ("   ", STYLE_DEFAULT),
                (&provider.model, model_style),
            ],
            width,
            colors_enabled,
        )?;
    }

    write_menu_line(out, "", width)?;
    write_styled_menu_line(
        out,
        &[
            ("←/→", STYLE_CYAN),
            (" switch list · ", STYLE_DIM),
            ("↑/↓", STYLE_CYAN),
            (" move · ", STYLE_DIM),
            ("Enter", STYLE_YELLOW),
            (" select · ", STYLE_DIM),
            ("Esc", STYLE_YELLOW),
            (" cancel", STYLE_DIM),
        ],
        width,
        colors_enabled,
    )?;

    out.flush().map(|_| line_count)
}

fn select_provider_interactive(
    providers: &[Provider],
    recent: &[String],
) -> io::Result<Option<usize>> {
    let recent_indices = build_recent_provider_indices(providers, recent);
    let mut state = default_provider_menu_state(&recent_indices);
    let _guard = TerminalGuard::new()?;
    let mut stderr = io::stderr();
    let mut rendered_lines = 0;

    loop {
        rendered_lines = render_provider_menu(
            &mut stderr,
            providers,
            &recent_indices,
            &state,
            rendered_lines,
        )?;

        if let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event::read()?
        {
            match code {
                KeyCode::Up => {
                    move_provider_menu_selection(&mut state, -1, &recent_indices, providers.len())
                }
                KeyCode::Down => {
                    move_provider_menu_selection(&mut state, 1, &recent_indices, providers.len())
                }
                KeyCode::Left => switch_provider_list(
                    &mut state,
                    ProviderList::Recent,
                    &recent_indices,
                    providers.len(),
                ),
                KeyCode::Right => switch_provider_list(
                    &mut state,
                    ProviderList::All,
                    &recent_indices,
                    providers.len(),
                ),
                KeyCode::Enter => {
                    return Ok(Some(selected_provider_index(&state, &recent_indices)));
                }
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => return Ok(None),
                _ => {}
            }
        }
    }
}

// ── Secret masking ────────────────────────────────────────────────────────────

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    MASK_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

fn mask_value(key: &str, value: &str) -> String {
    if is_sensitive_key(key) {
        format!("***masked (len={})***", value.chars().count())
    } else {
        value.to_string()
    }
}

// ── Command building ─────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
struct LaunchCmd {
    binary: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn build_launch_cmd(entry: &Provider, resume: bool, passthrough: &[String]) -> LaunchCmd {
    let binary = entry.executable.as_str().to_string();
    let mut args = Vec::new();

    if resume && entry.supports_resume && entry.resume_as_subcommand {
        args.push("resume".to_string());
    }
    for arg in &entry.base_args {
        args.push(arg.clone());
    }
    if resume && entry.supports_resume && !entry.resume_as_subcommand {
        args.push("-r".to_string());
    }
    for arg in passthrough {
        args.push(arg.clone());
    }

    let mut env: Vec<(String, String)> = entry
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.sort_by(|a, b| a.0.cmp(&b.0));

    LaunchCmd { binary, args, env }
}

fn shell_quote(s: &str) -> String {
    if s.chars().any(|c| " \t\"'\\{}[]()=".contains(c)) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

// ── Launch ────────────────────────────────────────────────────────────────────

fn launch(
    entry: &Provider,
    resume: bool,
    dry_run: bool,
    show_secrets: bool,
    passthrough: &[String],
) -> ! {
    if resume && !entry.supports_resume {
        eprintln!("⚠️  `{}` does not support resume, ignoring", entry.id);
    }

    let cmd_info = build_launch_cmd(entry, resume, passthrough);

    if dry_run {
        eprintln!("[dry-run] env:");
        for (k, v) in &cmd_info.env {
            let display = if show_secrets {
                v.clone()
            } else {
                mask_value(k, v)
            };
            eprintln!("  {}={}", k, display);
        }
        let args_str = cmd_info
            .args
            .iter()
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("[dry-run] cmd:");
        eprintln!("  {} {}", cmd_info.binary, args_str);
        std::process::exit(0);
    }

    let mut cmd = Command::new(&cmd_info.binary);
    for (k, v) in &cmd_info.env {
        cmd.env(k, v);
    }
    for arg in &cmd_info.args {
        cmd.arg(arg);
    }

    let err = cmd.exec();
    eprintln!("❌ Failed to launch {}: {err}", cmd_info.binary);
    std::process::exit(1);
}

// ── Subcommands ───────────────────────────────────────────────────────────────

fn cmd_list(providers: &[Provider], show_secrets: bool) {
    let (exe_w, prov_w) = compute_widths(providers);
    let id_w = providers
        .iter()
        .map(|p| p.id.len())
        .max()
        .unwrap_or(2)
        .max("ID".len());
    let model_w = providers
        .iter()
        .map(|p| p.model.len())
        .max()
        .unwrap_or(5)
        .max("MODEL".len());

    println!(
        "{:<id_w$}  {:<exe_w$}  {:<prov_w$}  {:<model_w$}  RESUME",
        "ID", "TOOL", "PROVIDER", "MODEL"
    );
    for p in providers {
        println!(
            "{:<id_w$}  {:<exe_w$}  {:<prov_w$}  {:<model_w$}  {}",
            p.id,
            p.executable.as_str(),
            p.provider,
            p.model,
            if p.supports_resume { "yes" } else { "no" },
        );
    }

    let with_env: Vec<&Provider> = providers.iter().filter(|p| !p.env.is_empty()).collect();
    if !with_env.is_empty() {
        let label = if show_secrets {
            "env:"
        } else {
            "env (masked; pass --show-secrets to reveal):"
        };
        println!("\n{}", label);
        for p in with_env {
            println!("  [{}]", p.id);
            let mut env: Vec<(&String, &String)> = p.env.iter().collect();
            env.sort_by(|a, b| a.0.cmp(b.0));
            for (k, v) in env {
                let display = if show_secrets {
                    v.clone()
                } else {
                    mask_value(k, v)
                };
                println!("    {}={}", k, display);
            }
        }
    }
}

fn cmd_validate(providers: &[Provider]) -> i32 {
    let mut ok = true;
    for p in providers {
        let exe = p.executable.as_str();
        match Command::new("which").arg(exe).output() {
            Ok(out) if out.status.success() => {
                let path = String::from_utf8_lossy(&out.stdout);
                let path = path.trim();
                println!("✓ {:<20} {} ({})", p.id, exe, path);
            }
            _ => {
                println!("✗ {:<20} {} — NOT FOUND in PATH", p.id, exe);
                ok = false;
            }
        }
    }
    if ok { 0 } else { 1 }
}

fn cmd_edit() -> std::io::Result<()> {
    let path = config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, DEFAULT_CONFIG)?;
        eprintln!("📝 Default config created: {}\n", path.display());
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor).arg(&path).status()?;
    std::process::exit(status.code().unwrap_or(1));
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    match args.command {
        Some(Commands::List) => {
            let providers = load_providers();
            cmd_list(&providers, args.show_secrets);
            std::process::exit(0);
        }
        Some(Commands::Validate) => {
            let providers = load_providers();
            std::process::exit(cmd_validate(&providers));
        }
        Some(Commands::Edit) => {
            let _ = cmd_edit();
            return;
        }
        None => {}
    }

    let providers = load_providers();

    let entry = if let Some(ref id) = args.provider {
        match providers.iter().find(|p| p.id == *id) {
            Some(p) => p.clone(),
            None => {
                eprintln!("❌ Unknown provider ID: {id}");
                let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
                eprintln!("Available IDs: {}", ids.join(", "));
                std::process::exit(1);
            }
        }
    } else {
        let recent = read_recent();
        let selection = match select_provider_interactive(&providers, &recent) {
            Ok(opt) => opt,
            Err(io_err) => {
                eprintln!("❌ I/O error during selection: {io_err}");
                std::process::exit(1);
            }
        };

        match selection {
            Some(idx) => providers[idx].clone(),
            None => {
                eprintln!("Cancelled");
                std::process::exit(0);
            }
        }
    };

    push_recent(&entry.id);
    if !args.dry_run {
        eprintln!(
            "🚀 {} / {} / {}",
            entry.executable.as_str(),
            entry.provider,
            entry.model
        );
    }
    launch(
        &entry,
        args.resume,
        args.dry_run,
        args.show_secrets,
        &args.passthrough,
    );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(
        id: &str,
        exe: Executable,
        supports_resume: bool,
        resume_as_subcommand: bool,
    ) -> Provider {
        Provider {
            id: id.to_string(),
            provider: "TestProvider".to_string(),
            model: "test-model".to_string(),
            executable: exe,
            supports_resume,
            resume_as_subcommand,
            base_args: vec!["--flag".to_string()],
            env: HashMap::from([("KEY".to_string(), "value".to_string())]),
        }
    }

    // ── Config parsing ───────────────────────────────────────────────────────

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[[providers]]
id = "test"
provider = "Test"
model = "m1"
executable = "claude"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].id, "test");
        assert_eq!(config.providers[0].executable, Executable::Claude);
        assert!(!config.providers[0].supports_resume);
        assert!(config.providers[0].base_args.is_empty());
        assert!(config.providers[0].env.is_empty());
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[[providers]]
id = "ds"
provider = "DeepSeek"
model = "deepseek-v4-pro"
executable = "claude"
supports_resume = true
base_args = ["--dangerously-skip-permissions"]

[providers.env]
ANTHROPIC_BASE_URL = "https://api.deepseek.com/anthropic"
ANTHROPIC_AUTH_TOKEN = "YOUR_KEY"
"#;
        let config = parse_config(toml).unwrap();
        let p = &config.providers[0];
        assert_eq!(p.id, "ds");
        assert!(p.supports_resume);
        assert!(!p.resume_as_subcommand);
        assert_eq!(p.base_args, vec!["--dangerously-skip-permissions"]);
        assert_eq!(
            p.env.get("ANTHROPIC_BASE_URL").unwrap(),
            "https://api.deepseek.com/anthropic"
        );
    }

    #[test]
    fn parse_multiple_providers() {
        let toml = r#"
[[providers]]
id = "a"
provider = "A"
model = "m1"
executable = "claude"

[[providers]]
id = "b"
provider = "B"
model = "m2"
executable = "codex"
supports_resume = true
resume_as_subcommand = true
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.providers[1].executable, Executable::Codex);
        assert!(config.providers[1].resume_as_subcommand);
    }

    #[test]
    fn parse_invalid_executable() {
        let toml = r#"
[[providers]]
id = "x"
provider = "X"
model = "m"
executable = "unknown"
"#;
        assert!(parse_config(toml).is_err());
    }

    #[test]
    fn parse_missing_required_field() {
        let toml = r#"
[[providers]]
id = "x"
provider = "X"
executable = "claude"
"#;
        assert!(parse_config(toml).is_err());
    }

    #[test]
    fn parse_empty_providers() {
        let toml = "providers = []\n";
        let config = parse_config(toml).unwrap();
        assert!(config.providers.is_empty());
    }

    #[test]
    fn parse_default_config_embedded() {
        let config = parse_config(DEFAULT_CONFIG).unwrap();
        assert!(!config.providers.is_empty());
        for p in &config.providers {
            assert!(!p.id.is_empty());
            assert!(!p.model.is_empty());
        }
    }

    // ── Command building ─────────────────────────────────────────────────────

    #[test]
    fn build_cmd_claude_no_resume() {
        let p = make_provider("test", Executable::Claude, true, false);
        let cmd = build_launch_cmd(&p, false, &[]);
        assert_eq!(cmd.binary, "claude");
        assert_eq!(cmd.args, vec!["--flag"]);
        assert_eq!(cmd.env, vec![("KEY".to_string(), "value".to_string())]);
    }

    #[test]
    fn build_cmd_claude_with_resume() {
        let p = make_provider("test", Executable::Claude, true, false);
        let cmd = build_launch_cmd(&p, true, &[]);
        assert_eq!(cmd.args, vec!["--flag", "-r"]);
    }

    #[test]
    fn build_cmd_codex_resume_as_subcommand() {
        let p = make_provider("test", Executable::Codex, true, true);
        let cmd = build_launch_cmd(&p, true, &[]);
        assert_eq!(cmd.binary, "codex");
        assert_eq!(cmd.args[0], "resume");
        assert_eq!(cmd.args[1], "--flag");
        assert!(!cmd.args.contains(&"-r".to_string()));
    }

    #[test]
    fn build_cmd_resume_not_supported() {
        let p = make_provider("test", Executable::Claude, false, false);
        let cmd = build_launch_cmd(&p, true, &[]);
        assert_eq!(cmd.args, vec!["--flag"]);
        assert!(!cmd.args.contains(&"-r".to_string()));
    }

    #[test]
    fn build_cmd_passthrough_args() {
        let p = make_provider("test", Executable::Claude, false, false);
        let pass = vec!["--print".to_string(), "hello world".to_string()];
        let cmd = build_launch_cmd(&p, false, &pass);
        assert_eq!(cmd.args, vec!["--flag", "--print", "hello world"]);
    }

    #[test]
    fn build_cmd_env_sorted() {
        let mut p = make_provider("test", Executable::Claude, false, false);
        p.env = HashMap::from([
            ("Z_VAR".to_string(), "z".to_string()),
            ("A_VAR".to_string(), "a".to_string()),
        ]);
        let cmd = build_launch_cmd(&p, false, &[]);
        assert_eq!(cmd.env[0].0, "A_VAR");
        assert_eq!(cmd.env[1].0, "Z_VAR");
    }

    // ── Menu display ─────────────────────────────────────────────────────────

    #[test]
    fn menu_items_aligned() {
        let providers = vec![
            Provider {
                id: "a".to_string(),
                provider: "DeepSeek".to_string(),
                model: "v4-pro".to_string(),
                executable: Executable::Claude,
                supports_resume: false,
                resume_as_subcommand: false,
                base_args: vec![],
                env: HashMap::new(),
            },
            Provider {
                id: "b".to_string(),
                provider: "OpenAI".to_string(),
                model: "gpt-4o".to_string(),
                executable: Executable::Codex,
                supports_resume: false,
                resume_as_subcommand: false,
                base_args: vec![],
                env: HashMap::new(),
            },
        ];
        let items = build_menu_items(&providers);
        assert_eq!(items.len(), 2);
        // Both lines should have the same length for the fixed columns
        let col1_end: usize = items[0].find("DeepSeek").unwrap();
        let col1_end_b: usize = items[1].find("OpenAI").unwrap();
        assert_eq!(col1_end, col1_end_b);
    }

    #[test]
    fn menu_items_single_provider() {
        let providers = vec![Provider {
            id: "solo".to_string(),
            provider: "Solo".to_string(),
            model: "m".to_string(),
            executable: Executable::Claude,
            supports_resume: false,
            resume_as_subcommand: false,
            base_args: vec![],
            env: HashMap::new(),
        }];
        let items = build_menu_items(&providers);
        assert_eq!(items.len(), 1);
        assert!(items[0].contains("claude"));
        assert!(items[0].contains("Solo"));
        assert!(items[0].contains("m"));
    }

    #[test]
    fn recent_provider_indices_filter_missing_and_duplicates() {
        let providers = vec![
            make_provider("a", Executable::Claude, false, false),
            make_provider("b", Executable::Claude, false, false),
            make_provider("c", Executable::Claude, false, false),
        ];
        let recent = vec![
            "c".to_string(),
            "missing".to_string(),
            "b".to_string(),
            "c".to_string(),
        ];

        let indices = build_recent_provider_indices(&providers, &recent);
        assert_eq!(indices, vec![2, 1]);
    }

    #[test]
    fn menu_defaults_to_recent_first_provider() {
        let state = default_provider_menu_state(&[2, 1]);

        assert_eq!(state.list, ProviderList::Recent);
        assert_eq!(state.recent_index, 0);
        assert_eq!(state.all_index, 2);
        assert_eq!(selected_provider_index(&state, &[2, 1]), 2);
    }

    #[test]
    fn menu_defaults_to_all_without_recent() {
        let state = default_provider_menu_state(&[]);

        assert_eq!(state.list, ProviderList::All);
        assert_eq!(state.recent_index, 0);
        assert_eq!(state.all_index, 0);
    }

    #[test]
    fn menu_switch_preserves_current_provider_when_present() {
        let recent_indices = vec![2, 1];
        let mut state = default_provider_menu_state(&recent_indices);

        switch_provider_list(&mut state, ProviderList::All, &recent_indices, 3);
        assert_eq!(state.list, ProviderList::All);
        assert_eq!(state.all_index, 2);

        move_provider_menu_selection(&mut state, -1, &recent_indices, 3);
        assert_eq!(state.all_index, 1);

        switch_provider_list(&mut state, ProviderList::Recent, &recent_indices, 3);
        assert_eq!(state.list, ProviderList::Recent);
        assert_eq!(state.recent_index, 1);
        assert_eq!(selected_provider_index(&state, &recent_indices), 1);
    }

    #[test]
    fn menu_switch_to_recent_falls_back_to_first_recent_provider() {
        let recent_indices = vec![2];
        let mut state = ProviderMenuState {
            list: ProviderList::All,
            recent_index: 0,
            all_index: 1,
        };

        switch_provider_list(&mut state, ProviderList::Recent, &recent_indices, 3);

        assert_eq!(state.list, ProviderList::Recent);
        assert_eq!(state.recent_index, 0);
        assert_eq!(selected_provider_index(&state, &recent_indices), 2);
    }

    #[test]
    fn menu_move_wraps_and_syncs_recent_selection_to_all_index() {
        let recent_indices = vec![2, 1];
        let mut state = ProviderMenuState {
            list: ProviderList::All,
            recent_index: 0,
            all_index: 0,
        };

        move_provider_menu_selection(&mut state, -1, &recent_indices, 3);
        assert_eq!(state.all_index, 2);

        switch_provider_list(&mut state, ProviderList::Recent, &recent_indices, 3);
        move_provider_menu_selection(&mut state, 1, &recent_indices, 3);

        assert_eq!(state.recent_index, 1);
        assert_eq!(state.all_index, 1);
    }

    #[test]
    fn truncate_to_width_keeps_short_lines() {
        assert_eq!(truncate_to_width("abc", 3), "abc");
        assert_eq!(truncate_to_width("abc", 5), "abc");
    }

    #[test]
    fn truncate_to_width_shortens_long_lines() {
        assert_eq!(truncate_to_width("abcdef", 6), "abcdef");
        assert_eq!(truncate_to_width("abcdef", 5), "ab...");
        assert_eq!(truncate_to_width("abcdef", 2), "..");
    }

    #[test]
    fn write_menu_line_uses_crlf_for_raw_mode() {
        let mut out = Vec::new();
        write_menu_line(&mut out, "abc", 80).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "abc\r\n");
    }

    #[test]
    fn write_styled_menu_line_plain_when_colors_disabled() {
        let mut out = Vec::new();
        write_styled_menu_line(
            &mut out,
            &[("abc", STYLE_CYAN_BOLD), ("def", STYLE_GREEN_BOLD)],
            80,
            false,
        )
        .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "abcdef\r\n");
    }

    // ── Shell quoting ────────────────────────────────────────────────────────

    #[test]
    fn shell_quote_plain() {
        assert_eq!(shell_quote("hello"), "hello");
        assert_eq!(shell_quote("--flag"), "--flag");
    }

    #[test]
    fn shell_quote_with_spaces() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
    }

    #[test]
    fn shell_quote_with_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_quote_with_special_chars() {
        assert_eq!(shell_quote("a=b"), "'a=b'");
        assert_eq!(shell_quote("{json}"), "'{json}'");
    }

    // ── Secret masking ───────────────────────────────────────────────────────

    #[test]
    fn mask_value_plain_keys() {
        assert_eq!(mask_value("ANTHROPIC_BASE_URL", "https://x"), "https://x");
        assert_eq!(mask_value("CLAUDE_CODE_EFFORT_LEVEL", "max"), "max");
        assert_eq!(mask_value("REGION", "us-east-1"), "us-east-1");
    }

    #[test]
    fn mask_value_token_substring() {
        let secret = "sk-1234567890abcdef"; // 19 chars
        let masked = mask_value("ANTHROPIC_AUTH_TOKEN", secret);
        assert!(masked.starts_with("***masked"));
        assert!(masked.contains("len=19"));
        assert!(!masked.contains("sk-"));
        assert!(!masked.contains("1234567890"));
    }

    #[test]
    fn mask_value_key_substring_case_insensitive() {
        assert!(mask_value("openai_api_key", "secret").contains("masked"));
        assert!(mask_value("MIMO_API_KEY", "secret").contains("masked"));
        assert!(mask_value("API_KEY", "x").contains("masked"));
        assert!(mask_value("private-key", "x").contains("masked"));
    }

    #[test]
    fn mask_value_secret_and_password() {
        assert!(mask_value("DB_PASSWORD", "hunter2").contains("masked"));
        assert!(mask_value("client_secret", "x").contains("masked"));
        assert!(mask_value("SecretKey", "x").contains("masked"));
    }

    #[test]
    fn mask_value_no_match() {
        assert_eq!(mask_value("PATH", "/usr/bin"), "/usr/bin");
        assert_eq!(mask_value("HOME", "/root"), "/root");
        assert_eq!(mask_value("LANG", "en_US"), "en_US");
    }

    // ── compute_widths ───────────────────────────────────────────────────────

    #[test]
    fn compute_widths_multi() {
        let providers = vec![
            make_provider("a", Executable::Claude, false, false),
            Provider {
                id: "b".to_string(),
                provider: "OpenAI".to_string(),
                model: "gpt-4o".to_string(),
                executable: Executable::Codex,
                supports_resume: false,
                resume_as_subcommand: false,
                base_args: vec![],
                env: HashMap::new(),
            },
        ];
        let (exe_w, prov_w) = compute_widths(&providers);
        // claude (6) > codex (5), so width is 6
        assert_eq!(exe_w, "claude".len());
        // "TestProvider" (12) > "OpenAI" (5)
        assert_eq!(prov_w, "TestProvider".len());
    }

    #[test]
    fn compute_widths_empty() {
        let (exe_w, prov_w) = compute_widths(&[]);
        assert_eq!(exe_w, 6); // "claude".len()
        assert_eq!(prov_w, 8); // "PROVIDER".len() default fallback
    }

    // ── Recent list (pure) ───────────────────────────────────────────────────

    #[test]
    fn recent_empty_push() {
        let r = update_recent_list(vec![], "a");
        assert_eq!(r, vec!["a".to_string()]);
    }

    #[test]
    fn recent_dedupe_push_to_front() {
        let r = update_recent_list(vec!["a".into(), "b".into()], "a");
        assert_eq!(r, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn recent_dedupe_in_middle() {
        let r = update_recent_list(vec!["c".into(), "a".into(), "b".into()], "a");
        assert_eq!(r, vec!["a".to_string(), "c".to_string(), "b".to_string()]);
    }

    #[test]
    fn recent_cap_at_three() {
        let r = update_recent_list(vec!["a".into(), "b".into(), "c".into()], "d");
        assert_eq!(r, vec!["d".to_string(), "a".to_string(), "b".to_string()]);
    }

    #[test]
    fn recent_cap_after_dedupe() {
        // "b" already present, push "b" again, then a new one — must stay ≤ 3
        let r = update_recent_list(vec!["b".into(), "a".into(), "c".into()], "b");
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], "b");
        assert!(r.contains(&"a".to_string()));
        assert!(r.contains(&"c".to_string()));
    }
}
