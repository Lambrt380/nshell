my shell

## Build and install

Build an optimized binary from this directory:

```sh
cargo build --release
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/nsh "$HOME/.local/bin/nsh"
```

Make sure `~/.local/bin` is on your `PATH`. Add this to `~/.bashrc` or
`~/.zshrc` if it is not already configured:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Open a new terminal, then check the installation and start Nshell:

```sh
nsh about
nsh
```

To update an existing installation, rebuild the project and run the same
`install` command again.

## Configuration

The default configuration file is:

```text
~/.config/nshell/config.nsh
```

If `XDG_CONFIG_HOME` is set, Nshell uses
`$XDG_CONFIG_HOME/nshell/config.nsh` instead.

Create a small starter configuration:

```sh
nsh init
```

Or generate the complete commented reference configuration:

```sh
nsh gen config
```

Nshell asks before replacing an existing config. Validate and reload it with:

```sh
nsh check
reload
```

Here is a small example:

```nsh
prompt_style = {
  show-name
  show-dir
  show-status
  show-git
  layout = "two-line"
  directory = "full"
  prompt = ">"
}

environment.EDITOR = "nano"
exec_once_opened = "echo Welcome to Nshell"

alias ll > "ls -la"
abbr gs > "git status"
```

Aliases execute their replacement command and keep any extra arguments:

```nsh
alias ll > "ls -la"
alias mkcd > {
  mkdir -p $1;
  cd $1;
}
alias all > "printf '<%s>\n' $@"
```

`$1` through `$9` select an alias argument, while `$@` inserts all arguments.
Arguments remain separate shell words. Abbreviations are interactive editor
shortcuts: typing `gs` followed by Space or Enter visibly expands it to
`git status` before execution.

See [`examples/config.nsh`](examples/config.nsh) for every prompt option,
color, shortcut, and command example.

## Rooms

A room is a `.nsh-room` file containing project-specific environment values,
aliases, abbreviations, lifecycle commands, and an optional prompt badge.

Create one in the current directory:

```sh
nsh new room my-project
```

Example `.nsh-room`:

```nsh
name = "my-project"
environment.PYTHONPATH = "./src"
alias run > "python main.py"
abbr t > "pytest -q"
prompt.badge = "PROJECT"
on_enter = "echo entered [[NAME]]"
on_leave = "echo leaving [[NAME]]"
```

Rooms can execute commands, so Nshell requires trust before activating one:

```sh
nsh room status
nsh room trust
nsh room untrust
```

Trust is tied to the room file's contents. If the file changes, inspect it and
trust it again.

## Command examples

```sh
printf "hello\n" | tr a-z A-Z
echo saved > output.txt
cargo build && cargo test
sleep 30 &
jobs
fg %1
history search cargo
nsh -c 'echo hello; exit 0'
```

Run `nsh help` for the command overview.

## Development

```sh
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```
