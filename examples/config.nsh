# Nshell v1 configuration reference
# Location: ~/.config/nshell/config.nsh
#
# Reload this file without restarting:
#   reload
#
# Validate it without opening an interactive shell:
#   nsh check ~/.config/nshell/config.nsh
#
# Generate this complete reference configuration:
#   nsh gen config
#
# If a config already exists, Nshell asks before replacing it. Only y or yes
# confirms replacement; Enter and every other answer preserve the existing file.


# NSHELL MANAGEMENT COMMANDS
#
#   nsh help
#     Shows the command-language, built-in, room, and configuration overview.
#
#   nsh about
#     Shows the shell name, current version, and author description.
#
#   nsh init
#     Creates a small starter config. It refuses to overwrite an existing file.
#
#   nsh gen config
#     Creates this long reference config with every supported feature.
#
#   nsh check
#     Validates the default global config without opening the interactive shell.
#
#   nsh check PATH
#     Validates a selected config or `.nsh-room` and reports precise errors.
#
#   nsh -c 'COMMAND'
#     Runs command text non-interactively and exits with its final status.


# PROMPT
#
# show-name  displays your username.
# show-dir   displays the current directory, abbreviating your home as ~.
# show-host  displays the machine hostname.
# show-status displays [N] after a command exits unsuccessfully.
# show-git displays the current Git branch and a dirty-worktree symbol.
# show-jobs displays [N job] or [N jobs] while background/stopped jobs exist.
# show-duration displays the previous command time after duration-threshold.
# show-time displays local time using time-format.
# layout accepts "compact", "two-line", or "framed".
# directory  accepts "full" or "short".
# separator  controls text between prompt components.
# \          is the backward-compatible form of layout = "two-line".
# prompt     changes the final prompt marker.
# name-color, directory-color, host-color, room-color, and status-color style
# their corresponding information components.
# git-color, jobs-color, duration-color, time-color, and frame-color style the
# new optional components.
# success-color and error-color style the final marker based on exit status.
#
# Available color names:
#   none, default, black, red, green, yellow, blue, magenta, cyan, white
#   bright-black, bright-red, bright-green, bright-yellow, bright-blue
#   bright-magenta, bright-cyan, bright-white
#
# `none` leaves that component uncolored. `default` uses the terminal's normal
# foreground. The standard NO_COLOR environment variable disables every prompt
# color regardless of these settings.
#
# Layout examples without colors:
#   compact:   ~/code main* ▶
#   two-line:  ~/code main*
#              ▶
#   framed:    ╭─ ~/code main*
#              ╰─ ▶
#
# Components always appear in this order: name, directory, host, Git, room,
# jobs, duration, time, and failure status. Git is omitted outside a repository
# or if its short status check cannot finish promptly. Jobs appear only when
# present. Duration accepts whole values in ms, s, or m. The time format accepts
# "24h" or "12h". A trusted room badge is inserted automatically.
# The marker is green after success and red after failure unless NO_COLOR is set.

prompt_style = {
  show-name
  show-dir
  # show-host
  # show-status
  # show-git
  # show-jobs
  # show-duration
  # show-time
  layout = "two-line"
  # layout = "compact"
  # layout = "framed"
  directory = "full"
  # directory = "short"
  separator = " "
  duration-threshold = "2s"
  time-format = "24h"
  git-dirty-symbol = "*"
  name-color = "cyan"
  directory-color = "blue"
  host-color = "yellow"
  room-color = "magenta"
  status-color = "red"
  git-color = "bright-magenta"
  jobs-color = "yellow"
  duration-color = "bright-black"
  time-color = "bright-black"
  frame-color = "bright-black"
  success-color = "bright-green"
  error-color = "bright-red"
  prompt = ">"
}

# Existing configs may continue using a bare \ instead of
# layout = "two-line". Do not use both in the same prompt_style block.
#
# Unicode symbols above work in ordinary modern terminal fonts. If you use a
# Nerd Font, the marker can be changed to an icon such as:
#   prompt = ""


