use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::{
    shell::{environment_from, listed_shell},
    *,
};
use crate::user_terminal::INPUT_BYTES;

const DEADLINE: Duration = Duration::from_secs(10);

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("sage-pty-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path.canonicalize().unwrap()
}

fn shell() -> LoginShell {
    LoginShell {
        declared: PathBuf::from("/bin/sh"),
        executable: PathBuf::from("/bin/sh").canonicalize().unwrap(),
    }
}

fn output_text(output: &TerminalOutputSink) -> String {
    String::from_utf8_lossy(&output.snapshot().unwrap().bytes).into_owned()
}

fn wait_for(output: &TerminalOutputSink, needle: &str) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let text = output_text(output);
        if text.contains(needle) {
            return text;
        }
        assert!(Instant::now() < deadline, "no {needle:?} in {text:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start(root: &Path) -> (UnixPtyProcess, TerminalOutputSink) {
    let output = TerminalOutputSink::new();
    let directory = File::open(root).unwrap();
    let Ok(process) = launch(&directory, &shell(), 80, 24, output.clone()) else {
        panic!("the shell did not start");
    };
    (process, output)
}

fn wait_for_exit(process: &UnixPtyProcess) {
    let deadline = Instant::now() + DEADLINE;
    while !process.exited().unwrap() {
        assert!(Instant::now() < deadline, "shell did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Running, not merely unreaped: a killed job's zombie waits for its new
/// parent, which in a container may never reap it.
fn alive(pid: libc::pid_t) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .is_some_and(|(_, rest)| !rest.trim_start().starts_with(['Z', 'X']))
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        // SAFETY: signal 0 only probes whether the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }
}

/// Starts a background job that prints its pid, and returns that pid.
fn start_job(process: &mut UnixPtyProcess, output: &TerminalOutputSink, job: &str) -> libc::pid_t {
    let command = format!("{job} & printf 'job=%s.\\n' \"$!\"\n");
    process.write(command.as_bytes()).unwrap();
    let text = wait_for(output, ".\r\n");
    let pid = text
        .rsplit("job=")
        .next()
        .and_then(|rest| rest.split('.').next())
        .and_then(|pid| pid.trim().parse::<libc::pid_t>().ok())
        .unwrap_or_else(|| panic!("no job id in {text:?}"));
    assert!(alive(pid));
    pid
}

/// A killed job may briefly stay visible until its new parent reaps it.
fn assert_ended(pid: libc::pid_t) {
    let deadline = Instant::now() + DEADLINE;
    while alive(pid) {
        assert!(Instant::now() < deadline, "job {pid} survived close");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn runs_a_login_shell_in_the_project_root_with_an_allowlisted_environment() {
    let root = root();
    let (mut process, output) = start(&root);
    // The format string keeps the echoed command from matching the marker.
    process
        .write(b"printf 'cwd=%s;term=%s;cargo=[%s]\\n' \"$PWD\" \"$TERM\" \"$CARGO\"\n")
        .unwrap();
    let text = wait_for(&output, "cwd=/");
    assert!(
        text.contains(&format!(
            "cwd={};term=xterm-256color;cargo=[]",
            root.display()
        )),
        "{text:?}"
    );
    assert_eq!(process.shell_path(), "/bin/sh");
    process.terminate().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn resizes_the_terminal() {
    let root = root();
    let (mut process, output) = start(&root);
    process.resize(101, 33).unwrap();
    process.write(b"stty size | sed 's/^/size=/'\n").unwrap();
    wait_for(&output, "size=33 101");
    process.terminate().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn close_ends_background_jobs_and_reports_ended_input_after_exit() {
    let root = root();
    let (mut process, output) = start(&root);
    let job = start_job(&mut process, &output, "sleep 600");
    process.terminate().unwrap();
    assert_ended(job);
    assert!(session::members(process.session).unwrap().is_empty());
    // A second close is a no-op.
    process.terminate().unwrap();

    let (mut exiting, _) = start(&root);
    exiting.write(b"exit\n").unwrap();
    wait_for_exit(&exiting);
    assert_eq!(exiting.write(b"echo late\n"), Err(TerminalError::Ended));
    exiting.terminate().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn close_kills_a_job_that_ignores_hangup() {
    let root = root();
    let (mut process, output) = start(&root);
    let job = start_job(&mut process, &output, "(trap '' HUP; exec sleep 600)");
    let started = Instant::now();
    process.terminate().unwrap();
    // Only the SIGKILL sweeps, after the hangup grace, can end it.
    assert!(started.elapsed() >= HANGUP_GRACE);
    assert_ended(job);
    std::fs::remove_dir_all(root).unwrap();
}

/// Reaping the shell early would free its pid and session id while close
/// might still signal them.
#[test]
fn an_exited_shell_stays_unreaped_until_close_proves_its_session_empty() {
    let root = root();
    let (mut process, _) = start(&root);
    let shell = process.session;
    process.write(b"exit\n").unwrap();
    wait_for_exit(&process);
    std::thread::sleep(Duration::from_millis(100));
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{shell}/stat")).unwrap();
        assert!(
            stat.rsplit_once(')')
                .unwrap()
                .1
                .trim_start()
                .starts_with('Z')
        );
    }
    // SAFETY: signal 0 only probes whether the pid is still taken.
    assert_eq!(unsafe { libc::kill(shell, 0) }, 0, "the shell was reaped");
    process.terminate().unwrap();
    assert!(process.finished);
    std::fs::remove_dir_all(root).unwrap();
}

/// A raw-mode program that never reads fills the PTY. Writes must keep
/// returning promptly, as busy once the queue is full, because they run under
/// Sage's session locks.
#[test]
fn input_to_a_program_that_is_not_reading_is_refused_as_busy_without_blocking() {
    let root = root();
    let (mut process, output) = start(&root);
    process
        .write(b"stty raw -echo; printf 'ready.\\n'; sleep 30\n")
        .unwrap();
    wait_for(&output, "ready.\n");
    let chunk = vec![b'a'; INPUT_BYTES];
    let mut busy = false;
    for _ in 0..512 {
        let started = Instant::now();
        let result = process.write(&chunk);
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "a write blocked for {:?}",
            started.elapsed()
        );
        match result {
            Ok(()) => {}
            Err(TerminalError::Busy) => {
                busy = true;
                break;
            }
            Err(error) => panic!("unexpected {error:?}"),
        }
    }
    assert!(busy, "2 MiB of unread input was never refused");
    let started = Instant::now();
    process.terminate().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn accepts_only_shells_listed_in_etc_shells() {
    let sh = shell().executable;
    let listed = "# comment\n/bin/sh\n  /bin/bash  \n";
    assert!(listed_shell(listed, Path::new("/bin/sh"), &sh));
    assert!(!listed_shell(
        "/bin/zsh-not-here\n",
        Path::new("/bin/sh"),
        &sh
    ));
    assert!(!listed_shell("bin/sh\n", Path::new("bin/sh"), &sh));
}

#[test]
fn environment_is_an_allowlist_with_a_utf8_default() {
    let host = |name: &str| match name {
        "HOME" => Some(OsString::from("/home/sage")),
        "SSH_AUTH_SOCK" => Some(OsString::from("/tmp/agent.sock")),
        "CARGO" => Some(OsString::from("/opt/cargo")),
        _ => None,
    };
    let entries = environment_from(Path::new("/bin/zsh"), host).unwrap();
    let names = entries.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "TERM",
            "COLORTERM",
            "SHELL",
            "HOME",
            "SSH_AUTH_SOCK",
            "LC_CTYPE"
        ]
    );
    assert_eq!(entries[2].1, "/bin/zsh");

    let with_locale = |name: &str| (name == "LANG").then(|| OsString::from("en_GB.UTF-8"));
    let entries = environment_from(Path::new("/bin/zsh"), with_locale).unwrap();
    assert!(!entries.iter().any(|(name, _)| *name == "LC_CTYPE"));

    let oversized = |name: &str| (name == "PATH").then(|| OsString::from("/".repeat(9_000)));
    assert!(environment_from(Path::new("/bin/zsh"), oversized).is_err());
}
