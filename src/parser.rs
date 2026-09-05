use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectKind {
    Input,
    Output,
    Append,
    Error,
    ErrorAppend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub kind: RedirectKind,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Command {
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub has_glob: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    Always,
    OnSuccess,
    OnFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<Command>,
    pub background: bool,
    pub source: String,
    pub condition: Condition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub column: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "column {}: {}", self.column, self.message)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
}

const LITERAL_DOLLAR: char = '\u{e000}';
pub const LITERAL_STAR: char = '\u{e001}';
pub const LITERAL_QUESTION: char = '\u{e002}';
pub const LITERAL_TILDE: char = '\u{e003}';

pub fn parse_line(input: &str) -> Result<Vec<Pipeline>, ParseError> {
    let input_chars: Vec<char> = input.chars().collect();
    let tokens = tokenize(input)?;
    let mut pipelines = Vec::new();
    let mut commands = vec![Command::default()];
    let mut index = 0;
    let mut start = 0;
    let mut condition = Condition::Always;
    let mut requires_command = false;
    while index < tokens.len() {
        let (token, position) = &tokens[index];
        match token.as_str() {
            "|" => {
                if commands
                    .last()
                    .is_none_or(|command| command.words.is_empty())
                {
                    return err(*position + 1, "expected command before pipe");
                }
                commands.push(Command::default());
            }
            ";" | "&" | "&&" | "||" => {
                if commands
                    .last()
                    .is_none_or(|command| command.words.is_empty())
                {
                    return err(*position + 1, "expected command");
                }
                pipelines.push(Pipeline {
                    commands: std::mem::take(&mut commands),
                    background: token == "&",
                    source: input_chars[start..*position]
                        .iter()
                        .collect::<String>()
                        .trim()
                        .to_string(),
                    condition,
                });
                commands.push(Command::default());
                start = *position + token.chars().count();
                condition = match token.as_str() {
                    "&&" => Condition::OnSuccess,
                    "||" => Condition::OnFailure,
                    _ => Condition::Always,
                };
                requires_command = matches!(token.as_str(), "&&" | "||");
            }
            "<" | ">" | ">>" | "2>" | "2>>" => {
                index += 1;
                let Some((path, _)) = tokens.get(index) else {
                    return err(*position + 1, "redirect requires a path");
                };
                if matches!(
                    path.as_str(),
                    "|" | ";" | "&" | "&&" | "||" | "<" | ">" | ">>" | "2>" | "2>>"
                ) {
                    return err(*position + 1, "redirect requires a path");
                }
                let kind = match token.as_str() {
                    "<" => RedirectKind::Input,
                    ">" => RedirectKind::Output,
                    ">>" => RedirectKind::Append,
                    "2>" => RedirectKind::Error,
                    _ => RedirectKind::ErrorAppend,
                };
                commands.last_mut().unwrap().redirects.push(Redirect {
                    kind,
                    path: path.clone(),
                });
            }
            _ => {
                let has_glob = token.contains('*') || token.contains('?');
                commands.last_mut().unwrap().words.push(Word {
                    text: token.clone(),
                    has_glob,
                });
                requires_command = false;
            }
        }
        index += 1;
    }
    if commands.iter().any(|command| !command.words.is_empty()) {
        if commands
            .last()
            .is_none_or(|command| command.words.is_empty())
        {
            return err(input.len(), "expected command after pipe");
        }
        pipelines.push(Pipeline {
            commands,
            background: false,
            source: input_chars[start..]
                .iter()
                .collect::<String>()
                .trim()
                .to_string(),
            condition,
        });
    } else if requires_command {
        return err(
            input_chars.len(),
            "expected command after conditional operator",
        );
    }
    Ok(pipelines)
}

