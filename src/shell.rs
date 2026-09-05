use crate::commands::{self, ExecutableIndex};
use crate::config::{Config, DirectoryStyle, Prompt, PromptColor, PromptLayout, TimeFormat};
use crate::editor::{self, ReadLine};
use crate::execute::{self, Executor};
use crate::parser::{self, Condition, Pipeline, Redirect, Word};
use crate::room::{self, ActiveRoom};
use crate::state;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

pub struct Shell {
    config_path: PathBuf,
    config: Config,
    config_modified: Option<SystemTime>,
    executor: Executor,
    active_room: Option<ActiveRoom>,
    declined_room: Option<(PathBuf, String)>,
    inherited_environment: HashMap<String, Option<OsString>>,
    global_environment: HashMap<String, String>,
    last_external: Option<String>,
    history: Vec<String>,
    directory_stack: Vec<PathBuf>,
    executable_index: ExecutableIndex,
    last_status: i32,
    last_duration: Option<Duration>,
    git_cache: Option<(PathBuf, Option<String>)>,
    startup_ran: bool,
    exit_requested: bool,
}

pub fn run_interactive() -> i32 {
    let mut shell = Shell::new(true);
    shell.run_startup();
    loop {
        shell.executor.reap();
        shell.reload_config();
        shell.refresh_room();
        let prompt = shell.prompt();
        let configured_commands = shell.completion_commands();
        let abbreviations = shell.effective_abbreviations();
        let history = &shell.history;
        let executable_index = &mut shell.executable_index;
        match editor::read_line(
            &prompt,
            history,
            &configured_commands,
            &abbreviations,
            executable_index,
        ) {
            Ok(ReadLine::Eof) => break,
            Ok(ReadLine::Interrupted) => continue,
            Ok(ReadLine::Line(line)) => {
                if line.is_empty() {
                    continue;
                }
                shell.record_history(&line);
                let started = Instant::now();
                shell.execute_text(&line, 0);
                shell.last_duration = Some(started.elapsed());
                shell.git_cache = None;
                if shell.exit_requested {
                    break;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                println!();
            }
            Err(error) => {
                eprintln!("nsh: {error}");
                return 1;
            }
        }
    }
    let status = if shell.exit_requested {
        shell.last_status
    } else {
        0
    };
    shell.leave_room();
    status
}

pub fn run_command(command: &str) -> i32 {
    Shell::new(false).execute_text(command, 0)
}

impl Shell {
    fn new(interactive: bool) -> Self {
        let config_path = global_config_path();
        let config = Config::load(&config_path).unwrap_or_else(|error| {
            eprintln!("{error}");
            Config::default()
        });
        let config_modified = modified(&config_path);
        let history = load_history();
        let mut shell = Self {
            config_path,
            config,
            config_modified,
            executor: Executor::new(interactive),
            active_room: None,
            declined_room: None,
            inherited_environment: HashMap::new(),
            global_environment: HashMap::new(),
            last_external: None,
            history,
            directory_stack: Vec::new(),
            executable_index: ExecutableIndex::default(),
            last_status: 0,
            last_duration: None,
            git_cache: None,
            startup_ran: false,
            exit_requested: false,
        };
        shell.apply_global_environment();
        shell
    }

    fn execute_text(&mut self, text: &str, alias_depth: usize) -> i32 {
        self.execute_text_with_suffix(text, alias_depth, Vec::new(), false)
    }

    fn execute_text_with_suffix(
        &mut self,
        text: &str,
        alias_depth: usize,
        redirects: Vec<Redirect>,
        background: bool,
    ) -> i32 {
        if alias_depth > 32 {
            eprintln!("nsh: recursive alias expansion");
            self.last_status = 2;
            return 2;
        }
        let (notify, text) = text
            .strip_prefix("!notify ")
            .map_or((false, text), |command| (true, command));
        let mut parsed = match parser::parse_line(text) {
            Ok(parsed) => parsed,
            Err(error) => {
                eprintln!("nsh: {error}");
                self.last_status = 2;
                return 2;
            }
        };
        if let Some(pipeline) = parsed.last_mut() {
            pipeline.background |= background;
            if let Some(command) = pipeline.commands.last_mut() {
                command.redirects.extend(redirects);
            }
        }
        let action = |shell: &mut Self| {
            let mut status = shell.last_status;
            for pipeline in parsed {
                let should_run = match pipeline.condition {
                    Condition::Always => true,
                    Condition::OnSuccess => status == 0,
                    Condition::OnFailure => status != 0,
                };
                if !should_run {
                    continue;
                }
                status = shell.execute_pipeline(pipeline, alias_depth);
                shell.last_status = status;
                if shell.exit_requested {
                    break;
                }
            }
            status
        };
        if notify {
            let started = std::time::Instant::now();
            let status = action(self);
            execute::notify(text, status, started.elapsed());
            status
        } else {
            action(self)
        }
    }

    fn execute_pipeline(&mut self, mut pipeline: Pipeline, alias_depth: usize) -> i32 {
        for command in &mut pipeline.commands {
            if let Some(word) = command.words.first_mut() {
                word.text = clean_command_name(&word.text);
            }
        }
        let builtin_precedes_alias = pipeline
            .commands
            .first()
            .and_then(|command| command.words.first())
            .is_some_and(|word| {
                is_builtin(&word.text) || matches!(word.text.as_str(), "sudo" | "doas")
            });
        if !builtin_precedes_alias
            && pipeline.commands.len() == 1
            && let Some(expanded) = match self.alias_expansion(&pipeline.commands[0].words) {
                Ok(expanded) => expanded,
                Err(error) => {
                    eprintln!("nsh: alias: {error}");
                    return 2;
                }
            }
        {
            let redirects = std::mem::take(&mut pipeline.commands[0].redirects);
            return self.execute_text_with_suffix(
                &expanded,
                alias_depth + 1,
                redirects,
                pipeline.background,
            );
        }
        for command in &mut pipeline.commands {
            command.words = std::mem::take(&mut command.words)
                .into_iter()
                .flat_map(|word| {
                    crate::expand::command_word(&word, self.last_status)
                        .into_iter()
                        .map(|text| Word {
                            text,
                            has_glob: false,
                        })
                })
                .collect();
            for redirect in &mut command.redirects {
                redirect.path = crate::expand::literal_word(&redirect.path, self.last_status);
            }
        }
        if pipeline.commands.len() == 1 {
            let words: Vec<String> = pipeline.commands[0]
                .words
                .iter()
                .map(|word| word.text.clone())
                .collect();
            if !pipeline.background && words.first().is_some_and(|name| is_builtin(name)) {
                let redirects = pipeline.commands[0].redirects.clone();
                return match execute::with_builtin_redirects(&redirects, || {
                    self.builtin(&words).unwrap()
                }) {
                    Ok(status) => status,
                    Err(error) => {
                        eprintln!("nsh: {}: {}", error.path, error.source);
                        1
                    }
                };
            }
            if matches!(words.first().map(String::as_str), Some("sudo" | "doas"))
                && words.len() == 1
            {
                let Some(previous) = &self.last_external else {
                    eprintln!("nsh: {}: no previous executable command", words[0]);
                    return 1;
                };
                if previous.starts_with("sudo ") || previous.starts_with("doas ") {
                    eprintln!("nsh: recursive privilege rerun refused");
                    return 1;
                }
                return self.execute_text(&format!("{} {}", words[0], previous), alias_depth);
            }
        }
        for command in &mut pipeline.commands {
            let Some(name) = command.words.first().map(|word| word.text.clone()) else {
                continue;
            };
            if is_builtin(&name) {
                let command_text = command
                    .words
                    .iter()
                    .map(|word| shell_quote(&word.text))
                    .collect::<Vec<_>>()
                    .join(" ");
                let executable = match env::current_exe() {
                    Ok(path) => path.to_string_lossy().into_owned(),
                    Err(error) => {
                        eprintln!("nsh: {name}: could not start builtin subprocess: {error}");
                        return 126;
                    }
                };
                command.words = vec![
                    Word {
                        text: executable,
                        has_glob: false,
                    },
                    Word {
                        text: "-c".to_string(),
                        has_glob: false,
                    },
                    Word {
                        text: command_text,
                        has_glob: false,
                    },
                ];
                continue;
            }
            if let Some(path) = name.strip_prefix("cd/") {
                eprintln!("nsh: `{name}` is not a command; did you mean `cd /{path}`?");
                return 127;
            }
            if name.contains('/') {
                continue;
            }
            if let Some(path) = self.executable_index.resolve(&name) {
                command.words[0].text = path.to_string_lossy().into_owned();
            } else {
                self.command_not_found(&name);
                return 127;
            }
        }
        self.last_external = Some(pipeline.source.clone());
        self.executor.run(&pipeline)
    }

    fn alias_expansion(&self, words: &[crate::parser::Word]) -> Result<Option<String>, String> {
        let words: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
        let aliases = self.effective_aliases();
        aliases
            .iter()
            .filter(|(name, _)| {
                words.len() >= name.len()
                    && words[..name.len()]
                        .iter()
                        .zip(name.iter())
                        .all(|(word, name)| *word == name)
            })
            .max_by_key(|(name, _)| name.len())
            .map(|(name, body)| {
                let trailing = &words[name.len()..];
                expand_alias_body(body, trailing)
            })
            .transpose()
    }

    fn effective_aliases(&self) -> HashMap<Vec<String>, String> {
        let mut aliases = self.config.aliases.clone();
        if let Some(active) = &self.active_room {
            aliases.extend(active.room.aliases.clone());
        }
        aliases
    }

    fn effective_abbreviations(&self) -> HashMap<String, String> {
        let mut abbreviations = self.config.abbreviations.clone();
        if let Some(active) = &self.active_room {
            abbreviations.extend(active.room.abbreviations.clone());
        }
        abbreviations
    }

    fn completion_commands(&self) -> Vec<String> {
        let mut commands: Vec<String> = self
            .effective_aliases()
            .keys()
            .filter_map(|words| words.first().cloned())
            .collect();
        commands.extend(self.effective_abbreviations().into_keys());
        commands.sort();
        commands.dedup();
        commands
    }

    fn builtin(&mut self, words: &[String]) -> Option<i32> {
        let name = words.first()?.as_str();
        Some(match name {
            "cd" => {
                if words.len() > 2 {
                    eprintln!("nsh: cd: too many arguments");
                    return Some(2);
                }
                let print_target = words.get(1).is_some_and(|word| word == "-");
                let target = if print_target {
                    env::var_os("OLDPWD").map(PathBuf::from)
                } else {
                    words.get(1).map(PathBuf::from)
                }
                .or_else(|| env::var_os("HOME").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
                self.change_directory(&target, print_target)
            }
            "pwd" => {
                if words.len() != 1 {
                    eprintln!("nsh: pwd: does not accept arguments");
                    return Some(2);
                }
                match env::current_dir() {
                    Ok(path) => {
                        println!("{}", path.display());
                        0
                    }
                    Err(error) => {
                        eprintln!("nsh: pwd: {error}");
                        1
                    }
                }
            }
            "export" => export(words),
            "unset" => {
                if let Some(key) = words[1..].iter().find(|key| !valid_variable_name(key)) {
                    eprintln!("nsh: unset: `{key}` is not a valid variable name");
                    return Some(2);
                }
                for key in &words[1..] {
                    unsafe { env::remove_var(key) };
                }
                0
            }
            "pushd" => {
                if words.len() != 2 {
                    eprintln!("nsh: pushd: expected one directory");
                    return Some(2);
                }
                let Ok(current) = env::current_dir() else {
                    eprintln!("nsh: pushd: could not read current directory");
                    return Some(1);
                };
                let status = self.change_directory(Path::new(&words[1]), false);
                if status == 0 {
                    self.directory_stack.push(current);
                    self.print_directories();
                }
                status
            }
            "popd" => {
                if words.len() != 1 {
                    eprintln!("nsh: popd: does not accept arguments");
                    return Some(2);
                }
                let Some(target) = self.directory_stack.last().cloned() else {
                    eprintln!("nsh: popd: directory stack is empty");
                    return Some(1);
                };
                let status = self.change_directory(&target, false);
                if status == 0 {
                    self.directory_stack.pop();
                    self.print_directories();
                }
                status
            }
            "dirs" => {
                if words.len() != 1 {
                    eprintln!("nsh: dirs: does not accept arguments");
                    return Some(2);
                }
                self.print_directories();
                0
            }
            "history" => self.history_command(words),
            "type" => self.type_command(words),
            "jobs" => {
                if words.len() != 1 {
                    eprintln!("nsh: jobs: does not accept arguments");
                    return Some(2);
                }
                self.executor.print_jobs()
            }
            "fg" => match requested_job(words, "fg") {
                Ok(id) => self.executor.foreground(id),
                Err(status) => status,
            },
            "bg" => match requested_job(words, "bg") {
                Ok(id) => self.executor.background(id),
                Err(status) => status,
            },
            "reload" => {
                if words.len() != 1 {
                    eprintln!("nsh: reload: does not accept arguments");
                    return Some(2);
                }
                self.config_modified = None;
                self.reload_config();
                self.refresh_room();
                0
            }
            "exit" => {
                if words.len() > 2 {
                    eprintln!("nsh: exit: too many arguments");
                    return Some(2);
                }
                let status = match words.get(1) {
                    None => self.last_status,
                    Some(code) => match code.parse::<i32>() {
                        Ok(code) => code.rem_euclid(256),
                        Err(_) => {
                            eprintln!("nsh: exit: `{code}` is not a valid exit status");
                            self.exit_requested = true;
                            return Some(2);
                        }
                    },
                };
                self.exit_requested = true;
                status
            }
            "room" => match words {
                [_, action] => crate::room_command(action),
                _ => {
                    eprintln!("nsh: room expects one of status, trust, or untrust");
                    2
                }
            },
            _ => return None,
        })
    }

    fn type_command(&mut self, words: &[String]) -> i32 {
        if words.len() < 2 {
            eprintln!("nsh: type: expected at least one command name");
            return 2;
        }
        let aliases = self.effective_aliases();
        let abbreviations = self.effective_abbreviations();
        let mut status = 0;
        for name in &words[1..] {
            let alias_name: Vec<String> = name.split_whitespace().map(str::to_string).collect();
            if let Some(body) = abbreviations.get(name) {
                println!("{name} is an abbreviation for `{body}`");
            } else if commands::SPECIAL.contains(&name.as_str()) {
                println!("{name} is special Nshell syntax");
            } else if commands::is_builtin(name) {
                println!("{name} is an Nshell built-in");
            } else if let Some(body) = aliases.get(&alias_name) {
                println!("{name} is an alias for `{body}`");
            } else if let Some(path) = self.executable_index.resolve(name) {
                println!("{name} is {}", path.display());
            } else {
                eprintln!("nsh: type: {name}: not found");
                status = 1;
            }
        }
        status
    }

    fn command_not_found(&mut self, name: &str) {
        eprintln!("nsh: {name}: command not found");
        let mut candidates: Vec<String> = commands::BUILTINS
            .iter()
            .chain(commands::SPECIAL.iter())
            .map(|name| (*name).to_string())
            .collect();
        candidates.extend(self.executable_index.names());
        candidates.extend(
            self.effective_aliases()
                .keys()
                .filter_map(|words| words.first().cloned()),
        );
        candidates.extend(self.effective_abbreviations().into_keys());
        let suggestions = commands::suggestions(name, candidates);
        if !suggestions.is_empty() {
            eprintln!("Did you mean {}?", suggestions.join(", "));
        }
    }

    fn change_directory(&mut self, target: &Path, print_target: bool) -> i32 {
        let old_directory = env::current_dir().ok();
        match env::set_current_dir(target) {
            Ok(()) => {
                if let Some(old_directory) = old_directory {
                    unsafe { env::set_var("OLDPWD", old_directory) };
                }
                if let Ok(directory) = env::current_dir() {
                    unsafe { env::set_var("PWD", &directory) };
                    if print_target {
                        println!("{}", directory.display());
                    }
                }
                self.refresh_room();
                self.git_cache = None;
                0
            }
            Err(error) => {
                eprintln!("nsh: cd: {error}");
                if target.is_absolute()
                    && let Some(relative) = target.strip_prefix("/").ok()
                    && let Some(home) = env::var_os("HOME")
                {
                    let home_target = PathBuf::from(home).join(relative);
                    if home_target.exists() {
                        eprintln!("nsh: did you mean `cd ~/{}`?", relative.display());
                    }
                }
                1
            }
        }
    }

    fn print_directories(&self) {
        let mut directories = Vec::new();
        if let Ok(current) = env::current_dir() {
            directories.push(abbreviate_home(&current));
        }
        directories.extend(
            self.directory_stack
                .iter()
                .rev()
                .map(|path| abbreviate_home(path)),
        );
        println!("{}", directories.join(" "));
    }

    fn history_command(&mut self, words: &[String]) -> i32 {
        match words.get(1).map(String::as_str) {
            None => {
                for (index, command) in self.history.iter().enumerate() {
                    println!("{:5}  {command}", index + 1);
                }
                0
            }
            Some("search") => {
                if words.len() < 3 {
                    eprintln!("nsh: history search: expected search text");
                    return 2;
                }
                let query = words[2..].join(" ").to_lowercase();
                for (index, command) in self.history.iter().enumerate() {
                    if command.to_lowercase().contains(&query) {
                        println!("{:5}  {command}", index + 1);
                    }
                }
                0
            }
            Some("clear") => {
                if words.len() > 3 || (words.len() == 3 && words[2] != "--force") {
                    eprintln!("nsh: history clear: expected optional --force");
                    return 2;
                }
                if words.get(2).is_none() {
                    print!("Clear all Nshell history? [y/N] ");
                    let _ = io::stdout().flush();
                    let mut answer = String::new();
                    if io::stdin().read_line(&mut answer).is_err()
                        || !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
                    {
                        println!("history kept");
                        return 0;
                    }
                }
                self.history.clear();
                match state::ensure_directory()
                    .and_then(|directory| fs::write(directory.join("history"), ""))
                {
                    Ok(()) => {
                        println!("history cleared");
                        0
                    }
                    Err(error) => {
                        eprintln!("nsh: history: {error}");
                        1
                    }
                }
            }
            _ => {
                eprintln!("nsh: history: expected search or clear");
                2
            }
        }
    }

    fn prompt(&mut self) -> String {
        let Prompt {
            show_name,
            show_dir,
            show_host,
            show_status,
            show_git,
            show_jobs,
            show_duration,
            show_time,
            directory_style,
            layout,
            separator,
            newline,
            marker,
            duration_threshold,
            time_format,
            git_dirty_symbol,
            name_color,
            directory_color,
            host_color,
            room_color,
            status_color,
            git_color,
            jobs_color,
            duration_color,
            time_color,
            frame_color,
            success_color,
            error_color,
        } = self.config.prompt.clone();
        let mut parts = Vec::new();
        let color = io::stdout().is_terminal()
            && env::var_os("NO_COLOR").is_none()
            && env::var("TERM").is_ok_and(|term| term != "dumb");
        if show_name {
            let name = env::var("USER").unwrap_or_else(|_| "user".to_string());
            parts.push(paint(&name, &name_color, color));
        }
        if show_dir {
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("?"));
            let display = match &directory_style {
                DirectoryStyle::Full => abbreviate_home(&cwd),
                DirectoryStyle::Short => short_directory(&cwd),
            };
            parts.push(paint(&display, &directory_color, color));
        }
        if show_host {
            parts.push(paint(&hostname(), &host_color, color));
        }
        if show_git && let Some(git) = self.git_prompt(&git_dirty_symbol) {
            parts.push(paint(&git, &git_color, color));
        }
        if let Some(badge) = self
            .active_room
            .as_ref()
            .and_then(|room| room.room.badge.as_ref())
        {
            parts.push(paint(&format!("[{badge}]"), &room_color, color));
        }
        if show_jobs && !self.executor.jobs.is_empty() {
            let count = self.executor.jobs.len();
            let label = if count == 1 { "job" } else { "jobs" };
            parts.push(paint(&format!("[{count} {label}]"), &jobs_color, color));
        }
        if show_duration
            && let Some(duration) = self.last_duration
            && duration >= duration_threshold
        {
            parts.push(paint(
                &format!("[{}]", format_duration(duration)),
                &duration_color,
                color,
            ));
        }
        if show_time && let Some(time) = local_time(&time_format) {
            parts.push(paint(&time, &time_color, color));
        }
        if show_status && self.last_status != 0 {
            parts.push(paint(
                &format!("[{}]", self.last_status),
                &status_color,
                color,
            ));
        }
        let marker = paint(
            &marker,
            if self.last_status == 0 {
                &success_color
            } else {
                &error_color
            },
            color,
        );
        if parts.is_empty() {
            return match layout {
                PromptLayout::Framed => {
                    format!("{} {marker} ", paint("╰─", &frame_color, color))
                }
                _ => format!("{marker} "),
            };
        }
        let information = parts.join(&separator);
        match if newline {
            PromptLayout::TwoLine
        } else {
            layout
        } {
            PromptLayout::Compact => format!("{information}{separator}{marker} "),
            PromptLayout::TwoLine => format!("{information}\n{marker} "),
            PromptLayout::Framed => format!(
                "{} {information}\n{} {marker} ",
                paint("╭─", &frame_color, color),
                paint("╰─", &frame_color, color)
            ),
        }
    }

    fn git_prompt(&mut self, dirty_symbol: &str) -> Option<String> {
        let directory = env::current_dir().ok()?;
        if let Some((cached_directory, cached)) = &self.git_cache
            && cached_directory == &directory
        {
            return cached.clone();
        }
        let value = git_status(&directory, dirty_symbol);
        self.git_cache = Some((directory, value.clone()));
        value
    }

    fn reload_config(&mut self) {
        let current = modified(&self.config_path);
        if current == self.config_modified {
            return;
        }
        self.config_modified = current;
        match Config::load(&self.config_path) {
            Ok(config) => {
                self.install_config(config);
            }
            Err(error) => eprintln!("{error} (keeping last valid configuration)"),
        }
    }

    fn run_startup(&mut self) {
        if self.startup_ran {
            return;
        }
        self.startup_ran = true;
        for command in self.config.startup.clone() {
            self.execute_text(&command, 0);
        }
    }

    fn apply_global_environment(&mut self) {
        let removed: Vec<String> = self
            .global_environment
            .keys()
            .filter(|key| !self.config.environment.contains_key(*key))
            .cloned()
            .collect();
        for key in removed {
            if let Some(original) = self.inherited_environment.get(&key) {
                if let Some(value) = original {
                    unsafe { env::set_var(&key, value) };
                } else {
                    unsafe { env::remove_var(&key) };
                }
            }
        }
        for (key, value) in &self.config.environment {
            self.inherited_environment
                .entry(key.clone())
                .or_insert_with(|| env::var_os(key));
            unsafe { env::set_var(key, value) };
        }
        self.global_environment = self.config.environment.clone();
    }

    fn install_config(&mut self, config: Config) {
        let active = self.active_room.take();
        if let Some(active) = &active {
            active.restore();
        }
        self.config = config;
        self.git_cache = None;
        self.apply_global_environment();
        if let Some(active) = active {
            self.active_room = Some(ActiveRoom::activate(active.room));
        }
    }

    fn refresh_room(&mut self) {
        let path = env::current_dir().ok().and_then(|cwd| room::discover(&cwd));
        let loaded = path.as_deref().and_then(|path| match room::load(path) {
            Ok(room) => Some(room),
            Err(error) => {
                eprintln!("{error}");
                None
            }
        });
        if loaded.is_none()
            && path.is_some()
            && self
                .active_room
                .as_ref()
                .is_some_and(|active| Some(active.room.path.as_path()) == path.as_deref())
        {
            return;
        }
        let unchanged = match (&self.active_room, &loaded) {
            (Some(active), Some(found)) => {
                active.room.path == found.path && active.room.hash == found.hash
            }
            (None, None) => true,
            _ => false,
        };
        let declined_unchanged = loaded.as_ref().is_some_and(|found| {
            self.declined_room
                .as_ref()
                .is_some_and(|(path, hash)| path == &found.path && hash == &found.hash)
        });
        if unchanged || declined_unchanged {
            return;
        }
        self.leave_room();
        let Some(found) = loaded else {
            self.declined_room = None;
            return;
        };
        if !room::is_trusted(&found) && !room::request_trust(&found) {
            self.declined_room = Some((found.path.clone(), found.hash.clone()));
            return;
        }
        self.declined_room = None;
        let enter = found
            .on_enter
            .as_ref()
            .map(|command| room::lifecycle(command, &found.name));
        self.active_room = Some(ActiveRoom::activate(found));
        if let Some(command) = enter {
            self.execute_text(&command, 0);
        }
    }

    fn leave_room(&mut self) {
        if let Some(active) = self.active_room.take() {
            if let Some(command) = &active.room.on_leave {
                let command = room::lifecycle(command, &active.room.name);
                self.execute_text(&command, 0);
            }
            active.restore();
        }
    }

    fn record_history(&mut self, command: &str) {
        if self
            .history
            .last()
            .is_some_and(|previous| previous == command)
        {
            return;
        }
        self.history.push(command.to_string());
        if self.history.len() > 10_000 {
            self.history.remove(0);
        }
        if let Ok(directory) = state::ensure_directory() {
            let path = directory.join("history");
            if self.history.len() == 10_000 {
                let contents = format!("{}\n", self.history.join("\n"));
                let _ = fs::write(path, contents);
            } else if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(file, "{}", command.replace('\n', " "));
            }
        }
    }
}

