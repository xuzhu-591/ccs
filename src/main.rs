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
    about = "Claude Code / Codex 启动工具 🚀\n配置文件: ~/.ccs/config.toml",
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

#[derive(Deserialize, Clone, PartialEq)]
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

#[derive(Deserialize, Clone)]
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

#[derive(Deserialize)]
struct Config {
    providers: Vec<Provider>,
}

// ── Config ────────────────────────────────────────────────────────────────────

fn config_path() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".ccs").join("config.toml"))
        .unwrap_or_else(|_| PathBuf::from(".ccs-config.toml"))
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

    let config: Config = toml::from_str(&content).unwrap_or_else(|e| {
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
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".ccs").join("last"))
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

/// Build left-padded display strings: "executable   provider   model"
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

// ── Launch ────────────────────────────────────────────────────────────────────

fn launch(entry: &Provider, resume: bool, dry_run: bool, passthrough: &[String]) -> ! {
    let binary = entry.executable.as_str();
    let mut cmd = Command::new(binary);

    for (key, val) in &entry.env {
        cmd.env(key, val);
    }
    // `codex resume` — subcommand goes before base_args
    if resume && entry.supports_resume && entry.resume_as_subcommand {
        cmd.arg("resume");
    }
    for arg in &entry.base_args {
        cmd.arg(arg);
    }
    // `claude -r` — flag goes after base_args
    if resume {
        if !entry.supports_resume {
            eprintln!("⚠️  `{}` 不支持 resume，已忽略", entry.id);
        } else if !entry.resume_as_subcommand {
            cmd.arg("-r");
        }
    }
    for arg in passthrough {
        cmd.arg(arg);
    }

    if dry_run {
        // Print env vars
        let mut env_pairs: Vec<(&String, &String)> = entry.env.iter().collect();
        env_pairs.sort_by_key(|(k, _)| k.as_str());
        eprintln!("[dry-run] env:");
        for (k, v) in &env_pairs {
            eprintln!("  {}={}", k, v);
        }
        // Print the full command with shell-quoted args
        let args_os = cmd.get_args()
            .map(|a| {
                let s = a.to_string_lossy();
                // Quote the arg if it contains spaces or special chars
                if s.chars().any(|c| " \t\"'\\{}[]()=".contains(c)) {
                    format!("'{}'", s.replace('\'', "'\\''"))
                } else {
                    s.into_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("[dry-run] cmd:");
        eprintln!("  {} {}", binary, args_os);
        std::process::exit(0);
    }

    // exec() replaces the current process — signals and terminal are inherited
    let err = cmd.exec();
    eprintln!("❌ 无法启动 {binary}: {err}");
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

        // Print column header above the prompt
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
            eprintln!(
                "  {:<exe_w$}   {:<prov_w$}   {}",
                "TOOL", "PROVIDER", "MODEL"
            );
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
