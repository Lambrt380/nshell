use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

pub const BUILTINS: &[&str] = &[
    "bg", "cd", "dirs", "exit", "export", "fg", "history", "jobs", "popd", "pushd", "pwd",
    "reload", "room", "type", "unset",
];

pub const SPECIAL: &[&str] = &["!notify", "doas", "sudo"];

#[derive(Default)]
pub struct ExecutableIndex {
    indexed_path: Option<OsString>,
    executables: BTreeMap<String, PathBuf>,
}

impl ExecutableIndex {
    pub fn refresh(&mut self) {
        let path = env::var_os("PATH");
        if path == self.indexed_path {
            return;
        }
        self.indexed_path = path.clone();
        self.executables.clear();
        let Some(path) = path else {
            return;
        };
        for directory in env::split_paths(&path) {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if is_executable(&path) {
                    self.executables
                        .entry(entry.file_name().to_string_lossy().into_owned())
                        .or_insert(path);
                }
            }
        }
    }

    pub fn resolve(&mut self, name: &str) -> Option<PathBuf> {
        if name == "nsh" {
            return env::current_exe().ok();
        }
        self.refresh();
        self.executables.get(name).cloned()
    }

    pub fn names(&mut self) -> Vec<String> {
        self.refresh();
        let mut names: Vec<String> = self.executables.keys().cloned().collect();
        if !names.iter().any(|name| name == "nsh") {
            names.push("nsh".to_string());
            names.sort();
        }
        names
    }
}

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

pub fn suggestions(name: &str, candidates: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut ranked: Vec<(usize, String)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let distance = edit_distance(name, &candidate);
            let limit = (name.chars().count() / 3).max(2);
            (distance <= limit).then_some((distance, candidate))
        })
        .collect();
    ranked.sort();
    ranked.dedup_by(|left, right| left.1 == right.1);
    ranked
        .into_iter()
        .take(3)
        .map(|(_, candidate)| candidate)
        .collect()
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right.iter().enumerate() {
            current.push(
                (current[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(previous[right_index] + usize::from(left_char != *right_char)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_are_ranked_and_limited() {
        assert_eq!(
            suggestions(
                "cagro",
                ["cargo", "cat", "go", "cagro-long"]
                    .into_iter()
                    .map(str::to_string)
            ),
            ["cargo"]
        );
    }
}
