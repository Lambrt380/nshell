use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::parser::{LITERAL_QUESTION, LITERAL_STAR, LITERAL_TILDE, Word};

pub fn variables(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\u{e000}' {
            output.push('$');
            index += 1;
        } else if chars[index] != '$' {
            output.push(chars[index]);
            index += 1;
        } else if chars.get(index + 1) == Some(&'{') {
            let start = index + 2;
            let mut end = start;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end == chars.len() {
                output.push('$');
                index += 1;
                continue;
            }
            let name: String = chars[start..end].iter().collect();
            output.push_str(&env::var(name).unwrap_or_default());
            index = end + 1;
        } else {
            let start = index + 1;
            let mut end = start;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            if end == start {
                output.push('$');
                index += 1;
            } else {
                let name: String = chars[start..end].iter().collect();
                output.push_str(&env::var(name).unwrap_or_default());
                index = end;
            }
        }
    }
    expand_home(output)
}

pub fn command_word(word: &Word, status: i32) -> Vec<String> {
    let expanded = variables(&word.text.replace("$?", &status.to_string()));
    if word.has_glob {
        let matches = glob_paths(&expanded);
        if !matches.is_empty() {
            return matches;
        }
    }
    vec![restore_literals(&expanded)]
}

pub fn literal_word(word: &str, status: i32) -> String {
    restore_literals(&variables(&word.replace("$?", &status.to_string())))
}

fn restore_literals(word: &str) -> String {
    word.chars()
        .map(|ch| match ch {
            LITERAL_STAR => '*',
            LITERAL_QUESTION => '?',
            LITERAL_TILDE => '~',
            _ => ch,
        })
        .collect()
}

fn glob_paths(pattern: &str) -> Vec<String> {
    let path = Path::new(pattern);
    let absolute = path.is_absolute();
    let explicit_current = pattern.starts_with("./");
    let mut candidates = vec![if absolute {
        PathBuf::from("/")
    } else {
        PathBuf::from(".")
    }];
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                for candidate in &mut candidates {
                    candidate.push("..");
                }
            }
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.contains('*') || part.contains('?') {
                    let mut next = Vec::new();
                    for directory in candidates {
                        let Ok(entries) = fs::read_dir(&directory) else {
                            continue;
                        };
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            if (!name.starts_with('.') || part.starts_with('.'))
                                && glob_matches(&part, &name)
                            {
                                next.push(entry.path());
                            }
                        }
                    }
                    candidates = next;
                } else {
                    let part = restore_literals(&part);
                    for candidate in &mut candidates {
                        candidate.push(&part);
                    }
                }
            }
            Component::Prefix(_) => return Vec::new(),
        }
    }
    let mut matches: Vec<String> = candidates
        .into_iter()
        .filter(|candidate| candidate.exists())
        .map(|candidate| {
            let display = candidate.to_string_lossy().into_owned();
            if !absolute && !explicit_current {
                display.strip_prefix("./").unwrap_or(&display).to_string()
            } else {
                display
            }
        })
        .collect();
    matches.sort();
    matches
}

fn glob_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let mut table = vec![vec![false; name.len() + 1]; pattern.len() + 1];
    table[0][0] = true;
    for index in 0..pattern.len() {
        if pattern[index] == '*' {
            table[index + 1][0] = table[index][0];
        }
        for name_index in 0..name.len() {
            table[index + 1][name_index + 1] = match pattern[index] {
                '*' => table[index][name_index + 1] || table[index + 1][name_index],
                '?' => table[index][name_index],
                literal => table[index][name_index] && literal == name[name_index],
            };
        }
    }
    table[pattern.len()][name.len()]
}

fn expand_home(word: String) -> String {
    if word == "~" {
        env::var("HOME").unwrap_or(word)
    } else if let Some(relative) = word.strip_prefix("~/") {
        env::var("HOME")
            .map(|home| format!("{home}/{relative}"))
            .unwrap_or(word)
    } else {
        word
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_braced_and_plain_variables() {
        unsafe { env::set_var("NSH_EXPAND_TEST", "works") };
        assert_eq!(
            variables("$NSH_EXPAND_TEST/${NSH_EXPAND_TEST}"),
            "works/works"
        );
    }

    #[test]
    fn preserves_quoted_literal_dollar_marker() {
        assert_eq!(variables("\u{e000}HOME"), "$HOME");
    }

    #[test]
    fn expands_home_paths() {
        unsafe { env::set_var("HOME", "/tmp/nshell-home") };
        assert_eq!(variables("~"), "/tmp/nshell-home");
        assert_eq!(variables("~/.config"), "/tmp/nshell-home/.config");
        assert_eq!(variables("file~"), "file~");
    }

    #[test]
    fn wildcard_matcher_supports_star_and_question() {
        assert!(glob_matches("*.rs", "main.rs"));
        assert!(glob_matches("file-?.log", "file-1.log"));
        assert!(!glob_matches("file-?.log", "file-12.log"));
    }

    #[test]
    fn restores_quoted_wildcard_markers_without_expanding() {
        let word = Word {
            text: format!("literal{LITERAL_STAR}.rs"),
            has_glob: false,
        };
        assert_eq!(command_word(&word, 0), ["literal*.rs"]);
    }
}
