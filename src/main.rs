use clap::Parser;
use dialoguer::{Select, theme::ColorfulTheme};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_CONFIG: &str = include_str!("default_providers.toml");

#[derive(Parser)]
#[command(
    name = "ccs",
    about = "Claude Code / Codex 启动工具 🚀\n配置文件: ~/.config/ccs/config.toml",
    version
)]
struct Args {
    /// 继续上次会话（传递 -r 给 claude）
    #[arg(short = 'r', long = "resume")]
    resume: bool,

    /// 直接指定提供商 ID，跳过交互式选择
    #[arg(short = 'p', long = "provider", value_name = "ID")]
    provider: Option<String>,

    /// 打印将要执行的命令，但不实际启动（调试用）
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,

    /// 透传给 claude/codex 的额外参数
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    passthrough: Vec<String>,
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
            eprintln!("⚠️  无法写入默认配置 {}: {e}", path.display());
        } else {
            eprintln!("📝 已生成默认配置: {}\n", path.display());
        }
    }

    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("❌ 无法读取配置文件 {}: {e}", path.display());
        std::process::exit(1);
    });

    let config: Config = parse_config(&content).unwrap_or_else(|e| {
        eprintln!("❌ 配置解析失败 ({}): {e}", path.display());
        std::process::exit(1);
    });

    if config.providers.is_empty() {
        eprintln!("❌ 配置文件中没有任何提供商定义");
        std::process::exit(1);
    }

    config.providers
}

// ── Last-used persistence ─────────────────────────────────────────────────────

fn last_id_path() -> Option<PathBuf> {
    Some(ccs_dir().join("last"))
}

fn read_last_id() -> Option<String> {
    let path = last_id_path()?;
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn save_last_id(id: &str) {
    if let Some(path) = last_id_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, id);
    }
}

// ── Display formatting ────────────────────────────────────────────────────────

fn build_menu_items(providers: &[Provider]) -> Vec<String> {
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

fn launch(entry: &Provider, resume: bool, dry_run: bool, passthrough: &[String]) -> ! {
    if resume && !entry.supports_resume {
        eprintln!("⚠️  `{}` 不支持 resume，已忽略", entry.id);
    }

    let cmd_info = build_launch_cmd(entry, resume, passthrough);

    if dry_run {
        eprintln!("[dry-run] env:");
        for (k, v) in &cmd_info.env {
            eprintln!("  {}={}", k, v);
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
    eprintln!("❌ 无法启动 {}: {err}", cmd_info.binary);
    std::process::exit(1);
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();
    let providers = load_providers();

    let entry = if let Some(ref id) = args.provider {
        match providers.iter().find(|p| p.id == *id) {
            Some(p) => p.clone(),
            None => {
                eprintln!("❌ 未知 ID: {id}");
                let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
                eprintln!("可用 ID: {}", ids.join(", "));
                std::process::exit(1);
            }
        }
    } else {
        let last = read_last_id();
        let default_idx = last
            .as_deref()
            .and_then(|id| providers.iter().position(|p| p.id == id))
            .unwrap_or(0);

        let items = build_menu_items(&providers);

        if !providers.is_empty() {
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
            eprintln!("  {:<exe_w$}   {:<prov_w$}   MODEL", "TOOL", "PROVIDER");
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("选择")
            .items(&items)
            .default(default_idx)
            .interact_opt()
            .unwrap_or(None);

        match selection {
            Some(idx) => providers[idx].clone(),
            None => {
                eprintln!("已取消");
                std::process::exit(0);
            }
        }
    };

    save_last_id(&entry.id);
    if !args.dry_run {
        eprintln!(
            "🚀 {} / {} / {}",
            entry.executable.as_str(),
            entry.provider,
            entry.model
        );
    }
    launch(&entry, args.resume, args.dry_run, &args.passthrough);
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
}
