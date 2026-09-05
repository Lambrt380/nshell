use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryStyle {
    Full,
    Short,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptLayout {
    Compact,
    TwoLine,
    Framed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeFormat {
    TwentyFourHour,
    TwelveHour,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptColor {
    None,
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl PromptColor {
    pub fn ansi_code(&self) -> Option<&'static str> {
        Some(match self {
            Self::None => return None,
            Self::Default => "39",
            Self::Black => "30",
            Self::Red => "31",
            Self::Green => "32",
            Self::Yellow => "33",
            Self::Blue => "34",
            Self::Magenta => "35",
            Self::Cyan => "36",
            Self::White => "37",
            Self::BrightBlack => "90",
            Self::BrightRed => "91",
            Self::BrightGreen => "92",
            Self::BrightYellow => "93",
            Self::BrightBlue => "94",
            Self::BrightMagenta => "95",
            Self::BrightCyan => "96",
            Self::BrightWhite => "97",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub show_name: bool,
    pub show_dir: bool,
    pub show_host: bool,
    pub show_status: bool,
    pub show_git: bool,
    pub show_jobs: bool,
    pub show_duration: bool,
    pub show_time: bool,
    pub directory_style: DirectoryStyle,
    pub layout: PromptLayout,
    pub separator: String,
    pub newline: bool,
    pub marker: String,
    pub duration_threshold: Duration,
    pub time_format: TimeFormat,
    pub git_dirty_symbol: String,
    pub name_color: PromptColor,
    pub directory_color: PromptColor,
    pub host_color: PromptColor,
    pub room_color: PromptColor,
    pub status_color: PromptColor,
    pub git_color: PromptColor,
    pub jobs_color: PromptColor,
    pub duration_color: PromptColor,
    pub time_color: PromptColor,
    pub frame_color: PromptColor,
    pub success_color: PromptColor,
    pub error_color: PromptColor,
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            show_name: true,
            show_dir: true,
            show_host: false,
            show_status: false,
            show_git: false,
            show_jobs: false,
            show_duration: false,
            show_time: false,
            directory_style: DirectoryStyle::Full,
            layout: PromptLayout::Compact,
            separator: " ".to_string(),
            newline: false,
            marker: "❯".to_string(),
            duration_threshold: Duration::from_secs(2),
            time_format: TimeFormat::TwentyFourHour,
            git_dirty_symbol: "*".to_string(),
            name_color: PromptColor::Cyan,
            directory_color: PromptColor::Blue,
            host_color: PromptColor::Yellow,
            room_color: PromptColor::Magenta,
            status_color: PromptColor::Red,
            git_color: PromptColor::BrightMagenta,
            jobs_color: PromptColor::Yellow,
            duration_color: PromptColor::BrightBlack,
            time_color: PromptColor::BrightBlack,
            frame_color: PromptColor::BrightBlack,
            success_color: PromptColor::BrightGreen,
            error_color: PromptColor::BrightRed,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub prompt: Prompt,
    pub aliases: HashMap<Vec<String>, String>,
    pub abbreviations: HashMap<String, String>,
    pub startup: Vec<String>,
    pub environment: HashMap<String, String>,
}

#[derive(Debug)]
pub struct ConfigError {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.path.display(),
            self.line,
            self.column,
            self.message
        )
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let source = fs::read_to_string(path).map_err(|error| ConfigError {
            path: path.to_path_buf(),
            line: 1,
            column: 1,
            message: error.to_string(),
        })?;
        parse(&source, path)
    }
}

pub fn parse(source: &str, path: &Path) -> Result<Config, ConfigError> {
    let mut config = Config::default();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = clean_line(lines[index]);
        index += 1;
        if line.is_empty() {
            continue;
        }
        if line == "prompt_style = {" {
            config.prompt.show_name = false;
            config.prompt.show_dir = false;
            let mut saw_component = false;
            let mut saw_newline = false;
            let mut saw_layout = false;
            let mut saw_prompt = false;
            let mut closed = false;
            while index < lines.len() {
                let item = clean_line(lines[index]);
                index += 1;
                match item.as_str() {
                    "show-name" => {
                        config.prompt.show_name = true;
                        saw_component = true;
                    }
                    "show-dir" => {
                        config.prompt.show_dir = true;
                        saw_component = true;
                    }
                    "show-host" => {
                        config.prompt.show_host = true;
                        saw_component = true;
                    }
                    "show-status" => {
                        config.prompt.show_status = true;
                        saw_component = true;
                    }
                    "show-git" => {
                        config.prompt.show_git = true;
                        saw_component = true;
                    }
                    "show-jobs" => {
                        config.prompt.show_jobs = true;
                        saw_component = true;
                    }
                    "show-duration" => {
                        config.prompt.show_duration = true;
                        saw_component = true;
                    }
                    "show-time" => {
                        config.prompt.show_time = true;
                        saw_component = true;
                    }
                    "\\" => {
                        if saw_newline {
                            return error(path, index, 1, "prompt line break may appear only once");
                        }
                        if !saw_component || saw_prompt {
                            return error(
                                path,
                                index,
                                1,
                                "prompt line break must follow prompt components and precede `prompt`",
                            );
                        }
                        if saw_layout {
                            return error(
                                path,
                                index,
                                1,
                                "prompt line break cannot be combined with `layout`",
                            );
                        }
                        config.prompt.newline = true;
                        config.prompt.layout = PromptLayout::TwoLine;
                        saw_newline = true;
                    }
                    "}" => {
                        closed = true;
                        break;
                    }
                    _ if item.starts_with("prompt") => {
                        config.prompt.marker = assignment_string(&item, "prompt", path, index)?;
                        saw_prompt = true;
                    }
                    _ if setting_key(&item) == Some("layout") => {
                        if saw_newline {
                            return error(
                                path,
                                index,
                                1,
                                "`layout` cannot be combined with a prompt line break",
                            );
                        }
                        config.prompt.layout =
                            match assignment_string(&item, "layout", path, index)?.as_str() {
                                "compact" => PromptLayout::Compact,
                                "two-line" => PromptLayout::TwoLine,
                                "framed" => PromptLayout::Framed,
                                _ => {
                                    return error(
                                        path,
                                        index,
                                        1,
                                        "layout must be \"compact\", \"two-line\", or \"framed\"",
                                    );
                                }
                            };
                        saw_layout = true;
                    }
                    _ if item
                        .split_once('=')
                        .is_some_and(|(key, _)| key.trim() == "directory") =>
                    {
                        config.prompt.directory_style =
                            match assignment_string(&item, "directory", path, index)?.as_str() {
                                "full" => DirectoryStyle::Full,
                                "short" => DirectoryStyle::Short,
                                _ => {
                                    return error(
                                        path,
                                        index,
                                        1,
                                        "directory must be \"full\" or \"short\"",
                                    );
                                }
                            };
                    }
                    _ if item.starts_with("separator") => {
                        config.prompt.separator =
                            assignment_string(&item, "separator", path, index)?;
                    }
                    _ if item.starts_with("duration-threshold") => {
                        config.prompt.duration_threshold = duration_setting(
                            &assignment_string(&item, "duration-threshold", path, index)?,
                            path,
                            index,
                        )?;
                    }
                    _ if item.starts_with("time-format") => {
                        config.prompt.time_format =
                            match assignment_string(&item, "time-format", path, index)?.as_str() {
                                "24h" => TimeFormat::TwentyFourHour,
                                "12h" => TimeFormat::TwelveHour,
                                _ => {
                                    return error(
                                        path,
                                        index,
                                        1,
                                        "time-format must be \"24h\" or \"12h\"",
                                    );
                                }
                            };
                    }
                    _ if item.starts_with("git-dirty-symbol") => {
                        config.prompt.git_dirty_symbol =
                            assignment_string(&item, "git-dirty-symbol", path, index)?;
                    }
                    _ if item.starts_with("name-color") => {
                        config.prompt.name_color = color_setting(&item, "name-color", path, index)?;
                    }
                    _ if item.starts_with("directory-color") => {
                        config.prompt.directory_color =
                            color_setting(&item, "directory-color", path, index)?;
                    }
                    _ if item.starts_with("host-color") => {
                        config.prompt.host_color = color_setting(&item, "host-color", path, index)?;
                    }
                    _ if item.starts_with("room-color") => {
                        config.prompt.room_color = color_setting(&item, "room-color", path, index)?;
                    }
                    _ if item.starts_with("status-color") => {
                        config.prompt.status_color =
                            color_setting(&item, "status-color", path, index)?;
                    }
                    _ if item.starts_with("git-color") => {
                        config.prompt.git_color = color_setting(&item, "git-color", path, index)?;
                    }
                    _ if item.starts_with("jobs-color") => {
                        config.prompt.jobs_color = color_setting(&item, "jobs-color", path, index)?;
                    }
                    _ if item.starts_with("duration-color") => {
                        config.prompt.duration_color =
                            color_setting(&item, "duration-color", path, index)?;
                    }
                    _ if item.starts_with("time-color") => {
                        config.prompt.time_color = color_setting(&item, "time-color", path, index)?;
                    }
                    _ if item.starts_with("frame-color") => {
                        config.prompt.frame_color =
                            color_setting(&item, "frame-color", path, index)?;
                    }
                    _ if item.starts_with("success-color") => {
                        config.prompt.success_color =
                            color_setting(&item, "success-color", path, index)?;
                    }
                    _ if item.starts_with("error-color") => {
                        config.prompt.error_color =
                            color_setting(&item, "error-color", path, index)?;
                    }
                    "" => {}
                    _ => return error(path, index, 1, "unknown prompt setting"),
                }
            }
            if !closed {
                return error(path, index.max(1), 1, "unclosed prompt_style block");
            }
        } else if line.starts_with("exec_once_opened") {
            config
                .startup
                .push(assignment_string(&line, "exec_once_opened", path, index)?);
        } else if let Some(variable) = line.strip_prefix("environment.") {
            let Some((name, value)) = variable.split_once('=') else {
                return error(path, index, 1, "environment entry requires `=`");
            };
            let name = name.trim();
            if !valid_variable_name(name) {
                return error(path, index, 1, "invalid environment variable name");
            }
            config
                .environment
                .insert(name.to_string(), quoted(value.trim(), path, index)?);
        } else if let Some(rest) = line.strip_prefix("abbr ") {
            let Some((name, body)) = rest.split_once('>') else {
                return error(path, index, 1, "abbreviation requires `>`");
            };
            let name = abbreviation_name(name.trim(), path, index)?;
            config
                .abbreviations
                .insert(name, quoted(body.trim(), path, index)?);
        } else if let Some(rest) = line.strip_prefix("alias ") {
            let Some((name, body)) = rest.split_once('>') else {
                return error(path, index, 1, "alias requires `>`");
            };
            let name = alias_name(name.trim(), path, index)?;
            let mut body = body.trim().to_string();
            if body == "{" {
                let mut commands = Vec::new();
                let mut closed = false;
                while index < lines.len() {
                    let item = clean_line(lines[index]);
                    index += 1;
                    if item == "}" {
                        closed = true;
                        break;
                    }
                    if !item.is_empty() {
                        commands.push(item.trim_end_matches(';').to_string());
                    }
                }
                if !closed {
                    return error(path, index, 1, "unclosed alias block");
                }
                body = commands.join("; ");
            } else {
                body = quoted(&body, path, index)?;
            }
            config
                .aliases
                .insert(name.split_whitespace().map(str::to_string).collect(), body);
        } else {
            return error(path, index, 1, "unknown configuration entry");
        }
    }
    Ok(config)
}

fn setting_key(line: &str) -> Option<&str> {
    line.split_once('=').map(|(key, _)| key.trim())
}

fn duration_setting(value: &str, path: &Path, line: usize) -> Result<Duration, ConfigError> {
    let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
        (number, 1)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1_000)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60_000)
    } else {
        return error(path, line, 1, "duration must use ms, s, or m");
    };
    let number: u64 = number.parse().map_err(|_| ConfigError {
        path: path.to_path_buf(),
        line,
        column: 1,
        message: "duration must be a whole non-negative number".to_string(),
    })?;
    Ok(Duration::from_millis(number.saturating_mul(multiplier)))
}