fn paint(text: &str, color: &PromptColor, enabled: bool) -> String {
    if enabled && let Some(code) = color.ansi_code() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn git_status(directory: &Path, dirty_symbol: &str) -> Option<String> {
    let child = Command::new("git")
        .args([
            "-C",
            directory.to_str()?,
            "status",
            "--porcelain=v1",
            "--branch",
            "--untracked-files=normal",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let output = child_output(child, Duration::from_millis(200))?;
    let text = String::from_utf8(output.stdout).ok()?;
    parse_git_status(&text, dirty_symbol)
}

fn child_output(mut child: std::process::Child, timeout: Duration) -> Option<std::process::Output> {
    let started = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(5));
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    child.wait_with_output().ok()
}

fn parse_git_status(text: &str, dirty_symbol: &str) -> Option<String> {
    let mut lines = text.lines();
    let heading = lines.next()?.strip_prefix("## ")?;
    let dirty = lines.next().is_some();
    let branch = if heading.starts_with("HEAD (no branch)") {
        "detached"
    } else {
        heading
            .strip_prefix("No commits yet on ")
            .unwrap_or(heading)
            .split("...")
            .next()
            .unwrap_or(heading)
            .split(" [")
            .next()
            .unwrap_or(heading)
    };
    let branch: String = branch.chars().filter(|ch| !ch.is_control()).collect();
    if branch.is_empty() {
        None
    } else if dirty {
        Some(format!("{branch}{dirty_symbol}"))
    } else {
        Some(branch)
    }
}

fn local_time(format: &TimeFormat) -> Option<String> {
    let argument = match format {
        TimeFormat::TwentyFourHour => "+%H:%M",
        TimeFormat::TwelveHour => "+%I:%M%p",
    };
    let child = Command::new("date")
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let output = child_output(child, Duration::from_millis(100))?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn format_duration(duration: Duration) -> String {
    let milliseconds = duration.as_millis();
    if milliseconds < 1_000 {
        format!("{milliseconds}ms")
    } else if milliseconds < 60_000 {
        let tenths = milliseconds / 100;
        if tenths.is_multiple_of(10) {
            format!("{}s", tenths / 10)
        } else {
            format!("{}.{:01}s", tenths / 10, tenths % 10)
        }
    } else {
        format!(
            "{}m {:02}s",
            milliseconds / 60_000,
            milliseconds / 1_000 % 60
        )
    }
}

fn expand_alias_body(body: &str, arguments: &[&str]) -> Result<String, String> {
    let chars: Vec<char> = body.chars().collect();
    let mut expanded = String::new();
    let mut index = 0;
    let mut used_placeholder = false;
    while index < chars.len() {
        if chars[index] == '$'
            && let Some(placeholder) = chars.get(index + 1).copied()
            && (placeholder == '@' || ('1'..='9').contains(&placeholder))
            && placeholder_boundary(chars.get(index.wrapping_sub(1)).copied())
            && placeholder_boundary(chars.get(index + 2).copied())
        {
            used_placeholder = true;
            if placeholder == '@' {
                expanded.push_str(
                    &arguments
                        .iter()
                        .map(|argument| shell_quote(argument))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            } else {
                let position = placeholder.to_digit(10).unwrap() as usize;
                let Some(argument) = arguments.get(position - 1) else {
                    return Err(format!("missing argument ${placeholder}"));
                };
                expanded.push_str(&shell_quote(argument));
            }
            index += 2;
            continue;
        }
        expanded.push(chars[index]);
        index += 1;
    }
    if !used_placeholder && !arguments.is_empty() {
        expanded.push(' ');
        expanded.push_str(
            &arguments
                .iter()
                .map(|argument| shell_quote(argument))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    Ok(expanded)
}

fn placeholder_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '<' | '>'))
}

fn global_config_path() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".config")
        })
        .join("nshell/config.nsh")
}

