use super::{Config, Executable, Profile, mask_value};
use crossterm::style::{Color, Stylize};
use std::ffi::{CString, OsString};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

fn agent_name(executable: &Executable) -> &'static str {
    match executable {
        Executable::Claude => "Claude Code",
        Executable::Codex => "Codex",
    }
}

// Keep configuration values from adding terminal control sequences or extra rows.
fn display_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
}

fn paint(value: &str, color: Color, bold: bool, colors: bool) -> String {
    if !colors {
        return value.to_string();
    }
    if bold {
        value.with(color).bold().to_string()
    } else {
        value.with(color).to_string()
    }
}

fn profile_row(profile: &Profile) -> [String; 4] {
    [
        display_text(&profile.id),
        display_text(&profile.provider),
        display_text(&profile.model),
        agent_name(&profile.executable).to_string(),
    ]
}

fn write_table(
    out: &mut impl Write,
    profiles: &[Profile],
    terminal_width: Option<usize>,
    colors: bool,
) -> io::Result<()> {
    let headers = ["ID", "PROVIDER", "MODEL", "AGENT"];
    let rows: Vec<_> = profiles.iter().map(profile_row).collect();
    let widths: [usize; 4] = std::array::from_fn(|column| {
        rows.iter()
            .map(|row| row[column].width())
            .chain([headers[column].width()])
            .max()
            .unwrap_or(0)
    });
    let total_width = widths.iter().sum::<usize>() + 9;
    if terminal_width.is_some_and(|width| total_width > width.saturating_sub(1)) {
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                writeln!(out)?;
            }
            writeln!(out, "{}", paint(&row[0], Color::Cyan, true, colors))?;
            for (label, value) in ["Provider", "Model", "Agent"].iter().zip(&row[1..]) {
                writeln!(out, "  {label:<8}  {value}")?;
            }
        }
        return Ok(());
    }

    for (column, header) in headers.iter().enumerate() {
        write!(out, "{}", paint(header, Color::DarkGrey, true, colors))?;
        if column < 3 {
            write!(out, "{}", " ".repeat(widths[column] - header.width() + 3))?;
        }
    }
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        paint(&"─".repeat(total_width), Color::DarkGrey, false, colors)
    )?;
    for row in rows {
        for (column, value) in row.iter().enumerate() {
            let color = if column == 0 {
                Color::Cyan
            } else {
                Color::White
            };
            write!(
                out,
                "{}",
                paint(value, color, column == 0, colors && column == 0)
            )?;
            if column < 3 {
                write!(out, "{}", " ".repeat(widths[column] - value.width() + 3))?;
            }
        }
        writeln!(out)?;
    }
    Ok(())
}

fn default_search_path() -> OsString {
    // POSIX provides the default search path used when PATH is absent.
    // SAFETY: confstr accepts a null buffer when requesting its required size.
    let size = unsafe { libc::confstr(libc::_CS_PATH, std::ptr::null_mut(), 0) };
    if size == 0 {
        return OsString::from("/bin:/usr/bin");
    }
    let mut buffer = vec![0_u8; size];
    // SAFETY: buffer is writable for size bytes and confstr includes a trailing NUL.
    unsafe { libc::confstr(libc::_CS_PATH, buffer.as_mut_ptr().cast(), size) };
    buffer.truncate(size - 1);
    std::ffi::OsStr::from_bytes(&buffer).to_owned()
}