# ERROR ALERT SOUND
#
# Play a short alert whenever an interactive command finishes unsuccessfully.
# `metal-gear` uses Nshell's built-in synthesized stealth-game-style sting.
# `bell` uses the terminal bell. You can also provide an audio file path;
# Nshell tries pw-play, paplay, and aplay, in that order (WAV is the safest).
#
# This is disabled when the setting is absent. Custom paths support ~ and
# environment-variable expansion.

# alert_sound = "metal-gear"
# alert_sound = "bell"
# alert_sound = "~/.config/nshell/my-alert.wav"


# GLOBAL ENVIRONMENT
#
# These values override the environment inherited by Nshell. Removing an entry
# during reload restores its original value. A room value with the same name
# temporarily takes precedence.

# environment.EDITOR = "nano"
# environment.PAGER = "less"


# STARTUP COMMANDS
#
# Each exec_once_opened entry runs once, in declaration order, when a new
# interactive Nshell process starts. Reloading this file does not run them
# again. Remove the leading # to enable an example.

# exec_once_opened = "echo first startup command"
# exec_once_opened = "echo second startup command"


# VISIBLE ABBREVIATIONS
#
# Abbreviations are fish-like editor shortcuts. Type `gs` followed by Space or
# Enter and the editable command line visibly becomes `git status`. Expansion
# happens only for an exact, unquoted command-position word, including after
# ;, |, &&, ||, or &. The expanded text is saved in history.
#
# Abbreviations are interactive only: `nsh -c 'gs'` does not expand them.
# `type gs` explains an abbreviation, and Tab completion includes its name.

abbr gs > "git status"
abbr rebuild > "sudo nixos-rebuild switch"


# SINGLE-COMMAND ALIASES
#
# Syntax:
#   alias NAME > "COMMAND"
#
# The alias name may be bare when it is one word. User arguments are appended
# to the expanded command, so `ll /tmp` becomes `ls -la /tmp`.

alias ll > "ls -la"
alias cls > "clear"
alias please > "sudo"


# ALIAS PARAMETERS
#
# Unquoted standalone $1 through $9 select an argument, while $@ inserts every
# argument.
# Values are kept as separate shell words, so spaces and command characters in
# an argument cannot turn into extra commands. A missing numbered argument is
# reported before any command in the alias body runs.
#
# When an alias contains a placeholder, arguments are not appended again.
# Aliases without placeholders retain the normal append-all behavior.

alias mkcd > {
  mkdir -p $1;
  cd $1;
}

# alias backup > "cp $@ ~/backup/"


# MULTIWORD ALIASES
#
# Quote an alias name containing spaces. Nshell uses longest-prefix matching,
# so `git clean branches` wins over a shorter `git` alias if both exist.

alias "git clean branches" > "git branch --merged"


# MULTI-COMMAND ALIASES
#
# Put one command on each line and terminate it with ;. Aliases may call other
# aliases. Recursive alias loops are detected and reported.

alias "system overview" > {
  echo "User: $USER";
  pwd;
  uname -a;
}