fn load_history() -> Vec<String> {
    fs::File::open(state::directory().join("history"))
        .ok()
        .map(|file| {
            let mut history: Vec<String> = io::BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .collect();
            if history.len() > 10_000 {
                history.drain(..history.len() - 10_000);
            }
            history
        })
        .unwrap_or_default()
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn abbreviate_home(path: &Path) -> String {
    let home = env::var_os("HOME").map(PathBuf::from);
    home.as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
        .map(|relative| {
            if relative.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", relative.display())
            }
        })
        .unwrap_or_else(|| path.display().to_string())
}

fn short_directory(path: &Path) -> String {
    if env::var_os("HOME").is_some_and(|home| path == Path::new(&home)) {
        "~".to_string()
    } else if path.parent().is_none() {
        "/".to_string()
    } else {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }
}

fn hostname() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|name| !name.is_empty())
        .or_else(|| {
            fs::read_to_string("/etc/hostname")
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "unknown-host".to_string())
}

fn export(words: &[String]) -> i32 {
    if words.len() == 1 {
        let mut variables: Vec<_> = env::vars().collect();
        variables.sort_by(|left, right| left.0.cmp(&right.0));
        for (key, value) in variables {
            println!("{key}={value}");
        }
        return 0;
    }
    for assignment in &words[1..] {
        let Some((key, value)) = assignment.split_once('=') else {
            eprintln!("nsh: export: expected NAME=VALUE");
            return 2;
        };
        if !valid_variable_name(key) {
            eprintln!("nsh: export: `{key}` is not a valid variable name");
            return 2;
        }
        unsafe { env::set_var(key, value) };
    }
    0
}