fn find_executable(profile: &Profile) -> Result<PathBuf, String> {
    let search_path = profile
        .env
        .get("PATH")
        .map(OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_else(default_search_path);
    let cwd = std::env::current_dir()
        .map_err(|error| format!("Cannot read working directory: {error}"))?;
    let executable = profile.executable.as_str();
    for directory in std::env::split_paths(&search_path) {
        let candidate = cwd.join(directory).join(executable);
        if !candidate.is_file() {
            continue;
        }
        let Ok(path) = CString::new(candidate.as_os_str().as_bytes()) else {
            continue;
        };
        // SAFETY: path is a valid, NUL-terminated C string for this call.
        if unsafe { libc::access(path.as_ptr(), libc::X_OK) } == 0 {
            return Ok(candidate);
        }
    }
    Err(format!(
        "{executable} not found or not executable in effective PATH"
    ))
}

pub(super) fn write_list(
    out: &mut impl Write,
    config: &Config,
    verbose: bool,
    show_secrets: bool,
    terminal_width: Option<usize>,
    colors: bool,
) -> io::Result<bool> {
    write_table(out, &config.profiles, terminal_width, colors)?;
    if !verbose {
        return Ok(true);
    }

    let mut config_fields: Vec<_> = config.unknown.keys().collect();
    config_fields.sort();
    let config_valid = config_fields.is_empty();
    for field in config_fields {
        writeln!(
            out,
            "\n{} — Unknown config field: {}",
            paint("FAIL", Color::Red, true, colors),
            display_text(field)
        )?;
    }
    let mut passed = 0;
    for (index, profile) in config.profiles.iter().enumerate() {
        let executable = find_executable(profile);
        let mut fields: Vec<_> = profile.unknown.keys().collect();
        fields.sort();
        let valid = executable.is_ok() && fields.is_empty();
        if valid {
            passed += 1;
        }
        let (status, color) = if valid {
            ("OK", Color::Green)
        } else {
            ("FAIL", Color::Red)
        };
        writeln!(
            out,
            "\n{}  {}",
            paint(&display_text(&profile.id), Color::Cyan, true, colors),
            paint(status, color, true, colors)
        )?;
        match executable {
            Ok(path) => writeln!(
                out,
                "  Agent  {}  {}",
                profile.executable.as_str(),
                display_text(&path.to_string_lossy())
            )?,
            Err(reason) => writeln!(out, "  Check  {reason}")?,
        }
        for field in fields {
            writeln!(
                out,
                "  Check  Unknown field: profiles[{index}].{}",
                display_text(field)
            )?;
        }
        let mut env: Vec<_> = profile.env.iter().collect();
        env.sort_by(|a, b| a.0.cmp(b.0));
        let key_width = env
            .iter()
            .map(|(key, _)| display_text(key).width())
            .max()
            .unwrap_or(0);
        if env.is_empty() {
            writeln!(out, "  Env    N/A")?;
        }
        for (index, (key, value)) in env.iter().enumerate() {
            let label = if index == 0 { "Env" } else { "" };
            let key_display = display_text(key);
            let value_display = if show_secrets {
                display_text(value)
            } else {
                mask_value(key, value)
            };
            writeln!(
                out,
                "  {label:<5}  {}{} = {}",
                key_display,
                " ".repeat(key_width - key_display.width()),
                value_display
            )?;
        }
    }
    writeln!(
        out,
        "\nLocal checks: {passed} passed, {} failed{}.",
        config.profiles.len() - passed,
        if config_valid {
            ""
        } else {
            "; invalid config fields"
        }
    )?;
    writeln!(out, "Credentials and service connectivity are not checked.")?;
    Ok(config_valid && passed == config.profiles.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        super::super::parse_config(
            r#"
[[profiles]]
id = "cn"
provider = "网易"
model = "模型一"
executable = "claude"
[profiles.env]
API_CREDENTIAL = "test-sensitive-value"
[[profiles]]
id = "en"
provider = "OpenAI"
model = "m"
executable = "codex"
"#,
        )
        .unwrap()
    }

    #[test]
    fn table_aligns_display_cells_and_omits_environment() {
        let mut output = Vec::new();
        write_list(&mut output, &config(), false, false, None, false).unwrap();
        let output = String::from_utf8(output).unwrap();
        let lines: Vec<_> = output.lines().collect();
        let cn_column = lines[2].find("模型一").unwrap();
        let en_column = lines[3].find("m ").unwrap();
        assert_eq!(lines[2][..cn_column].width(), lines[3][..en_column].width());
        assert!(lines[0].ends_with("AGENT"));
        assert!(!output.contains("API_CREDENTIAL"));
        assert!(!output.contains("RESUME"));
        assert!(!output.contains('\x1b'));
    }

    #[test]
    fn narrow_layout_keeps_complete_values() {
        let mut output = Vec::new();
        write_list(&mut output, &config(), false, false, Some(25), false).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Provider  网易"));
        assert!(output.contains("模型一"));
        assert!(output.contains("Claude Code"));
    }

    #[test]
    fn display_escapes_control_characters() {
        assert_eq!(display_text("中\n\x1b[31m\t"), "中\\n\\u{1b}[31m\\t");
    }
}
