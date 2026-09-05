use crate::parser::{Pipeline, RedirectKind};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

unsafe extern "C" {
    fn getpgrp() -> i32;
    fn setpgid(pid: i32, pgid: i32) -> i32;
    fn tcsetpgrp(fd: i32, pgid: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn signal(signal: i32, disposition: usize) -> usize;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn dup(fd: i32) -> i32;
    fn dup2(old_fd: i32, new_fd: i32) -> i32;
    fn close(fd: i32) -> i32;
}

const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGTERM: i32 = 15;
const SIGCONT: i32 = 18;
const SIGTSTP: i32 = 20;
const SIGTTIN: i32 = 21;
const SIGTTOU: i32 = 22;
const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;
const WNOHANG: i32 = 1;
const WUNTRACED: i32 = 2;
const WCONTINUED: i32 = 8;

pub struct Job {
    pub id: usize,
    pub command: String,
    pids: Vec<i32>,
    pub pgid: i32,
    pub stopped: bool,
    last_pid: i32,
}

pub struct Executor {
    pub jobs: Vec<Job>,
    next_job: usize,
    shell_pgid: i32,
    interactive: bool,
}

struct WaitOutcome {
    status: i32,
    stopped: bool,
}

impl Executor {
    pub fn new(interactive: bool) -> Self {
        if interactive {
            unsafe {
                // An interactive shell must own a distinct process group before
                // it can hand the terminal to foreground jobs.
                setpgid(0, 0);
                signal(SIGINT, SIG_IGN);
                signal(SIGQUIT, SIG_IGN);
                signal(SIGTSTP, SIG_IGN);
                signal(SIGTTIN, SIG_IGN);
                signal(SIGTTOU, SIG_IGN);
            }
        }
        let shell_pgid = unsafe { getpgrp() };
        if interactive {
            unsafe {
                // If setpgid was rejected because this process is already a
                // session leader, getpgrp still supplies the correct group.
                tcsetpgrp(0, shell_pgid);
            }
        }
        Self {
            jobs: Vec::new(),
            next_job: 1,
            shell_pgid,
            interactive,
        }
    }

    pub fn run(&mut self, pipeline: &Pipeline) -> i32 {
        if pipeline
            .commands
            .iter()
            .any(|command| command.words.is_empty())
        {
            return 0;
        }
        let mut pids = Vec::new();
        let mut previous_stdout = None;
        let mut pgid = 0;
        for (index, parsed) in pipeline.commands.iter().enumerate() {
            let words = &parsed.words;
            let mut command = Command::new(&words[0].text);
            command.args(words[1..].iter().map(|word| &word.text));
            if let Some(stdout) = previous_stdout.take() {
                command.stdin(Stdio::from(stdout));
            } else if index > 0 {
                // A stdout redirect on the preceding command replaces its pipe.
                // The next command must see EOF instead of inheriting shell input.
                command.stdin(Stdio::null());
            }
            if index + 1 < pipeline.commands.len() {
                command.stdout(Stdio::piped());
            }
            if let Err(error) = redirects(&mut command, &parsed.redirects) {
                eprintln!("nsh: {}: {}", error.path, error.source);
                terminate_group(pgid);
                reap_pids(&mut pids);
                return 1;
            }
            let target_pgid = pgid;
            unsafe {
                command.pre_exec(move || {
                    signal(SIGINT, SIG_DFL);
                    signal(SIGQUIT, SIG_DFL);
                    signal(SIGTSTP, SIG_DFL);
                    signal(SIGTTIN, SIG_DFL);
                    signal(SIGTTOU, SIG_DFL);
                    if setpgid(0, target_pgid) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            match command.spawn() {
                Ok(mut child) => {
                    let pid = child.id() as i32;
                    if pgid == 0 {
                        pgid = pid;
                    }
                    unsafe {
                        // The child also calls setpgid. Doing it here closes the race
                        // before the shell gives the group terminal ownership.
                        setpgid(pid, pgid);
                    }
                    previous_stdout = child.stdout.take();
                    pids.push(pid);
                }
                Err(error) => {
                    eprintln!("nsh: {}: {error}", words[0].text);
                    terminate_group(pgid);
                    reap_pids(&mut pids);
                    return if error.kind() == io::ErrorKind::NotFound {
                        127
                    } else {
                        126
                    };
                }
            }
        }
        let last_pid = *pids.last().unwrap();
        if pipeline.background {
            return self.add_job(pipeline.source.clone(), pids, pgid, last_pid, false);
        }
        let outcome = self.wait_foreground(pgid, &mut pids, last_pid);
        if outcome.stopped {
            self.add_job(pipeline.source.clone(), pids, pgid, last_pid, true);
        }
        outcome.status
    }

    fn add_job(
        &mut self,
        command: String,
        pids: Vec<i32>,
        pgid: i32,
        last_pid: i32,
        stopped: bool,
    ) -> i32 {
        let id = self.next_job;
        self.next_job += 1;
        println!("[{id}] {}{pgid}", if stopped { "Stopped\t" } else { "" });
        self.jobs.push(Job {
            id,
            command,
            pids,
            pgid,
            stopped,
            last_pid,
        });
        0
    }

    pub fn reap(&mut self) {
        self.jobs.retain_mut(|job| {
            loop {
                let mut status = 0;
                let pid =
                    unsafe { waitpid(-job.pgid, &mut status, WNOHANG | WUNTRACED | WCONTINUED) };
                if pid <= 0 {
                    break;
                }
                if status == 0xffff {
                    job.stopped = false;
                } else if is_stopped(status) {
                    job.stopped = true;
                } else {
                    job.pids.retain(|candidate| *candidate != pid);
                }
            }
            if job.pids.is_empty() {
                println!("[{}] Done\t{}", job.id, job.command);
                false
            } else {
                true
            }
        });
    }

    pub fn print_jobs(&mut self) -> i32 {
        self.reap();
        for job in &self.jobs {
            println!(
                "[{}] {}\t{}",
                job.id,
                if job.stopped { "Stopped" } else { "Running" },
                job.command
            );
        }
        0
    }

    pub fn foreground(&mut self, id: Option<usize>) -> i32 {
        let Some(index) = select_job(&self.jobs, id) else {
            eprintln!("nsh: fg: no such job");
            return 1;
        };
        let mut job = self.jobs.remove(index);
        unsafe {
            kill(-job.pgid, SIGCONT);
        }
        job.stopped = false;
        let outcome = self.wait_foreground(job.pgid, &mut job.pids, job.last_pid);
        if outcome.stopped {
            println!("[{}] Stopped\t{}", job.id, job.command);
            job.stopped = true;
            self.jobs.push(job);
        }
        outcome.status
    }

    pub fn background(&mut self, id: Option<usize>) -> i32 {
        let Some(index) = select_job(&self.jobs, id) else {
            eprintln!("nsh: bg: no such job");
            return 1;
        };
        if unsafe { kill(-self.jobs[index].pgid, SIGCONT) } == -1 {
            eprintln!("nsh: bg: {}", io::Error::last_os_error());
            return 1;
        }
        self.jobs[index].stopped = false;
        println!("[{}] {}", self.jobs[index].id, self.jobs[index].command);
        0
    }

    fn wait_foreground(&self, pgid: i32, pids: &mut Vec<i32>, last_pid: i32) -> WaitOutcome {
        if self.interactive {
            unsafe {
                tcsetpgrp(0, pgid);
            }
        }
        let mut exit_status = 0;
        let mut stopped = false;
        while !pids.is_empty() {
            let mut status = 0;
            let pid = unsafe { waitpid(-pgid, &mut status, WUNTRACED) };
            if pid == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(4) {
                    continue;
                }
                if error.raw_os_error() != Some(10) {
                    eprintln!("nsh: wait: {error}");
                    exit_status = 1;
                }
                break;
            }
            if is_stopped(status) {
                stopped = true;
                exit_status = 128 + ((status >> 8) & 0xff);
                break;
            }
            pids.retain(|candidate| *candidate != pid);
            if pid == last_pid {
                exit_status = status_code(status);
            }
        }
        if self.interactive {
            unsafe {
                tcsetpgrp(0, self.shell_pgid);
            }
        }
        WaitOutcome {
            status: exit_status,
            stopped,
        }
    }
}

fn select_job(jobs: &[Job], id: Option<usize>) -> Option<usize> {
    id.map_or_else(
        || jobs.len().checked_sub(1),
        |id| jobs.iter().position(|job| job.id == id),
    )
}

fn status_code(status: i32) -> i32 {
    if status & 0x7f == 0 {
        (status >> 8) & 0xff
    } else {
        128 + (status & 0x7f)
    }
}

fn is_stopped(status: i32) -> bool {
    status & 0xff == 0x7f
}

fn terminate_group(pgid: i32) {
    if pgid > 0 {
        unsafe {
            kill(-pgid, SIGTERM);
        }
    }
}

fn reap_pids(pids: &mut Vec<i32>) {
    for pid in pids.drain(..) {
        unsafe {
            waitpid(pid, std::ptr::null_mut(), 0);
        }
    }
}

pub struct RedirectError {
    pub path: String,
    pub source: io::Error,
}

fn open_redirect(redirect: &crate::parser::Redirect) -> Result<File, RedirectError> {
    let opened = match redirect.kind {
        RedirectKind::Input => File::open(&redirect.path),
        RedirectKind::Output | RedirectKind::Error => File::create(&redirect.path),
        RedirectKind::Append | RedirectKind::ErrorAppend => OpenOptions::new()
            .create(true)
            .append(true)
            .open(&redirect.path),
    };
    opened.map_err(|source| RedirectError {
        path: redirect.path.clone(),
        source,
    })
}

fn redirects(
    command: &mut Command,
    redirects: &[crate::parser::Redirect],
) -> Result<(), RedirectError> {
    for redirect in redirects {
        let file = open_redirect(redirect)?;
        match redirect.kind {
            RedirectKind::Input => command.stdin(Stdio::from(file)),
            RedirectKind::Output | RedirectKind::Append => command.stdout(Stdio::from(file)),
            RedirectKind::Error | RedirectKind::ErrorAppend => command.stderr(Stdio::from(file)),
        };
    }
    Ok(())
}

pub fn with_builtin_redirects<T>(
    redirects: &[crate::parser::Redirect],
    action: impl FnOnce() -> T,
) -> Result<T, RedirectError> {
    if redirects.is_empty() {
        return Ok(action());
    }
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    let mut restored = RestoreFds(Vec::new());
    for redirect in redirects {
        let target = match redirect.kind {
            RedirectKind::Input => 0,
            RedirectKind::Output | RedirectKind::Append => 1,
            RedirectKind::Error | RedirectKind::ErrorAppend => 2,
        };
        if !restored.0.iter().any(|(fd, _)| *fd == target) {
            let copy = unsafe { dup(target) };
            if copy == -1 {
                return Err(RedirectError {
                    path: redirect.path.clone(),
                    source: io::Error::last_os_error(),
                });
            }
            restored.0.push((target, copy));
        }
        let file = open_redirect(redirect)?;
        if unsafe { dup2(file.as_raw_fd(), target) } == -1 {
            return Err(RedirectError {
                path: redirect.path.clone(),
                source: io::Error::last_os_error(),
            });
        }
    }
    let value = action();
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    drop(restored);
    Ok(value)
}

struct RestoreFds(Vec<(i32, i32)>);

impl Drop for RestoreFds {
    fn drop(&mut self) {
        for (target, saved) in self.0.drain(..).rev() {
            unsafe {
                dup2(saved, target);
                close(saved);
            }
        }
    }
}

pub fn notify(command_text: &str, status: i32, elapsed: Duration) {
    let summary = format!(
        "{} — exit {} ({:.1}s)",
        command_text,
        status,
        elapsed.as_secs_f64()
    );
    let desktop = Command::new("notify-send")
        .arg("Nshell command finished")
        .arg(&summary)
        .status()
        .is_ok_and(|status| status.success());
    if !desktop {
        print!("\x07");
    }
    println!("{summary}");
}
