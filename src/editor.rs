use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::commands::{BUILTINS, ExecutableIndex, SPECIAL};

pub enum ReadLine {
    Line(String),
    Eof,
    Interrupted,
}

pub fn read_line(
    prompt: &str,
    history: &[String],
    configured_commands: &[String],
    abbreviations: &HashMap<String, String>,
    executable_index: &mut ExecutableIndex,
) -> io::Result<ReadLine> {
    print!("{prompt}");
    io::stdout().flush()?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return cooked_line();
    }
    let Some(terminal) = RawTerminal::enter() else {
        return cooked_line();
    };
    let mut line: Vec<char> = Vec::new();
    let editing_prompt = editable_prompt(prompt);
    let mut cursor = 0;
    let mut history_index = history.len();
    let mut history_draft = Vec::new();
    let mut input = io::stdin().lock();
    loop {
        let mut byte = [0];
        input.read_exact(&mut byte)?;
        match byte[0] {
            b'\r' | b'\n' => {
                if expand_abbreviation(&mut line, &mut cursor, abbreviations) {
                    redraw(editing_prompt, &line, cursor, history)?;
                }
                drop(terminal);
                println!();
                return Ok(ReadLine::Line(line.iter().collect()));
            }
            3 => {
                drop(terminal);
                println!("^C");
                return Ok(ReadLine::Interrupted);
            }
            4 if line.is_empty() => {
                drop(terminal);
                println!();
                return Ok(ReadLine::Eof);
            }
            1 => cursor = 0,
            5 => cursor = line.len(),
            2 if cursor > 0 => cursor -= 1,
            6 => {
                if cursor < line.len() {
                    cursor += 1;
                } else if let Some(suggestion) = suggestion(&line, history) {
                    line = suggestion.chars().collect();
                    cursor = line.len();
                }
            }
            8 | 127 if cursor > 0 => {
                cursor -= 1;
                line.remove(cursor);
            }
            11 => line.truncate(cursor),
            21 => {
                line.drain(..cursor);
                cursor = 0;
            }
            23 => delete_previous_word(&mut line, &mut cursor),
            12 => print!("\x1b[2J\x1b[H"),
            18 => {
                if let Some(found) = reverse_history(&line, history) {
                    line = found.chars().collect();
                    cursor = line.len();
                }
            }
            b'\t' => {
                complete(
                    &mut line,
                    &mut cursor,
                    configured_commands,
                    executable_index,
                )?;
            }
            27 => {
                let sequence = escape_sequence(&mut input)?;
                if sequence == "200~" {
                    insert_bracketed_paste(&mut input, &mut line, &mut cursor)?;
                } else {
                    handle_escape(
                        &sequence,
                        &mut line,
                        &mut cursor,
                        history,
                        &mut history_index,
                        &mut history_draft,
                    );
                }
            }
            b' ' => {
                expand_abbreviation(&mut line, &mut cursor, abbreviations);
                line.insert(cursor, ' ');
                cursor += 1;
            }
            byte if byte >= 32 => {
                let ch = read_character(byte, &mut input)?;
                line.insert(cursor, ch);
                cursor += 1;
            }
            _ => {}
        }
        redraw(editing_prompt, &line, cursor, history)?;
    }
}

fn expand_abbreviation(
    line: &mut Vec<char>,
    cursor: &mut usize,
    abbreviations: &HashMap<String, String>,
) -> bool {
    let Some((start, name)) = command_token_before_cursor(line, *cursor) else {
        return false;
    };
    let Some(replacement) = abbreviations.get(&name) else {
        return false;
    };
    let replacement: Vec<char> = replacement.chars().collect();
    line.splice(start..*cursor, replacement.iter().copied());
    *cursor = start + replacement.len();
    true
}