fn valid_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn color_setting(
    line: &str,
    key: &str,
    path: &Path,
    line_number: usize,
) -> Result<PromptColor, ConfigError> {
    let value = assignment_string(line, key, path, line_number)?;
    let color = match value.as_str() {
        "none" => PromptColor::None,
        "default" => PromptColor::Default,
        "black" => PromptColor::Black,
        "red" => PromptColor::Red,
        "green" => PromptColor::Green,
        "yellow" => PromptColor::Yellow,
        "blue" => PromptColor::Blue,
        "magenta" => PromptColor::Magenta,
        "cyan" => PromptColor::Cyan,
        "white" => PromptColor::White,
        "bright-black" => PromptColor::BrightBlack,
        "bright-red" => PromptColor::BrightRed,
        "bright-green" => PromptColor::BrightGreen,
        "bright-yellow" => PromptColor::BrightYellow,
        "bright-blue" => PromptColor::BrightBlue,
        "bright-magenta" => PromptColor::BrightMagenta,
        "bright-cyan" => PromptColor::BrightCyan,
        "bright-white" => PromptColor::BrightWhite,
        _ => {
            return error(
                path,
                line_number,
                1,
                "unknown color; use none, default, a basic color, or bright-COLOR",
            );
        }
    };
    Ok(color)
}