fn tokenize(input: &str) -> Result<Vec<(String, usize)>, ParseError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut word_started = false;
    let mut word_start = 0;
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            if !word_started {
                word_start = index.saturating_sub(1);
                word_started = true;
            }
            word.push(match ch {
                '$' => LITERAL_DOLLAR,
                '*' => LITERAL_STAR,
                '?' => LITERAL_QUESTION,
                '~' => LITERAL_TILDE,
                _ => ch,
            });
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' && quote != Quote::Single {
            let next = chars.get(index + 1).copied();
            if quote == Quote::Double && !matches!(next, Some('"' | '$' | '\\' | '\n')) {
                if !word_started {
                    word_start = index;
                    word_started = true;
                }
                word.push('\\');
                index += 1;
                continue;
            }
            escaped = true;
            index += 1;
            continue;
        }
        match quote {
            Quote::Single if ch == '\'' => quote = Quote::None,
            Quote::Double if ch == '"' => quote = Quote::None,
            Quote::None if ch == '\'' => {
                if !word_started {
                    word_start = index;
                    word_started = true;
                }
                quote = Quote::Single;
            }
            Quote::None if ch == '"' => {
                if !word_started {
                    word_start = index;
                    word_started = true;
                }
                quote = Quote::Double;
            }
            Quote::None if ch == '#' && !word_started => break,
            Quote::None if ch.is_whitespace() => {
                push_word(&mut tokens, &mut word, &mut word_started, word_start);
            }
            Quote::None if matches!(ch, '|' | ';' | '&' | '<' | '>') => {
                push_word(&mut tokens, &mut word, &mut word_started, word_start);
                let position = index;
                let mut op = ch.to_string();
                if matches!(ch, '>' | '|' | '&') && chars.get(index + 1) == Some(&ch) {
                    op.push('>');
                    if ch != '>' {
                        op.pop();
                        op.push(ch);
                    }
                    index += 1;
                }
                tokens.push((op, position));
            }
            Quote::None if ch == '2' && chars.get(index + 1) == Some(&'>') && word.is_empty() => {
                let mut op = "2>".to_string();
                index += 1;
                if chars.get(index + 1) == Some(&'>') {
                    op.push('>');
                    index += 1;
                }
                let position = index.saturating_sub(op.len() - 1);
                tokens.push((op, position));
            }
            Quote::Single if ch == '$' => word.push(LITERAL_DOLLAR),
            Quote::Single | Quote::Double if ch == '*' => word.push(LITERAL_STAR),
            Quote::Single | Quote::Double if ch == '?' => word.push(LITERAL_QUESTION),
            Quote::Single | Quote::Double if ch == '~' => word.push(LITERAL_TILDE),
            _ => {
                if !word_started {
                    word_start = index;
                    word_started = true;
                }
                word.push(ch);
            }
        }
        index += 1;
    }
    if escaped {
        return err(chars.len(), "trailing escape");
    }
    if quote != Quote::None {
        return err(chars.len(), "unterminated quote");
    }
    push_word(&mut tokens, &mut word, &mut word_started, word_start);
    Ok(tokens)
}

fn push_word(
    tokens: &mut Vec<(String, usize)>,
    word: &mut String,
    word_started: &mut bool,
    start: usize,
) {
    if *word_started {
        tokens.push((std::mem::take(word), start));
        *word_started = false;
    }
}

fn err<T>(column: usize, message: &str) -> Result<T, ParseError> {
    Err(ParseError {
        column,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quotes_pipes_redirects_and_sequence() {
        let parsed = parse_line(r#"echo "hello world" | cat >> out; false &"#).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0].commands[0]
                .words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["echo", "hello world"]
        );
        assert_eq!(
            parsed[0].commands[1].redirects[0].kind,
            RedirectKind::Append
        );
        assert!(parsed[1].background);
    }

    #[test]
    fn reports_unterminated_quote() {
        assert!(
            parse_line("echo 'no")
                .unwrap_err()
                .message
                .contains("unterminated")
        );
    }

    #[test]
    fn preserves_empty_quoted_arguments_and_unicode_sources() {
        let parsed = parse_line("printf '%s' \"\"; echo привет").unwrap();
        assert_eq!(
            parsed[0].commands[0]
                .words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["printf", "%s", ""]
        );
        assert_eq!(parsed[0].source, "printf '%s' \"\"");
        assert_eq!(parsed[1].source, "echo привет");
    }

    #[test]
    fn preserves_non_special_backslashes_inside_double_quotes() {
        let parsed = parse_line(r#"printf "<%s>\n" value"#).unwrap();
        assert_eq!(parsed[0].commands[0].words[1].text, r#"<%s>\n"#);
    }

    #[test]
    fn parses_conditional_pipeline_chains() {
        let parsed = parse_line("false && echo no || printf yes; echo done").unwrap();
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].condition, Condition::Always);
        assert_eq!(parsed[1].condition, Condition::OnSuccess);
        assert_eq!(parsed[2].condition, Condition::OnFailure);
        assert_eq!(parsed[3].condition, Condition::Always);
    }

    #[test]
    fn rejects_missing_conditional_commands() {
        assert!(
            parse_line("echo yes &&")
                .unwrap_err()
                .message
                .contains("expected command")
        );
        assert!(parse_line("|| echo no").is_err());
    }

    #[test]
    fn conditional_operators_do_not_leak_into_pipeline_sources() {
        let parsed = parse_line("false && echo no || echo yes").unwrap();
        assert_eq!(parsed[0].source, "false");
        assert_eq!(parsed[1].source, "echo no");
        assert_eq!(parsed[2].source, "echo yes");
    }

    #[test]
    fn conditional_operator_cannot_be_a_redirect_path() {
        assert!(
            parse_line("echo > && echo no")
                .unwrap_err()
                .message
                .contains("redirect requires a path")
        );
    }

    #[test]
    fn marks_only_unquoted_wildcards_for_expansion() {
        let parsed = parse_line(r#"echo *.rs "*.txt" \?.md"#).unwrap();
        let words = &parsed[0].commands[0].words;
        assert!(words[1].has_glob);
        assert!(!words[2].has_glob);
        assert!(!words[3].has_glob);
    }
}