fn command_token_before_cursor(line: &[char], cursor: usize) -> Option<(usize, String)> {
    let mut quote = None;
    let mut escaped = false;
    let mut expect_command = true;
    let mut token_start = None;
    let mut token_is_command = false;
    let mut token_is_plain = true;
    for (index, ch) in line[..cursor].iter().copied().enumerate() {
        if escaped {
            escaped = false;
            token_is_plain = false;
            if token_start.is_none() {
                token_start = Some(index.saturating_sub(1));
                token_is_command = expect_command;
            }
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            token_is_plain = false;
            if token_start.is_none() {
                token_start = Some(index);
                token_is_command = expect_command;
            }
            continue;
        }
        if let Some(mark) = quote {
            token_is_plain = false;
            if ch == mark {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            token_is_plain = false;
            if token_start.is_none() {
                token_start = Some(index);
                token_is_command = expect_command;
            }
        } else if ch.is_whitespace() {
            if token_start.take().is_some() {
                expect_command = false;
            }
            token_is_plain = true;
        } else if matches!(ch, ';' | '|' | '&') {
            token_start = None;
            token_is_plain = true;
            expect_command = true;
        } else {
            if token_start.is_none() {
                token_start = Some(index);
                token_is_command = expect_command;
            }
        }
    }
    let start = token_start?;
    if !token_is_command || !token_is_plain || quote.is_some() || escaped {
        return None;
    }
    Some((start, line[start..cursor].iter().collect()))
}

fn editable_prompt(prompt: &str) -> &str {
    prompt.rsplit_once('\n').map_or(prompt, |(_, line)| line)
}

fn cooked_line() -> io::Result<ReadLine> {
    let mut line = String::new();
    match io::stdin().read_line(&mut line)? {
        0 => Ok(ReadLine::Eof),
        _ => Ok(ReadLine::Line(
            line.trim_end_matches(['\r', '\n']).to_string(),
        )),
    }
}

fn redraw(prompt: &str, line: &[char], cursor: usize, history: &[String]) -> io::Result<()> {
    let text: String = line.iter().collect();
    let ghost = if cursor == line.len() {
        suggestion(line, history)
            .and_then(|item| item.get(text.len()..).map(str::to_string))
            .unwrap_or_default()
    } else {
        String::new()
    };
    print!("\r\x1b[2K{prompt}{text}\x1b[2m{ghost}\x1b[0m");
    let back = line.len() - cursor + ghost.chars().count();
    if back > 0 {
        print!("\x1b[{back}D");
    }
    io::stdout().flush()
}

fn complete(
    line: &mut Vec<char>,
    cursor: &mut usize,
    configured_commands: &[String],
    executable_index: &mut ExecutableIndex,
) -> io::Result<()> {
    let before: String = line[..*cursor].iter().collect();
    let start_byte = before
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    let prefix = &before[start_byte..];
    let start_char = before[..start_byte].chars().count();
    let prior_words: Vec<&str> = before[..start_byte].split_whitespace().collect();
    let command_position = prior_words.is_empty();
    let choices = if prior_words.as_slice() == ["history"] {
        ["clear", "search"]
            .into_iter()
            .filter(|choice| choice.starts_with(prefix))
            .map(str::to_string)
            .collect()
    } else if prior_words.as_slice() == ["history", "clear"] {
        ["--force"]
            .into_iter()
            .filter(|choice| choice.starts_with(prefix))
            .map(str::to_string)
            .collect()
    } else if command_position || prior_words.as_slice() == ["type"] {
        executable_choices(prefix, configured_commands, executable_index)
    } else {
        file_choices(prefix)
    };
    if choices.len() == 1 {
        let replacement: Vec<char> = choices[0].chars().collect();
        line.splice(start_char..*cursor, replacement.iter().copied());
        *cursor = start_char + replacement.len();
    } else if !choices.is_empty() {
        print!("\r\n{}\r\n", choices.join("  "));
    }
    io::stdout().flush()
}

fn suggestion(line: &[char], history: &[String]) -> Option<String> {
    let prefix: String = line.iter().collect();
    if prefix.is_empty() {
        return None;
    }
    history
        .iter()
        .rev()
        .find(|item| item.starts_with(&prefix) && **item != prefix)
        .cloned()
}

fn reverse_history(line: &[char], history: &[String]) -> Option<String> {
    let query: String = line.iter().collect();
    history
        .iter()
        .rev()
        .find(|item| item.contains(&query))
        .cloned()
}

fn escape_sequence(input: &mut impl Read) -> io::Result<String> {
    let mut sequence = String::new();
    let mut byte = [0];
    input.read_exact(&mut byte)?;
    if byte[0] != b'[' {
        return Ok(sequence);
    }
    for _ in 0..8 {
        input.read_exact(&mut byte)?;
        sequence.push(char::from(byte[0]));
        if byte[0].is_ascii_alphabetic() || byte[0] == b'~' {
            break;
        }
    }
    Ok(sequence)
}

fn handle_escape(
    sequence: &str,
    line: &mut Vec<char>,
    cursor: &mut usize,
    history: &[String],
    history_index: &mut usize,
    history_draft: &mut Vec<char>,
) {
    match sequence {
        "A" if *history_index > 0 => {
            if *history_index == history.len() {
                history_draft.clone_from(line);
            }
            *history_index -= 1;
            *line = history[*history_index].chars().collect();
            *cursor = line.len();
        }
        "B" if *history_index < history.len() => {
            *history_index += 1;
            *line = history
                .get(*history_index)
                .map_or_else(|| history_draft.clone(), |item| item.chars().collect());
            *cursor = line.len();
        }
        "D" if *cursor > 0 => *cursor -= 1,
        "C" if *cursor < line.len() => *cursor += 1,
        "C" => {
            if let Some(found) = suggestion(line, history) {
                *line = found.chars().collect();
                *cursor = line.len();
            }
        }
        "H" | "1~" | "7~" => *cursor = 0,
        "F" | "4~" | "8~" => *cursor = line.len(),
        "3~" if *cursor < line.len() => {
            line.remove(*cursor);
        }
        "1;5D" => previous_word(line, cursor),
        "1;5C" => next_word(line, cursor),
        "b" => previous_word(line, cursor),
        "f" => next_word(line, cursor),
        _ => {}
    }
}

fn insert_bracketed_paste(
    input: &mut impl Read,
    line: &mut Vec<char>,
    cursor: &mut usize,
) -> io::Result<()> {
    const END: &[u8] = b"\x1b[201~";
    let mut pasted = Vec::new();
    let mut byte = [0];
    loop {
        input.read_exact(&mut byte)?;
        pasted.push(byte[0]);
        if pasted.ends_with(END) {
            pasted.truncate(pasted.len() - END.len());
            break;
        }
    }
    let text = String::from_utf8_lossy(&pasted);
    for ch in text.chars() {
        line.insert(*cursor, if matches!(ch, '\r' | '\n') { ' ' } else { ch });
        *cursor += 1;
    }
    Ok(())
}

fn previous_word(line: &[char], cursor: &mut usize) {
    while *cursor > 0 && line[*cursor - 1].is_whitespace() {
        *cursor -= 1;
    }
    while *cursor > 0 && !line[*cursor - 1].is_whitespace() {
        *cursor -= 1;
    }
}

fn next_word(line: &[char], cursor: &mut usize) {
    while *cursor < line.len() && !line[*cursor].is_whitespace() {
        *cursor += 1;
    }
    while *cursor < line.len() && line[*cursor].is_whitespace() {
        *cursor += 1;
    }
}

fn delete_previous_word(line: &mut Vec<char>, cursor: &mut usize) {
    let end = *cursor;
    previous_word(line, cursor);
    line.drain(*cursor..end);
}

fn read_character(first: u8, input: &mut impl Read) -> io::Result<char> {
    if first.is_ascii() {
        return Ok(char::from(first));
    }
    let length = if first & 0b1111_0000 == 0b1111_0000 {
        4
    } else if first & 0b1110_0000 == 0b1110_0000 {
        3
    } else {
        2
    };
    let mut bytes = vec![first];
    for _ in 1..length {
        let mut byte = [0];
        input.read_exact(&mut byte)?;
        bytes.push(byte[0]);
    }
    Ok(std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| text.chars().next())
        .unwrap_or('�'))
}