fn assignment_string(
    line: &str,
    key: &str,
    path: &Path,
    line_number: usize,
) -> Result<String, ConfigError> {
    let Some((left, right)) = line.split_once('=') else {
        return error(path, line_number, 1, "expected `=`");
    };
    if left.trim() != key {
        return error(path, line_number, 1, "unexpected setting");
    }
    quoted(right.trim(), path, line_number)
}

pub fn quoted(value: &str, path: &Path, line: usize) -> Result<String, ConfigError> {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        Ok(value[1..value.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"))
    } else {
        error(path, line, 1, "expected a quoted string")
    }
}

pub fn alias_name(value: &str, path: &Path, line: usize) -> Result<String, ConfigError> {
    if value.starts_with('"') {
        quoted(value, path, line)
    } else if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(value.to_string())
    } else {
        error(path, line, 1, "invalid alias name")
    }
}

pub fn abbreviation_name(value: &str, path: &Path, line: usize) -> Result<String, ConfigError> {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(value.to_string())
    } else {
        error(path, line, 1, "invalid abbreviation name")
    }
}

pub fn line_without_comment(line: &str) -> String {
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && quoted {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if ch == '#' && !quoted {
            return line[..index].to_string();
        }
    }
    line.to_string()
}

fn clean_line(line: &str) -> String {
    line_without_comment(line).trim().to_string()
}

