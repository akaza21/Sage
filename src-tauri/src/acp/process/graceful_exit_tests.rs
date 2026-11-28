use super::*;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::acp::process::{TreeControl, TreeExit};

/// A scratch folder for the markers a harness writes once its work lands.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-graceful-exit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Reads stdin to its end, then takes about a second (a sign-in refresh still
/// in flight) before it writes `marker` and exits.
fn finishes_after_eof_script(marker: &Path) -> String {
    finishes_after_eof_script_in(marker, 1)
}

/// Like [`finishes_after_eof_script`], taking about `seconds` after stdin ends.
fn finishes_after_eof_script_in(marker: &Path, seconds: u32) -> String {
    #[cfg(windows)]
    {
        format!(
            "%SystemRoot%\\System32\\findstr.exe x >nul & %SystemRoot%\\System32\\PING.EXE -n {} 127.0.0.1 >nul & type nul > {}",
            seconds + 1,
            marker.display()
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "cat >/dev/null; sleep {seconds}; : > '{}'",
            marker.display()
        )
    }
}

/// Never reads stdin and runs for a minute.
fn ignores_eof_script() -> String {
    #[cfg(windows)]
    {
        "%SystemRoot%\\System32\\PING.EXE -n 60 127.0.0.1 >nul".to_owned()
    }
    #[cfg(not(windows))]
    {
        "exec sleep 60".to_owned()
    }
}

fn child(script: &str) -> Child {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("cmd");
        // cmd parses its own command line, so the script is passed unquoted.
        command
            .args(["/d", "/s", "/c"])
            .raw_arg(format!("\"{script}\""));
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    };
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// A harness tree as Sage runs one, with its stdin already closed.
fn tree(epoch: u64, script: &str) -> TreeControl {
    #[cfg(windows)]
    let tree = crate::acp::process::launch_script_for_test(epoch, script);
    #[cfg(not(windows))]
    let tree = crate::acp::process::launch_shell_for_test(epoch, script);
    let (mut stdin, _stdout, control) = tree.into_parts();
    stdin.close();
    control
}

#[test]
fn a_process_finishing_its_work_after_stdin_closes_is_left_to_finish_it() {
    let scratch = Scratch::new();
    let marker = scratch.marker("saved");
    let mut process = child(&finishes_after_eof_script(&marker));
    drop(process.stdin.take());
    assert!(exit_or_kill(&mut process, Instant::now() + EXIT_GRACE));
    assert!(marker.exists(), "the work it did after stdin closed landed");
}

#[test]
fn a_process_that_does_not_exit_is_killed_after_the_grace() {
    let mut process = child(&ignores_eof_script());
    drop(process.stdin.take());
    let begun = Instant::now();
    assert!(!exit_or_kill(
        &mut process,
        begun + Duration::from_millis(300)
    ));
    assert!(begun.elapsed() < Duration::from_secs(5));
    assert!(process.try_wait().unwrap().is_some(), "it was killed");
}

#[test]
fn an_intentional_close_lets_the_harness_finish_after_stdin_closes() {
    let scratch = Scratch::new();
    let marker = scratch.marker("saved");
    let control = tree(71, &finishes_after_eof_script(&marker));
    control
        .request_graceful_termination(TreeExit::Cancelled, EXIT_GRACE)
        .unwrap();
    let (exit, _zero) = control.wait_zero().unwrap();
    assert_eq!(exit, TreeExit::Cancelled, "the close is still Sage's");
    assert!(marker.exists(), "the harness saved its work before exiting");
    assert_eq!(control.exit_before_termination(), Ok(None));
}

#[test]
fn an_intentional_close_kills_a_harness_that_ignores_it_after_the_grace() {
    let control = tree(72, &ignores_eof_script());
    let begun = Instant::now();
    control
        .request_graceful_termination(TreeExit::Cancelled, Duration::from_millis(500))
        .unwrap();
    let (exit, _zero) = control.wait_zero().unwrap();
    let elapsed = begun.elapsed();
    assert_eq!(exit, TreeExit::Cancelled);
    assert!(
        elapsed >= Duration::from_millis(500),
        "killed only after the grace: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "killed soon after the grace: {elapsed:?}"
    );
}

