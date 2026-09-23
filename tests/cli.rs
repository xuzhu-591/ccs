use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const CONFIG: &str = r#"[[providers]]
id = "demo"
provider = "Example"
model = "example-model"
executable = "codex"
supports_resume = true
resume_as_subcommand = true
base_args = ["--base"]
"#;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ccs-test-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(root.join("ccs")).unwrap();
        Self { root, bin }
    }

    fn config(&self, text: &str) {
        fs::write(self.root.join("ccs/config.toml"), text).unwrap();
    }

    fn agent(&self, name: &str) {
        let path = self.bin.join(name);
        fs::write(
            &path,
            "#!/bin/sh\nprintf 'ARG:%s\\n' \"$@\"\nprintf 'ENV:%s\\n' \"$DEMO_VALUE\"\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ccs"));
        command
            .args(args)
            .env_clear()
            .env("XDG_CONFIG_HOME", &self.root)
            .env("PATH", &self.bin)
            .current_dir(&self.bin);
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn list_is_a_four_column_inventory_without_agent_or_environment_checks() {
    let f = Fixture::new();
    f.config(&format!(
        "{CONFIG}\n[providers.env]\nAPI_CREDENTIAL=\"private-marker\"\n"
    ));
    let output = f.run(&["list"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert_eq!(
        text.lines()
            .next()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["ID", "PROVIDER", "MODEL", "AGENT"]
    );
    assert!(text.contains("example-model"));
    assert!(!text.contains("API_CREDENTIAL"));
    assert!(!text.contains("private-marker"));
    assert!(!text.contains("RESUME"));
    assert!(!text.contains('\x1b'));
    assert!(!f.root.join("ccs/recent").exists());
}

#[test]
fn verbose_checks_without_which_masks_every_value_and_reveals_only_on_request() {
    let f = Fixture::new();
    f.agent("codex");
    f.config(&format!(
        "{CONFIG}\n[providers.env]\nAPI_CREDENTIAL=\"private-marker\"\nREGION=\"hidden-region\"\n"
    ));
    let output = f.run(&["list", "--verbose"]);
    assert!(output.status.success(), "{}", stdout(&output));
    let text = stdout(&output);
    assert!(text.contains("1 passed, 0 failed"));
    assert!(text.contains("API_CREDENTIAL"));
    assert!(text.contains("***masked***"));
    assert!(!text.contains("private-marker"));
    assert!(!text.contains("hidden-region"));
    assert!(!text.contains("ARG:"));
    for args in [
        vec!["list", "--verbose", "--show-secrets"],
        vec!["--show-secrets", "list", "-v"],
    ] {
        let output = f.run(&args);
        assert!(output.status.success());
        assert!(stdout(&output).contains("private-marker"));
        assert!(stdout(&output).contains("hidden-region"));
    }
    assert_eq!(f.run(&["list", "--show-secrets"]).status.code(), Some(2));
}

#[test]
fn effective_path_checks_agree_with_launch_in_both_directions() {
    let f = Fixture::new();
    f.agent("codex");
    f.config(&format!(
        "{CONFIG}\n[providers.env]\nPATH=\"{}\"\n",
        f.root.join("missing").display()
    ));
    let output = f.run(&["list", "--verbose"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("FAIL"));
    assert_eq!(f.run(&["-p", "demo"]).status.code(), Some(1));

    f.config(&format!(
        "{CONFIG}\n[providers.env]\nPATH=\"{}\"\n",
        f.bin.display()
    ));
    for args in [vec!["list", "--verbose"], vec!["--profile", "demo"]] {
        let output = f
            .command(&args)
            .env("PATH", f.root.join("missing"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            stdout(&output),
            stderr(&output)
        );
    }
}

#[test]
fn path_search_handles_empty_relative_components_permissions_and_symlinks() {
    let f = Fixture::new();
    f.agent("codex");
    for path in ["", ".", "../bin", "missing:"] {
        f.config(&format!("{CONFIG}\n[providers.env]\nPATH=\"{path}\"\n"));
        assert!(f.run(&["list", "--verbose"]).status.success());
        assert!(f.run(&["-p", "demo"]).status.success());
    }
    fs::set_permissions(f.bin.join("codex"), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(f.run(&["list", "--verbose"]).status.code(), Some(1));
    fs::remove_file(f.bin.join("codex")).unwrap();
    symlink("missing", f.bin.join("codex")).unwrap();
    assert_eq!(f.run(&["list", "--verbose"]).status.code(), Some(1));
    f.agent("missing");
    assert!(f.run(&["list", "--verbose"]).status.success());
}

#[test]
fn absent_path_reports_missing_cli_without_panicking() {
    let f = Fixture::new();
    f.config(CONFIG);
    let output = f
        .command(&["list", "--verbose"])
        .env_remove("PATH")
        .output()
        .unwrap();
    // The platform default PATH excludes this fixture's codex binary.
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("effective PATH"));
}

#[test]
fn structural_errors_fail_all_loading_entry_points() {
    let f = Fixture::new();
    for (config, reason) in [
        ("providers=[]".to_string(), "No profiles"),
        (format!("{CONFIG}\n{CONFIG}"), "Duplicate profile ID"),
        (
            CONFIG.replace("id = \"demo\"", "id = \"  \""),
            "id must not be blank",
        ),
        (
            CONFIG.replace("model = \"example-model\"", "model = \"\""),
            "model must not be blank",
        ),
    ] {
        f.config(&config);
        for args in [
            vec!["list"],
            vec!["list", "--verbose"],
            vec!["-p", "demo", "-n"],
        ] {
            let output = f.run(&args);
            assert_eq!(output.status.code(), Some(1));
            assert!(stderr(&output).contains(reason), "{}", stderr(&output));
        }
    }
}

#[test]
fn verbose_reports_unknown_fields_but_allows_custom_environment_keys() {
    let f = Fixture::new();
    f.agent("codex");
    f.config(&format!(
        "typo=true\n{CONFIG}\nsuports_resume=true\n[providers.env]\nCUSTOM_SETTING=\"value\"\n"
    ));
    assert!(f.run(&["list"]).status.success());
    assert!(f.run(&["-p", "demo", "-n"]).status.success());
    let output = f.run(&["list", "--verbose"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("Unknown config field: typo"));
    assert!(text.contains("providers[0].suports_resume"));
    assert!(!text.contains("Unknown field: providers[0].CUSTOM_SETTING"));
}

#[test]
fn launch_preserves_arguments_environment_recent_and_resume() {
    let f = Fixture::new();
    f.agent("codex");
    f.config(&format!(
        "{CONFIG}\n[providers.env]\nDEMO_VALUE=\"injected\"\n"
    ));
    for selection in ["-p", "--profile"] {
        let output = f.run(&[
            selection,
            "demo",
            "-r",
            "--",
            "--provider",
            "downstream",
            "two words",
        ]);
        assert!(output.status.success());
        assert_eq!(
            stdout(&output),
            "ARG:resume\nARG:--base\nARG:--provider\nARG:downstream\nARG:two words\nENV:injected\n"
        );
        assert_eq!(
            fs::read_to_string(f.root.join("ccs/recent")).unwrap(),
            "demo\n"
        );
    }
    let output = f.run(&["--profile", "demo", "-n"]);
    assert!(output.status.success());
    assert!(stderr(&output).contains("DEMO_VALUE=***masked***"));
    assert!(!stderr(&output).contains("injected"));
}

#[test]
fn removed_commands_fail_before_loading_config_and_help_shows_new_interface() {
    let f = Fixture::new();
    for (args, hint) in [
        (vec!["--provider", "demo"], "--profile"),
        (vec!["validate"], "list --verbose"),
    ] {
        let output = f.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains(hint));
    }
    assert!(!f.root.join("ccs/config.toml").exists());
    let help = stdout(&f.run(&["--help"]));
    assert!(help.contains("--profile <PROFILE_ID>"));
    assert!(!help.contains("--provider"));
    assert!(!help.contains("validate"));
    assert!(stdout(&f.run(&["list", "--help"])).contains("--verbose"));
}

#[test]
fn first_run_creates_and_identifies_example_configuration() {
    let f = Fixture::new();
    let output = f.run(&["list"]);
    assert!(output.status.success());
    assert!(stderr(&output).contains("Example config created"));
    assert!(f.root.join("ccs/config.toml").exists());
    assert!(!stdout(&output).contains("YOUR_"));
}