fn error<T>(path: &Path, line: usize, column: usize, message: &str) -> Result<T, ConfigError> {
    Err(ConfigError {
        path: path.to_path_buf(),
        line,
        column,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_startup_and_aliases() {
        let source = r#"
prompt_style = {
 show-name
 show-dir
 prompt = "$"
}
exec_once_opened = "one"
exec_once_opened = "two"
alias ff > "fastfetch"
alias "fun echo" > {
 echo one;
 echo two;
}
"#;
        let config = parse(source, Path::new("test.nsh")).unwrap();
        assert_eq!(config.startup, ["one", "two"]);
        assert_eq!(config.prompt.marker, "$");
        assert_eq!(
            config.aliases[&vec!["fun".into(), "echo".into()]],
            "echo one; echo two"
        );
    }

    #[test]
    fn errors_include_location() {
        let error = parse("unknown = true", Path::new("bad.nsh")).unwrap_err();
        assert_eq!(error.line, 1);
        assert!(error.to_string().contains("bad.nsh:1:1"));
    }

    #[test]
    fn hash_inside_quotes_is_not_a_comment() {
        let config = parse(
            "alias color > \"printf '#ffffff'\" # real comment",
            Path::new("colors.nsh"),
        )
        .unwrap();
        assert_eq!(config.aliases[&vec!["color".into()]], "printf '#ffffff'");
    }

    #[test]
    fn parses_extended_prompt_and_environment_settings() {
        let config = parse(
            r#"
prompt_style = {
  show-name
  show-dir
  show-host
  show-status
  show-git
  show-jobs
  show-duration
  show-time
  layout = "framed"
  directory = "short"
  separator = " :: "
  duration-threshold = "1500ms"
  time-format = "12h"
  git-dirty-symbol = "+"
  name-color = "bright-cyan"
  directory-color = "magenta"
  host-color = "yellow"
  room-color = "bright-magenta"
  status-color = "bright-red"
  git-color = "cyan"
  jobs-color = "yellow"
  duration-color = "bright-black"
  time-color = "white"
  frame-color = "blue"
  success-color = "green"
  error-color = "red"
  prompt = ">"
}
environment.EDITOR = "nano"
abbr gs > "git status"
"#,
            Path::new("extended.nsh"),
        )
        .unwrap();
        assert!(config.prompt.show_name);
        assert!(config.prompt.show_dir);
        assert!(config.prompt.show_host);
        assert!(config.prompt.show_status);
        assert!(config.prompt.show_git);
        assert!(config.prompt.show_jobs);
        assert!(config.prompt.show_duration);
        assert!(config.prompt.show_time);
        assert_eq!(config.prompt.layout, PromptLayout::Framed);
        assert_eq!(config.prompt.directory_style, DirectoryStyle::Short);
        assert_eq!(config.prompt.separator, " :: ");
        assert_eq!(
            config.prompt.duration_threshold,
            Duration::from_millis(1500)
        );
        assert_eq!(config.prompt.time_format, TimeFormat::TwelveHour);
        assert_eq!(config.prompt.git_dirty_symbol, "+");
        assert_eq!(config.prompt.marker, ">");
        assert_eq!(config.prompt.name_color, PromptColor::BrightCyan);
        assert_eq!(config.prompt.directory_color, PromptColor::Magenta);
        assert_eq!(config.prompt.room_color, PromptColor::BrightMagenta);
        assert_eq!(config.prompt.success_color, PromptColor::Green);
        assert_eq!(config.prompt.error_color, PromptColor::Red);
        assert_eq!(config.environment["EDITOR"], "nano");
        assert_eq!(config.abbreviations["gs"], "git status");
    }

    #[test]
    fn rejects_invalid_directory_style() {
        let error = parse(
            "prompt_style = {\nshow-dir\ndirectory = \"tiny\"\n}",
            Path::new("bad-dir.nsh"),
        )
        .unwrap_err();
        assert!(error.message.contains("full"));
        assert_eq!(error.line, 3);
    }

    #[test]
    fn rejects_repeated_or_misplaced_prompt_line_breaks() {
        let repeated = parse(
            "prompt_style = {\nshow-name\n\\\n\\\nprompt = \">\"\n}",
            Path::new("repeated.nsh"),
        )
        .unwrap_err();
        assert!(repeated.message.contains("only once"));

        let misplaced = parse(
            "prompt_style = {\n\\\nshow-name\nprompt = \">\"\n}",
            Path::new("misplaced.nsh"),
        )
        .unwrap_err();
        assert!(misplaced.message.contains("must follow"));
    }

    #[test]
    fn rejects_layout_line_break_conflicts_and_invalid_values() {
        let conflict = parse(
            "prompt_style = {\nshow-dir\nlayout = \"framed\"\n\\\n}",
            Path::new("conflict.nsh"),
        )
        .unwrap_err();
        assert!(conflict.message.contains("cannot be combined"));

        let layout = parse(
            "prompt_style = {\nshow-dir\nlayout = \"wide\"\n}",
            Path::new("layout.nsh"),
        )
        .unwrap_err();
        assert!(layout.message.contains("compact"));

        let duration = parse(
            "prompt_style = {\nshow-duration\nduration-threshold = \"soon\"\n}",
            Path::new("duration.nsh"),
        )
        .unwrap_err();
        assert!(duration.message.contains("ms, s, or m"));
    }

    #[test]
    fn rejects_line_break_outside_prompt_block() {
        let error = parse("\\", Path::new("outside.nsh")).unwrap_err();
        assert!(error.message.contains("unknown configuration"));
    }

    #[test]
    fn rejects_unknown_prompt_color() {
        let error = parse(
            "prompt_style = {\nshow-name\nname-color = \"orange\"\nprompt = \">\"\n}",
            Path::new("bad-color.nsh"),
        )
        .unwrap_err();
        assert_eq!(error.line, 3);
        assert!(error.message.contains("unknown color"));
    }
}
