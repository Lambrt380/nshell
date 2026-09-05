use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

fn temporary_directory(name: &str) -> PathBuf {
    let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("nshell-{name}-{}-{number}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn nsh() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nsh"))
}

#[test]
fn about_uses_the_cargo_package_version() {
    let output = nsh().arg("about").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "noelle's shell, version {}, made by one Finnish person!\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn help_is_available_directly_and_from_inside_nshell_without_path() {
    let direct = nsh().arg("help").output().unwrap();
    assert!(direct.status.success());
    let direct_help = String::from_utf8(direct.stdout).unwrap();
    assert!(direct_help.contains("Nshell — noelle's interactive shell"));
    assert!(direct_help.contains("nsh gen config"));

    let nested = nsh()
        .env("PATH", "")
        .args(["-c", "nsh help"])
        .output()
        .unwrap();
    assert!(nested.status.success());
    assert_eq!(String::from_utf8(nested.stdout).unwrap(), direct_help);
}

#[test]
fn pipeline_redirect_and_sequence_status_work() {
    let directory = temporary_directory("pipeline");
    let output_path = directory.join("output");
    let status = nsh()
        .args([
            "-c",
            &format!(
                "printf hello | tr a-z A-Z > {}; true; false",
                output_path.display()
            ),
        ])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    assert_eq!(fs::read_to_string(output_path).unwrap(), "HELLO");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn redirected_pipeline_does_not_read_shell_input() {
    let directory = temporary_directory("redirected-pipeline-input");
    let redirected = directory.join("redirected");
    let input = directory.join("input");
    fs::write(&input, "must-not-leak\n").unwrap();
    let output = nsh()
        .stdin(fs::File::open(input).unwrap())
        .args([
            "-c",
            &format!("printf command-output > {} | cat", redirected.display()),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(redirected).unwrap(), "command-output");
    assert!(output.stdout.is_empty());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn builtins_run_in_pipeline_subprocesses() {
    let output = nsh()
        .env_remove("NSH_PIPELINE_ONLY")
        .args([
            "-c",
            "pwd | cat; export NSH_PIPELINE_ONLY=changed | cat; printf '<%s>' $NSH_PIPELINE_ONLY",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n<>", std::env::current_dir().unwrap().display())
    );
}

#[test]
fn redirects_apply_to_builtins() {
    let directory = temporary_directory("builtin-redirects");
    let pwd_output = directory.join("pwd");
    let cd_error = directory.join("cd-error");
    let output = nsh()
        .args([
            "-c",
            &format!(
                "pwd > {}; cd /definitely/missing/nshell/path 2> {}",
                pwd_output.display(),
                cd_error.display()
            ),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(pwd_output).unwrap().trim(),
        std::env::current_dir().unwrap().display().to_string()
    );
    assert!(fs::read_to_string(cd_error).unwrap().contains("nsh: cd:"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn empty_arguments_comments_and_last_status_work() {
    let output = nsh()
        .args([
            "-c",
            r#"printf "<%s>\n" ""; false; echo status=$? # ignored"#,
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "<>\nstatus=1\n");
}

#[test]
fn home_expansion_applies_to_builtins() {
    let home = temporary_directory("home");
    fs::create_dir(home.join(".config")).unwrap();
    let output = nsh()
        .env("HOME", &home)
        .args(["-c", "cd ~/.config; pwd"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        home.join(".config").display().to_string()
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn alias_trailing_arguments_keep_spaces() {
    let root = temporary_directory("alias");
    let config = root.join("config/nshell");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.nsh"),
        "alias say > \"printf \\\"[%s]\\\\n\\\"\"\n",
    )
    .unwrap();
    let output = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", "say 'two words'"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "[two words]\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn alias_keeps_call_site_redirects() {
    let root = temporary_directory("alias-redirect");
    let config = root.join("config/nshell");
    let redirected = root.join("alias-output");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.nsh"),
        "alias say > \"printf alias-output\"\n",
    )
    .unwrap();
    let output = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", &format!("say > {}", redirected.display())])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_to_string(redirected).unwrap(), "alias-output");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_unset_name_reports_an_error_instead_of_panicking() {
    let output = nsh().args(["-c", "unset ''"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not a valid variable name"));
    assert!(!stderr.contains("panicked"));
}

#[test]
fn builtins_reject_invalid_arguments() {
    for command in ["pwd extra", "jobs extra", "reload extra", "fg nope", "bg %"] {
        let output = nsh().args(["-c", command]).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "command: {command}");
        assert!(!output.stderr.is_empty(), "command: {command}");
    }
}

#[test]
fn exit_validates_arguments_and_uses_the_previous_status() {
    assert_eq!(
        nsh().args(["-c", "false; exit"]).status().unwrap().code(),
        Some(1)
    );

    let invalid = nsh().args(["-c", "exit nope"]).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("not a valid exit status")
    );

    let extra = nsh()
        .args(["-c", "exit 7 extra; printf still-running"])
        .output()
        .unwrap();
    assert!(extra.status.success());
    assert_eq!(String::from_utf8(extra.stdout).unwrap(), "still-running");
    assert!(
        String::from_utf8(extra.stderr)
            .unwrap()
            .contains("too many arguments")
    );
}

#[test]
fn interactive_exit_returns_its_requested_status() {
    let root = temporary_directory("interactive-exit");
    let mut child = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"exit 37\n").unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(37));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_alert_plays_after_an_interactive_error() {
    let root = temporary_directory("interactive-alert");
    let config = root.join("config/nshell");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.nsh"), "alert_sound = \"bell\"\n").unwrap();
    let mut child = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"false\nexit 0\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.contains(&b'\x07'));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn alias_parameters_are_safe_and_missing_arguments_run_nothing() {
    let root = temporary_directory("alias-parameters");
    let config = root.join("config/nshell");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.nsh"),
        r#"alias mkcd > {
  mkdir -p $1;
  cd $1;
}
alias all > "printf '<%s>\n' $@"
alias guarded > {
  touch marker;
  printf %s $2;
}
"#,
    )
    .unwrap();

    let output = nsh()
        .current_dir(&root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", "mkcd 'safe; not a command'; pwd; all one 'two words'"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("safe; not a command"));
    assert!(stdout.contains("<one>\n<two words>\n"));

    let output = nsh()
        .current_dir(&root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", "guarded only"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("missing argument $2")
    );
    assert!(!root.join("marker").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn type_reports_abbreviations_but_noninteractive_commands_do_not_expand_them() {
    let root = temporary_directory("abbreviation");
    let config = root.join("config/nshell");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.nsh"), "abbr gs > \"printf expanded\"\n").unwrap();

    let output = nsh()
        .env("PATH", "")
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", "type gs"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "gs is an abbreviation for `printf expanded`\n"
    );

    let output = nsh()
        .env("PATH", "")
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", "gs"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn global_environment_is_available_to_commands() {
    let root = temporary_directory("environment");
    let config = root.join("config/nshell");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.nsh"),
        "environment.NSH_CONFIG_VALUE = \"configured\"\n",
    )
    .unwrap();
    let output = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", "printf %s $NSH_CONFIG_VALUE"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "configured");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn conditionals_short_circuit_left_to_right() {
    let output = nsh()
        .args([
            "-c",
            "false && echo wrong || echo recovered; true || echo wrong; echo done",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "recovered\ndone\n"
    );
}

#[test]
fn wildcards_are_sorted_quote_aware_and_hide_dotfiles() {
    let root = temporary_directory("glob");
    fs::write(root.join("b.txt"), "").unwrap();
    fs::write(root.join("a.txt"), "").unwrap();
    fs::write(root.join(".hidden.txt"), "").unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "").unwrap();
    fs::write(root.join("src/.private.rs"), "").unwrap();
    let output = nsh()
        .current_dir(&root)
        .args([
            "-c",
            "printf '%s\\n' *.txt \"*.txt\" missing-?.txt src/*.rs src/.*.rs",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "a.txt\nb.txt\n*.txt\nmissing-?.txt\nsrc/main.rs\nsrc/.private.rs\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn directory_stack_returns_to_saved_directory() {
    let root = temporary_directory("directory-stack");
    fs::create_dir(root.join("child")).unwrap();
    let output = nsh()
        .current_dir(&root)
        .args(["-c", "pushd child; pwd; popd; pwd"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&root.join("child").display().to_string()));
    assert!(
        stdout
            .lines()
            .last()
            .unwrap()
            .ends_with(&root.display().to_string())
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn type_reports_builtins_aliases_and_executables() {
    let root = temporary_directory("type");
    let bin = root.join("bin");
    let config = root.join("config/nshell");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&config).unwrap();
    make_executable(&bin.join("demo-command"));
    fs::write(config.join("config.nsh"), "alias ll > \"ls -la\"\n").unwrap();
    let output = nsh()
        .env("PATH", &bin)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["-c", "type cd ll demo-command"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("cd is an Nshell built-in"));
    assert!(stdout.contains("ll is an alias for `ls -la`"));
    assert!(stdout.contains(&format!(
        "demo-command is {}",
        bin.join("demo-command").display()
    )));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn executable_cache_refreshes_after_path_changes() {
    let root = temporary_directory("path-cache");
    let first = root.join("first");
    let second = root.join("second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    make_executable(&first.join("first-command"));
    make_executable(&second.join("second-command"));
    let output = nsh()
        .env("PATH", &first)
        .args([
            "-c",
            &format!(
                "type first-command; export PATH={}; type second-command",
                second.display()
            ),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("first-command is"));
    assert!(stdout.contains("second-command is"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn history_search_and_forced_clear_update_state_file() {
    let root = temporary_directory("history");
    let state = root.join("state/nshell");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join("history"),
        "cargo build\nprintf hello\ncargo test\n",
    )
    .unwrap();
    let output = nsh()
        .env("XDG_STATE_HOME", root.join("state"))
        .args(["-c", "history search CARGO"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 2);
    assert!(
        nsh()
            .env("XDG_STATE_HOME", root.join("state"))
            .args(["-c", "history clear --force"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read_to_string(state.join("history")).unwrap(), "");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn history_clear_refusal_preserves_state_file() {
    let root = temporary_directory("history-refuse");
    let state = root.join("state/nshell");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("history"), "keep this\n").unwrap();
    let mut child = nsh()
        .env("XDG_STATE_HOME", root.join("state"))
        .args(["-c", "history clear"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(
        fs::read_to_string(state.join("history")).unwrap(),
        "keep this\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn command_not_found_suggests_without_executing() {
    let root = temporary_directory("suggestions");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    make_executable(&bin.join("frobnicate"));
    let output = nsh()
        .env("PATH", &bin)
        .args(["-c", "frobnciate"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("❗ nsh: frobnciate: command not found ❗"));
    assert!(stderr.contains("Did you mean frobnicate?"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invisible_command_characters_do_not_break_builtins_or_executables() {
    let root = temporary_directory("invisible-command");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    make_executable(&bin.join("xonsh"));
    let output = nsh()
        .env("PATH", &bin)
        .args(["-c", "cd\u{200b} /tmp; xon\u{feff}sh"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cd_diagnostics_explain_home_paths_and_missing_spaces() {
    let root = temporary_directory("cd-diagnostics");
    let relative = format!("nshell-downloads-{}", std::process::id());
    fs::create_dir(root.join(&relative)).unwrap();
    let output = nsh()
        .env("HOME", &root)
        .args(["-c", &format!("cd /{relative}")])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains(&format!("did you mean `cd ~/{relative}`?"))
    );

    let output = nsh().args(["-c", "cd/Downloads"]).output().unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("did you mean `cd /Downloads`?")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gen_config_creates_a_valid_complete_reference() {
    let root = temporary_directory("gen-create");
    let status = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["gen", "config"])
        .status()
        .unwrap();
    assert!(status.success());
    let path = root.join("config/nshell/config.nsh");
    let generated = fs::read_to_string(&path).unwrap();
    for documented_feature in [
        "prompt_style = {",
        "environment.EDITOR",
        "exec_once_opened",
        "alias \"system overview\"",
        "!notify",
        "nsh new room",
        "nsh about",
        "cargo build && cargo run",
        "history clear --force",
        "pushd ~/projects",
        "type cargo",
        "success-color = \"bright-green\"",
        "layout = \"framed\"",
        "show-git",
        "duration-threshold = \"2s\"",
        "abbr gs > \"git status\"",
        "mkdir -p $1",
        "git-dirty-symbol = \"*\"",
    ] {
        assert!(generated.contains(documented_feature));
    }
    let status = nsh()
        .args(["check", path.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gen_config_keeps_existing_file_without_confirmation() {
    let root = temporary_directory("gen-refuse");
    let path = root.join("config/nshell/config.nsh");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "# keep me\n").unwrap();
    let mut child = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["gen", "config"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"\n").unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(fs::read_to_string(path).unwrap(), "# keep me\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gen_config_replaces_existing_file_after_yes() {
    let root = temporary_directory("gen-confirm");
    let path = root.join("config/nshell/config.nsh");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "# replace me\n").unwrap();
    let mut child = nsh()
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["gen", "config"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    assert!(child.wait().unwrap().success());
    let generated = fs::read_to_string(path).unwrap();
    assert!(generated.contains("Nshell v1 configuration reference"));
    fs::remove_dir_all(root).unwrap();
}

fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}