fn executable_choices(
    prefix: &str,
    configured_commands: &[String],
    executable_index: &mut ExecutableIndex,
) -> Vec<String> {
    let mut choices = BTreeSet::new();
    for command in BUILTINS.iter().chain(SPECIAL.iter()) {
        if command.starts_with(prefix) {
            choices.insert((*command).to_string());
        }
    }
    for command in configured_commands {
        if command.starts_with(prefix) {
            choices.insert(command.clone());
        }
    }
    for name in executable_index.names() {
        if name.starts_with(prefix) {
            choices.insert(name);
        }
    }
    choices.into_iter().collect()
}

fn file_choices(prefix: &str) -> Vec<String> {
    let expanded = crate::expand::variables(prefix);
    let path = Path::new(&expanded);
    let directory = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name_prefix = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(name_prefix) {
                let mut candidate = if directory == Path::new(".") {
                    name
                } else {
                    directory.join(name).to_string_lossy().into_owned()
                };
                if prefix.starts_with("~/")
                    && let Ok(home) = env::var("HOME")
                    && let Some(relative) = candidate.strip_prefix(&home)
                {
                    candidate = format!("~{relative}");
                }
                if entry.path().is_dir() {
                    candidate.push('/');
                }
                Some(candidate)
            } else {
                None
            }
        })
        .collect()
}

struct RawTerminal {
    settings: String,
}

impl RawTerminal {
    fn enter() -> Option<Self> {
        let settings = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .output()
            .ok()?;
        if !settings.status.success() {
            return None;
        }
        let settings = String::from_utf8(settings.stdout).ok()?.trim().to_string();
        if !Command::new("stty")
            .args(["raw", "-echo"])
            .status()
            .ok()?
            .success()
        {
            return None;
        }
        print!("\x1b[?2004h");
        let _ = io::stdout().flush();
        Some(Self { settings })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        print!("\x1b[?2004l");
        let _ = io::stdout().flush();
        let _ = Command::new("stty").arg(&self.settings).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_prompt_redraws_only_the_editable_line() {
        assert_eq!(editable_prompt("socks ~\n> "), "> ");
        assert_eq!(editable_prompt("socks ~ ❯ "), "socks ~ ❯ ");
    }

    #[test]
    fn abbreviations_expand_only_in_plain_command_positions() {
        let abbreviations = HashMap::from([("gs".to_string(), "git status".to_string())]);
        for source in [
            "gs",
            "echo ok; gs",
            "false && gs",
            "echo ok | gs",
            "sleep 1 & gs",
        ] {
            let mut line: Vec<char> = source.chars().collect();
            let mut cursor = line.len();
            assert!(expand_abbreviation(&mut line, &mut cursor, &abbreviations));
            assert!(line.iter().collect::<String>().ends_with("git status"));
        }

        for source in ["echo gs", "'gs'", "\"gs\"", r"\gs"] {
            let mut line: Vec<char> = source.chars().collect();
            let mut cursor = line.len();
            assert!(!expand_abbreviation(&mut line, &mut cursor, &abbreviations));
            assert_eq!(line.iter().collect::<String>(), source);
        }
    }
}
