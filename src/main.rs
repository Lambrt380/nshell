mod alert;
mod commands;
mod config;
mod editor;
mod execute;
mod expand;
mod parser;
mod room;
mod shell;
mod state;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const FULL_CONFIG: &str = include_str!("../examples/config.nsh");

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match args.as_slice() {
        [] => shell::run_interactive(),
        [flag, command @ ..] if flag == "-c" => {
            if command.is_empty() {
                eprintln!("nsh: -c requires a command");
                2
            } else {
                shell::run_command(&command.join(" "))
            }
        }
        [command] if command == "init" => init_config(),
        [command] if matches!(command.as_str(), "help" | "-h" | "--help") => {
            print_help();
            0
        }
        [first, second] if first == "gen" && second == "config" => generate_config(),
        [command] if command == "about" => {
            println!(
                "noelle's shell, version {}, made by one Finnish person!",
                env!("CARGO_PKG_VERSION")
            );
            0
        }
        [command] if command == "check" => check_config(None),
        [command, path] if command == "check" => check_config(Some(Path::new(path))),
        [first, second, name] if first == "new" && second == "room" => {
            room::create_room(Path::new("."), name)
        }
        [first, second] if first == "room" => room_command(second),
        _ => {
            eprintln!(
                "usage: nsh [-c command] | about | init | gen config | check [path] | new room NAME | room <status|trust|untrust>"
            );
            2
        }
    };
    std::process::exit(code);
}

fn print_help() {
    println!(
        r#"Nshell — noelle's interactive shell

USAGE
  nsh                         Start an interactive shell
  nsh -c 'COMMAND'            Run command text and return its final status
  nsh help                    Show this guide
  nsh about                   Show version and authorship
  nsh init                    Create a small starter config
  nsh gen config              Generate the complete documented config
  nsh check [PATH]            Validate a config or .nsh-room
  nsh new room NAME           Create .nsh-room in the current directory
  nsh room status             Show the nearest room and trust state
  nsh room trust              Trust the nearest room's current contents
  nsh room untrust            Remove trust for the nearest room

SHELL LANGUAGE
  command1 | command2         Pipeline
  command > file              Replace stdout
  command >> file             Append stdout
  command 2> file             Replace stderr
  command < file              Read stdin
  command1; command2          Unconditional sequence
  command1 && command2        Run command2 after success
  command1 || command2        Run command2 after failure
  command &                   Background job
  *.rs and file-?.log         Sorted wildcard expansion
  $NAME, ${{NAME}}, $?, ~      Environment, status, and home expansion
  !notify command             Report status and elapsed time on completion

BUILT-INS
  cd, pwd, pushd, popd, dirs, export, unset, history, type,
  jobs, fg, bg, reload, room, exit

CONFIG HIGHLIGHTS
  prompt_style supports compact, two-line, and framed layouts, optional Git,
  duration, time, job, room, and failure information, and per-part colors.
  `alert_sound = "metal-gear"` enables the built-in error alert sting.
  `abbr gs > "git status"` creates a visible fish-like editor abbreviation.
  Aliases support standalone $1 through $9 and $@ argument placeholders.

Run `nsh gen config` for every setting, exact prompt example, editor shortcut,
and room feature. It asks before replacing an existing config."#
    );
}

fn generate_config() -> i32 {
    let path = config_path();
    if path.exists() {
        print!("{} already exists. Replace it? [y/N] ", path.display());
        if io::stdout().flush().is_err() {
            eprintln!("nsh: could not display replacement prompt");
            return 1;
        }
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err()
            || !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
        {
            println!("kept {}", path.display());
            return 0;
        }
    }
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        eprintln!("nsh: {error}");
        return 1;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    if let Err(error) =
        fs::write(&temporary, FULL_CONFIG).and_then(|_| fs::rename(&temporary, &path))
    {
        let _ = fs::remove_file(&temporary);
        eprintln!("nsh: could not generate {}: {error}", path.display());
        return 1;
    }
    println!("generated {}", path.display());
    0
}

fn config_path() -> PathBuf {
    if let Some(dir) = env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(dir).join("nshell/config.nsh")
    } else {
        PathBuf::from(env::var_os("HOME").unwrap_or_else(|| ".".into()))
            .join(".config/nshell/config.nsh")
    }
}

fn init_config() -> i32 {
    let path = config_path();
    if path.exists() {
        eprintln!("nsh: refusing to overwrite {}", path.display());
        return 1;
    }
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("nsh: {error}");
        return 1;
    }
    let starter = r#"# Nshell configuration
prompt_style = {
  show-name
  show-dir
  layout = "two-line"
  prompt = ">"
}

# Run `nsh gen config` for every prompt layout and configuration feature.
# alert_sound = "metal-gear"
# environment.EDITOR = "nano"
# abbr gs > "git status"
# alias ll > "ls -la"
# exec_once_opened = "echo Welcome to Nshell"
"#;
    match std::fs::write(&path, starter) {
        Ok(()) => {
            println!("created {}", path.display());
            0
        }
        Err(error) => {
            eprintln!("nsh: {error}");
            1
        }
    }
}

fn check_config(path: Option<&Path>) -> i32 {
    let path = path.map(Path::to_path_buf).unwrap_or_else(config_path);
    let checked = if path.file_name().is_some_and(|name| name == ".nsh-room") {
        room::load(&path).map(|_| ())
    } else {
        config::Config::load(&path).map(|_| ())
    };
    match checked {
        Ok(_) => {
            println!("{}: valid", path.display());
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn room_command(action: &str) -> i32 {
    let cwd = match env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("nsh: {error}");
            return 1;
        }
    };
    let Some(path) = room::discover(&cwd) else {
        eprintln!("nsh: no .nsh-room found");
        return 1;
    };
    match action {
        "status" => room::print_status(&path),
        "trust" => room::trust(&path),
        "untrust" => room::untrust(&path),
        _ => {
            eprintln!("nsh: room expects status, trust, or untrust");
            2
        }
    }
}