fn valid_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_builtin(name: &str) -> bool {
    commands::is_builtin(name)
}

fn shell_quote(word: &str) -> String {
    if word
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_-./".contains(ch))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

fn clean_command_name(name: &str) -> String {
    name.chars()
        .filter(|ch| {
            !ch.is_control()
                && !matches!(
                    *ch,
                    '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
                )
        })
        .collect()
}

fn requested_job(words: &[String], command: &str) -> Result<Option<usize>, i32> {
    if words.len() > 2 {
        eprintln!("nsh: {command}: expected at most one job ID");
        return Err(2);
    }
    let Some(word) = words.get(1) else {
        return Ok(None);
    };
    let id = word.strip_prefix('%').unwrap_or(word);
    match id.parse() {
        Ok(id) => Ok(Some(id)),
        Err(_) => {
            eprintln!("nsh: {command}: `{word}` is not a valid job ID");
            Err(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::Room;

    #[test]
    fn home_is_abbreviated() {
        unsafe { env::set_var("HOME", "/tmp/example-home") };
        assert_eq!(
            abbreviate_home(Path::new("/tmp/example-home/code")),
            "~/code"
        );
    }

    #[test]
    fn alias_arguments_are_safely_quoted() {
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote("plain-file"), "plain-file");
    }

    #[test]
    fn alias_placeholders_preserve_arguments_and_old_append_behavior() {
        assert_eq!(
            expand_alias_body("printf %s $1", &["two words"]).unwrap(),
            "printf %s 'two words'"
        );
        assert_eq!(
            expand_alias_body("printf '%s\\n' $@", &["one", "two words"]).unwrap(),
            "printf '%s\\n' one 'two words'"
        );
        assert_eq!(
            expand_alias_body("printf %s", &["two words"]).unwrap(),
            "printf %s 'two words'"
        );
        assert_eq!(
            expand_alias_body("echo $2; echo first", &["only"]).unwrap_err(),
            "missing argument $2"
        );
    }

    #[test]
    fn prompt_line_break_places_marker_on_second_line() {
        let mut config = Config::default();
        config.prompt.show_name = false;
        config.prompt.show_dir = false;
        config.prompt.show_status = true;
        config.prompt.newline = true;
        config.prompt.marker = ">".to_string();
        let mut shell = test_shell(config);
        shell.last_status = 7;
        assert_eq!(shell.prompt(), "[7]\n> ");
    }

    #[test]
    fn prompt_layouts_and_duration_render_exactly() {
        let mut config = Config::default();
        config.prompt.show_name = false;
        config.prompt.show_dir = false;
        config.prompt.show_status = true;
        config.prompt.show_duration = true;
        config.prompt.duration_threshold = Duration::from_secs(1);
        config.prompt.marker = ">".to_string();
        let mut shell = test_shell(config);
        shell.last_status = 7;
        shell.last_duration = Some(Duration::from_millis(2_350));

        assert_eq!(shell.prompt(), "[2.3s] [7] > ");
        shell.config.prompt.layout = PromptLayout::TwoLine;
        assert_eq!(shell.prompt(), "[2.3s] [7]\n> ");
        shell.config.prompt.layout = PromptLayout::Framed;
        assert_eq!(shell.prompt(), "╭─ [2.3s] [7]\n╰─ > ");
    }

    #[test]
    fn git_status_formats_clean_dirty_and_detached_heads() {
        assert_eq!(
            parse_git_status("## main...origin/main\n", "*").as_deref(),
            Some("main")
        );
        assert_eq!(
            parse_git_status("## No commits yet on trunk\n?? file\n", "+").as_deref(),
            Some("trunk+")
        );
        assert_eq!(
            parse_git_status("## HEAD (no branch)\n M file\n", "*").as_deref(),
            Some("detached*")
        );
    }

    #[test]
    fn config_reload_preserves_room_environment_precedence() {
        let variable = format!("NSH_ENV_PRECEDENCE_{}", std::process::id());
        unsafe { env::set_var(&variable, "inherited") };
        let mut first = Config::default();
        first
            .environment
            .insert(variable.clone(), "global-one".to_string());
        let mut shell = test_shell(first);
        shell.apply_global_environment();
        assert_eq!(env::var(&variable).unwrap(), "global-one");

        let room = Room {
            name: "test".to_string(),
            environment: HashMap::from([(variable.clone(), "room".to_string())]),
            ..Room::default()
        };
        shell.active_room = Some(ActiveRoom::activate(room));
        assert_eq!(env::var(&variable).unwrap(), "room");

        let mut second = Config::default();
        second
            .environment
            .insert(variable.clone(), "global-two".to_string());
        shell.install_config(second);
        assert_eq!(env::var(&variable).unwrap(), "room");

        shell.active_room.take().unwrap().restore();
        assert_eq!(env::var(&variable).unwrap(), "global-two");
        shell.install_config(Config::default());
        assert_eq!(env::var(&variable).unwrap(), "inherited");
        unsafe { env::remove_var(variable) };
    }

    #[test]
    fn room_abbreviations_override_global_abbreviations() {
        let mut config = Config::default();
        config
            .abbreviations
            .insert("t".to_string(), "cargo test".to_string());
        let mut shell = test_shell(config);
        let room = Room {
            name: "test".to_string(),
            abbreviations: HashMap::from([("t".to_string(), "pytest".to_string())]),
            ..Room::default()
        };
        shell.active_room = Some(ActiveRoom::activate(room));
        assert_eq!(shell.effective_abbreviations()["t"], "pytest");
    }

    #[test]
    fn failed_pushd_leaves_directory_stack_untouched() {
        let mut shell = test_shell(Config::default());
        let missing = format!("/tmp/nshell-missing-directory-{}", std::process::id());
        assert_eq!(shell.builtin(&["pushd".to_string(), missing]).unwrap(), 1);
        assert!(shell.directory_stack.is_empty());
    }

    #[test]
    fn prompt_colors_render_or_disable_independently() {
        assert_eq!(
            paint("name", &PromptColor::BrightCyan, true),
            "\u{1b}[96mname\u{1b}[0m"
        );
        assert_eq!(paint("name", &PromptColor::None, true), "name");
        assert_eq!(paint("name", &PromptColor::Red, false), "name");
    }

    #[test]
    fn command_names_drop_invisible_formatting_characters() {
        assert_eq!(clean_command_name("c\u{200b}d"), "cd");
        assert_eq!(clean_command_name("\u{feff}xonsh"), "xonsh");
        assert_eq!(clean_command_name("normal-command"), "normal-command");
    }

    fn test_shell(config: Config) -> Shell {
        Shell {
            config_path: PathBuf::from("/nonexistent/config.nsh"),
            config,
            config_modified: None,
            executor: Executor::new(false),
            active_room: None,
            declined_room: None,
            inherited_environment: HashMap::new(),
            global_environment: HashMap::new(),
            last_external: None,
            history: Vec::new(),
            directory_stack: Vec::new(),
            executable_index: ExecutableIndex::default(),
            last_status: 0,
            last_duration: None,
            git_cache: None,
            startup_ran: false,
            exit_requested: false,
        }
    }
}