# COMMAND-LINE SYNTAX CHEAT SHEET
#
# These are shell features, not configuration entries. Type them at the prompt:
#
# Variables:
#   echo $HOME
#   echo ${HOME}
#   echo $?
#   echo ~/.config
#
# $? is the previous command's exit status.
#
# Quoting and escaping:
#   echo "double quoted text"
#   echo 'literal $HOME'
#   echo spaces\ are\ escaped
#
# Pipelines and sequencing:
#   printf "hello\n" | tr a-z A-Z
#   echo first; echo second
#
# Conditional execution:
#   cargo build && cargo run
#   cargo test || echo "tests failed"
#
# && runs the next pipeline only after success. || runs it only after failure.
# Conditions evaluate left-to-right, while ; always starts an unconditional
# sequence.
#
# Wildcards:
#   printf "%s\n" *.rs
#   ls build-?.log
#   echo "literal *.rs"
#
# Unquoted * and ? match sorted filesystem paths. Hidden names match only when
# that pattern component begins with a dot. Quoted, escaped, and unmatched
# patterns remain literal.
#
# Redirects:
#   echo replace > file.txt
#   echo append >> file.txt
#   command 2> errors.txt
#   command 2>> errors.txt
#   cat < file.txt
#
# Background jobs:
#   sleep 30 &
#   Ctrl+Z
#   jobs
#   fg %1
#   bg %1
#
# Ctrl+Z suspends a foreground process. `jobs` lists tracked jobs, while `fg`
# and `bg` use the newest job when no %ID is supplied.
#
# Built-ins:
#   cd ~/.config
#   cd -
#   pushd ~/projects
#   popd
#   dirs
#   pwd
#   export NAME=value
#   unset NAME
#   history
#   history search cargo
#   history clear
#   history clear --force
#   type cd
#   type ll
#   type cargo
#   jobs
#   fg
#   bg
#   reload
#   exit
#
# pushd saves the current directory before changing it; popd returns to the
# newest saved directory; dirs prints the current directory and saved stack.
# `type` explains whether a name is special syntax, a built-in, an alias, or an
# executable. Missing commands suggest up to three close names but never run a
# suggestion automatically.
#
# `history search` is case-insensitive. `history clear` asks before removing
# in-memory and persisted history; --force skips that confirmation.
#
# Privilege rerun:
#   sudo command arguments
#   doas command arguments
#   sudo
#   doas
#
# Bare sudo or doas reruns the previous external command. It will not rerun a
# built-in or another bare privilege rerun.
#
# Completion notification:
#   !notify sleep 5
#
# The complete command runs normally. Nshell reports its status and duration,
# tries notify-send, and falls back to a terminal notification.
#
# Editor controls:
#   Left / Right        move by character
#   Ctrl+Left / Right   move by word
#   Alt+Left / Right    move by word
#   Ctrl+B / Ctrl+F     move left or right
#   Ctrl+A / Ctrl+E     start or end of line
#   Home / End          start or end of line
#   Backspace / Delete  delete around the cursor
#   Up / Down           command history
#   Right, Ctrl+F       accept a dim history suggestion
#   Tab                 complete commands, aliases, paths, and subcommands
#   Ctrl+R              search history
#   Ctrl+W              delete the previous word
#   Ctrl+U / Ctrl+K     delete before or after the cursor
#   Ctrl+C              cancel prompt input or interrupt the foreground command
#   Ctrl+Shift+C        copy through your terminal emulator
#   Ctrl+Shift+V        paste through your terminal emulator
#   Ctrl+D              exit when the line is empty
#   Ctrl+L              clear the screen
#
# History is persisted below XDG_STATE_HOME/nshell, or
# ~/.local/state/nshell when XDG_STATE_HOME is unset. Consecutive duplicates
# are skipped and the newest 10,000 entries are retained.
#
# The prompt marker is green after success and red after an error. Set the
# standard NO_COLOR environment variable to disable prompt colors.


# DIRECTORY ROOMS
#
# Room settings do not belong in this global file. Create a `.nsh-room`:
#   nsh new room "python project"
#
# A complete room file looks like this (copy it without the leading # marks):
#
# name = "python project"
#
# environment.PYTHONPATH = "./src"
# environment.DEBUG = "1"
#
# alias run > "python main.py"
# alias test > "pytest"
# abbr t > "pytest -q"
#
# prompt.badge = "PROJECT"
#
# on_enter = "echo entered [[NAME]]"
# on_leave = "echo leaving [[NAME]]"
#
# Room commands:
#   nsh room status
#   nsh room trust
#   nsh room untrust
#   nsh check .nsh-room
#
# Relative room environment paths such as ./src are resolved from the directory
# containing `.nsh-room`. Room environment values and aliases temporarily
# override global values and are restored when you leave the room tree. Nshell
# discovers the nearest room in the current directory or its ancestors, asks
# for trust before first activation, and requires approval again after changes.
