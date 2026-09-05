use crate::config::{self, ConfigError};
use crate::state;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Room {
    pub path: PathBuf,
    pub name: String,
    pub environment: HashMap<String, String>,
    pub aliases: HashMap<Vec<String>, String>,
    pub abbreviations: HashMap<String, String>,
    pub badge: Option<String>,
    pub on_enter: Option<String>,
    pub on_leave: Option<String>,
    pub hash: String,
}

pub fn discover(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|directory| directory.join(".nsh-room"))
        .find(|path| path.is_file())
}

pub fn load(path: &Path) -> Result<Room, ConfigError> {
    let source = fs::read_to_string(path).map_err(|error| ConfigError {
        path: path.to_path_buf(),
        line: 1,
        column: 1,
        message: error.to_string(),
    })?;
    let root = path.parent().unwrap_or(Path::new("."));
    let mut room = Room {
        path: path.to_path_buf(),
        hash: state::hash(source.as_bytes()),
        ..Room::default()
    };
    for (offset, raw_line) in source.lines().enumerate() {
        let line_number = offset + 1;
        let cleaned = config::line_without_comment(raw_line);
        let line = cleaned.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("alias ") {
            let Some((name, value)) = rest.split_once('>') else {
                return room_error(path, line_number, "alias requires `>`");
            };
            room.aliases.insert(
                config::alias_name(name.trim(), path, line_number)?
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                config::quoted(value.trim(), path, line_number)?,
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix("abbr ") {
            let Some((name, value)) = rest.split_once('>') else {
                return room_error(path, line_number, "abbreviation requires `>`");
            };
            room.abbreviations.insert(
                config::abbreviation_name(name.trim(), path, line_number)?,
                config::quoted(value.trim(), path, line_number)?,
            );
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return room_error(path, line_number, "expected `=`");
        };
        let key = key.trim();
        let mut value = config::quoted(value.trim(), path, line_number)?;
        match key {
            "name" => room.name = value,
            "prompt.badge" => room.badge = Some(value),
            "on_enter" => room.on_enter = Some(value),
            "on_leave" => room.on_leave = Some(value),
            _ if key.starts_with("environment.") => {
                let variable = &key["environment.".len()..];
                if variable.is_empty()
                    || !variable
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                {
                    return room_error(path, line_number, "invalid environment variable name");
                }
                if value.starts_with("./") || value == "." {
                    value = root.join(value).to_string_lossy().into_owned();
                }
                room.environment.insert(variable.to_string(), value);
            }
            _ => return room_error(path, line_number, "unknown room entry"),
        }
    }
    if room.name.is_empty() {
        return room_error(path, 1, "room requires a name");
    }
    Ok(room)
}

pub fn create_room(directory: &Path, name: &str) -> i32 {
    let path = directory.join(".nsh-room");
    if path.exists() {
        eprintln!("nsh: refusing to overwrite {}", path.display());
        return 1;
    }
    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    let template = format!(
        r#"name = "{escaped}"

# environment.PYTHONPATH = "./src"
# alias run > "python main.py"
# abbr t > "pytest -q"
# prompt.badge = "PROJECT"
# on_enter = "echo entered [[NAME]]"
# on_leave = "echo leaving [[NAME]]"
"#
    );
    match fs::write(&path, template) {
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

fn trust_file() -> PathBuf {
    state::directory().join("trusted-rooms")
}

fn canonical_record(path: &Path, hash: &str) -> io::Result<String> {
    Ok(format!("{}\t{hash}", fs::canonicalize(path)?.display()))
}

fn trust_records() -> Vec<String> {
    fs::read_to_string(trust_file())
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

pub fn is_trusted(room: &Room) -> bool {
    canonical_record(&room.path, &room.hash).is_ok_and(|record| trust_records().contains(&record))
}

pub fn request_trust(room: &Room) -> bool {
    eprint!("Trust room {}? [y/N] ", room.path.display());
    let _ = io::stderr().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err()
        || !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    {
        return false;
    }
    trust(&room.path) == 0
}

pub fn trust(path: &Path) -> i32 {
    let room = match load(path) {
        Ok(room) => room,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let record = match canonical_record(path, &room.hash) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("nsh: {error}");
            return 1;
        }
    };
    let mut records = trust_records();
    records.retain(|entry| {
        !entry.starts_with(&format!("{}\t", fs::canonicalize(path).unwrap().display()))
    });
    records.push(record);
    if let Err(error) = state::ensure_directory()
        .and_then(|_| fs::write(trust_file(), format!("{}\n", records.join("\n"))))
    {
        eprintln!("nsh: {error}");
        return 1;
    }
    println!("trusted {}", path.display());
    0
}

pub fn untrust(path: &Path) -> i32 {
    let canonical = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("nsh: {error}");
            return 1;
        }
    };
    let prefix = format!("{}\t", canonical.display());
    let mut records = trust_records();
    records.retain(|entry| !entry.starts_with(&prefix));
    if let Err(error) = state::ensure_directory()
        .and_then(|_| fs::write(trust_file(), format!("{}\n", records.join("\n"))))
    {
        eprintln!("nsh: {error}");
        return 1;
    }
    println!("untrusted {}", path.display());
    0
}

pub fn print_status(path: &Path) -> i32 {
    match load(path) {
        Ok(room) => {
            println!(
                "{}: {} ({})",
                path.display(),
                room.name,
                if is_trusted(&room) {
                    "trusted"
                } else {
                    "untrusted"
                }
            );
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn room_error<T>(path: &Path, line: usize, message: &str) -> Result<T, ConfigError> {
    Err(ConfigError {
        path: path.to_path_buf(),
        line,
        column: 1,
        message: message.to_string(),
    })
}

pub fn lifecycle(command: &str, name: &str) -> String {
    command.replace("[[NAME]]", name)
}

pub struct ActiveRoom {
    pub room: Room,
    previous: HashMap<String, Option<OsString>>,
}

impl ActiveRoom {
    pub fn activate(room: Room) -> Self {
        let mut previous = HashMap::new();
        for (key, value) in &room.environment {
            previous.insert(key.clone(), env::var_os(key));
            unsafe { env::set_var(key, value) };
        }
        Self { room, previous }
    }

    pub fn restore(&self) {
        for (key, value) in &self.previous {
            if let Some(value) = value {
                unsafe { env::set_var(key, value) };
            } else {
                unsafe { env::remove_var(key) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn nearest_ancestor_wins() {
        let root = std::env::temp_dir().join(format!("nsh-room-test-{}", std::process::id()));
        let nested = root.join("a/b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join(".nsh-room"), "name = \"root\"").unwrap();
        fs::write(root.join("a/.nsh-room"), "name = \"near\"").unwrap();
        assert_eq!(discover(&nested).unwrap(), root.join("a/.nsh-room"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn substitutes_lifecycle_name() {
        assert_eq!(lifecycle("echo [[NAME]]", "demo"), "echo demo");
    }

    #[test]
    fn room_restores_non_utf8_environment_values() {
        let key = format!("NSH_ROOM_NON_UTF8_{}", std::process::id());
        let original = OsString::from_vec(vec![b'v', 0xff]);
        unsafe { env::set_var(&key, &original) };
        let active = ActiveRoom::activate(Room {
            environment: HashMap::from([(key.clone(), "temporary".to_string())]),
            ..Room::default()
        });
        assert_eq!(env::var(&key).unwrap(), "temporary");
        active.restore();
        assert_eq!(env::var_os(&key).unwrap(), original);
        unsafe { env::remove_var(key) };
    }

    #[test]
    fn parses_documented_room_settings_and_relative_paths() {
        let root = std::env::temp_dir().join(format!("nsh-room-parse-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(".nsh-room");
        fs::write(
            &path,
            r#"name = "python project"
environment.PYTHONPATH = "./src"
environment.DEBUG = "1"
alias run > "python main.py"
abbr t > "pytest -q"
prompt.badge = "PROJECT"
on_enter = "echo entered [[NAME]]"
on_leave = "echo leaving [[NAME]]"
"#,
        )
        .unwrap();
        let parsed = load(&path).unwrap();
        assert_eq!(parsed.name, "python project");
        assert_eq!(
            parsed.environment["PYTHONPATH"],
            root.join("./src").to_string_lossy()
        );
        assert_eq!(parsed.aliases[&vec!["run".into()]], "python main.py");
        assert_eq!(parsed.abbreviations["t"], "pytest -q");
        assert_eq!(parsed.badge.as_deref(), Some("PROJECT"));
        fs::remove_dir_all(root).unwrap();
    }
}