#[test]
fn a_hard_request_ends_a_pending_grace() {
    let control = tree(73, &ignores_eof_script());
    let begun = Instant::now();
    control
        .request_graceful_termination(TreeExit::Cancelled, EXIT_GRACE)
        .unwrap();
    control.request_termination(TreeExit::Deadline).unwrap();
    let (exit, _zero) = control.wait_zero().unwrap();
    assert_eq!(
        exit,
        TreeExit::Cancelled,
        "the first request is the one reported"
    );
    assert!(begun.elapsed() < Duration::from_secs(5));
}

#[test]
fn closing_several_harnesses_gives_each_its_grace_at_once() {
    let scratch = Scratch::new();
    let markers: Vec<_> = (0..3)
        .map(|index| scratch.marker(&format!("saved-{index}")))
        .collect();
    let controls: Vec<_> = markers
        .iter()
        .zip(74..)
        .map(|(marker, epoch)| tree(epoch, &finishes_after_eof_script_in(marker, 2)))
        .collect();
    let begun = Instant::now();
    let exits = close_all(&controls, |control| {
        control
            .request_graceful_termination(TreeExit::ParentShutdown, EXIT_GRACE)
            .and_then(|()| control.wait_zero())
            .map(|(exit, _zero)| exit)
    });
    let elapsed = begun.elapsed();
    assert_eq!(exits, vec![Ok(TreeExit::ParentShutdown); 3]);
    assert!(
        markers.iter().all(|marker| marker.exists()),
        "every harness saved its work"
    );
    // Each needs about two seconds after its stdin closes; one after another
    // would take six.
    assert!(
        elapsed < Duration::from_millis(4_500),
        "closed in parallel: {elapsed:?}"
    );
}

#[test]
fn a_short_wait_never_ends_the_grace_early() {
    let control = tree(77, &ignores_eof_script());
    let begun = Instant::now();
    control
        .request_graceful_termination(TreeExit::Cancelled, Duration::from_millis(1_500))
        .unwrap();
    assert_eq!(
        control
            .wait_zero_before(Instant::now() + Duration::from_millis(200))
            .map(|(exit, _zero)| exit),
        Err(sage_process::error::LaunchError::Deadline)
    );
    assert_eq!(
        control.native_exit_code_if_exited(),
        Ok(None),
        "a wait shorter than the grace does not kill the harness"
    );
    let (exit, _zero) = control.wait_zero().unwrap();
    assert_eq!(exit, TreeExit::Cancelled);
    assert!(
        begun.elapsed() >= Duration::from_millis(1_500),
        "killed at the grace's end"
    );
}

#[test]
fn a_cleanly_finished_probe_is_left_to_save_its_work() {
    let scratch = Scratch::new();
    let marker = scratch.marker("saved");
    let control = tree(78, &finishes_after_eof_script(&marker));
    let (exit, _zero) = end_tree_before(
        &control,
        TreeExit::ParentShutdown,
        Instant::now() + close_budget(),
    )
    .unwrap();
    assert_eq!(exit, TreeExit::ParentShutdown);
    assert!(marker.exists(), "the probe saved its work before exiting");
}

#[test]
fn a_probe_grace_fits_inside_its_cleanup_deadline() {
    let control = tree(79, &ignores_eof_script());
    let begun = Instant::now();
    // Six seconds leave the five-second tree budget and about one second of grace.
    let deadline = begun + Duration::from_secs(6);
    let (exit, _zero) = end_tree_before(&control, TreeExit::ParentShutdown, deadline).unwrap();
    let elapsed = begun.elapsed();
    assert_eq!(exit, TreeExit::ParentShutdown);
    assert!(
        elapsed >= Duration::from_millis(800),
        "it had a grace: {elapsed:?}"
    );
    assert!(
        Instant::now() <= deadline,
        "zero was proven by the deadline: {elapsed:?}"
    );
}
