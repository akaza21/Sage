//! How a model probe's CLI ends. Claude Code refreshes an expiring OAuth sign-in as it starts,
//! and the sign-in server retires the old refresh token as soon as it answers. On a clean exit
//! Claude Code waits for that refresh and saves the new token (up to about 8.5 s for a process
//! this young); a process killed in between loses it, and the saved sign-in can no longer be
//! refreshed on any machine that shares it. So the probe ends its CLI by closing stdin and
//! kills it only after [`EXIT_GRACE`].

use std::{
    process::{Child, ChildStdin},
    thread,
    time::{Duration, Instant},
};

/// Longer than Claude Code's own wait for an in-flight refresh on exit.
pub(super) const EXIT_GRACE: Duration = Duration::from_secs(10);
const EXIT_POLL: Duration = Duration::from_millis(50);

/// Closes the CLI's stdin, which ends Claude Code's stream-json input, and waits up to `grace`
/// for it to exit on its own before killing it. True when it exited on its own.
pub(super) fn retire(mut child: Child, stdin: Option<ChildStdin>, grace: Duration) -> bool {
    drop(stdin);
    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() < deadline => thread::sleep(EXIT_POLL),
            _ => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    /// A child that reads stdin to its end, then takes about a second (a refresh still in
    /// flight) before it writes `marker` and exits.
    fn finishes_after_eof(marker: &std::path::Path) -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("cmd");
            // cmd parses its own command line, so it is passed through unquoted.
            command.args(["/d", "/s", "/c"]).raw_arg(format!(
                "\"findstr x >nul & ping -n 2 127.0.0.1 >nul & type nul > \"{}\"\"",
                marker.display()
            ));
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("sh");
            command.arg("-c").arg(format!(
                "cat >/dev/null; sleep 1; : > '{}'",
                marker.display()
            ));
            command
        }
    }

    fn ignores_eof() -> Command {
        #[cfg(windows)]
        {
            let mut command = Command::new("ping");
            command.args(["-n", "60", "127.0.0.1"]);
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("sh");
            command.args(["-c", "exec sleep 60"]);
            command
        }
    }

    fn started(mut command: Command) -> (Child, Option<ChildStdin>) {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        (child, stdin)
    }

    #[test]
    fn a_cli_finishing_its_work_after_stdin_closes_is_left_to_finish_it() {
        let directory =
            std::env::temp_dir().join(format!("sage-probe-exit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let marker = directory.join("saved");
        let (child, stdin) = started(finishes_after_eof(&marker));
        assert!(retire(child, stdin, EXIT_GRACE));
        assert!(marker.exists(), "the work it did after stdin closed landed");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_cli_that_does_not_exit_is_killed_after_the_grace() {
        let (child, stdin) = started(ignores_eof());
        let begun = Instant::now();
        assert!(!retire(child, stdin, Duration::from_millis(300)));
        assert!(begun.elapsed() < Duration::from_secs(10));
    }
}
