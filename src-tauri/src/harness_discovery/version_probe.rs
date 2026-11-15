use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Instant,
};

use super::{ProbeLimits, VERSION_POLL_INTERVAL};

pub(super) struct StreamOutput {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
}

pub(super) struct VersionCommandOutput {
    pub(super) stdout: StreamOutput,
    pub(super) stderr: StreamOutput,
}

pub(super) fn run_version_command(
    path: &Path,
    limits: ProbeLimits,
) -> Option<VersionCommandOutput> {
    run_bounded_command(path, &[std::ffi::OsStr::new("--version")], None, limits)
}

pub(super) fn run_bounded_command(
    path: &Path,
    args: &[&std::ffi::OsStr],
    current_dir: Option<&Path>,
    limits: ProbeLimits,
) -> Option<VersionCommandOutput> {
    let mut command = Command::new(path);
    if let Some(directory) = current_dir {
        command.current_dir(directory);
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(sage_process::console::CREATION_FLAGS);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let (stdout_tx, stdout_rx) = mpsc::sync_channel(1);
    let (stderr_tx, stderr_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = stdout_tx.send(read_stream_bounded(stdout, limits.output_bytes_max));
    });
    thread::spawn(move || {
        let _ = stderr_tx.send(read_stream_bounded(stderr, limits.output_bytes_max));
    });

    let deadline = Instant::now() + limits.command_deadline;
    let status = wait_for_exit_before(&mut child, deadline);
    let Some(status) = status else {
        let _ = child.kill();
        let reaped = reap_before(&mut child, Instant::now() + limits.reap_deadline);
        // Keep the Child and both pipe-reader handles alive until the process
        // has exited. A version probe that defies the reap bound must not be
        // detached as a child process at app startup.
        if !reaped {
            handoff_version_reaper(child, stdout_rx, stderr_rx);
        }
        return None;
    };
    if !status.success() {
        let _ = receive_stream_before(stdout_rx, deadline);
        let _ = receive_stream_before(stderr_rx, deadline);
        return None;
    }

    let stdout = receive_stream_before(stdout_rx, deadline)?;
    let stderr = receive_stream_before(stderr_rx, deadline)?;
    (!stdout.truncated && !stderr.truncated).then_some(VersionCommandOutput { stdout, stderr })
}

fn wait_for_exit_before(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Option<std::process::ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(VERSION_POLL_INTERVAL),
            Ok(None) | Err(_) => return None,
        }
    }
}

fn read_stream_bounded(mut stream: impl Read, limit: usize) -> StreamOutput {
    let mut output = StreamOutput {
        bytes: Vec::with_capacity(limit.min(8 * 1024)),
        truncated: false,
    };
    let mut buffer = [0_u8; 8 * 1024];
    while let Ok(read) = stream.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.bytes.len());
        let retained = remaining.min(read);
        output.bytes.extend_from_slice(&buffer[..retained]);
        output.truncated |= retained < read;
    }
    output
}

fn receive_stream_before(
    receiver: mpsc::Receiver<StreamOutput>,
    deadline: Instant,
) -> Option<StreamOutput> {
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
}

fn reap_before(child: &mut std::process::Child, deadline: Instant) -> bool {
    wait_for_exit_before(child, deadline).is_some()
}

fn handoff_version_reaper(
    mut child: std::process::Child,
    stdout: mpsc::Receiver<StreamOutput>,
    stderr: mpsc::Receiver<StreamOutput>,
) {
    thread::spawn(move || {
        let _readers = (stdout, stderr);
        let _ = child.kill();
        let _ = child.wait();
    });
}

#[cfg(all(test, windows))]
mod tests {
    use super::run_version_command;
    use crate::console_window_fixture::{Fixture, assert_launch_has_no_console_window};
    use crate::harness_discovery::ProbeLimits;

    #[test]
    fn version_probe_starts_harness_without_a_console_window() {
        let test_path = module_path!().split_once("::").unwrap().1;
        assert_launch_has_no_console_window(
            &format!("{test_path}::version_probe_starts_harness_without_a_console_window"),
            |executable| {
                let probed = run_version_command(executable, ProbeLimits::PRODUCTION)
                    .expect("the console fixture's version output is captured");
                assert!(!probed.stdout.truncated);
                assert!(probed.stderr.bytes.is_empty());
                probed.stdout.bytes
            },
        );
    }

    #[test]
    fn version_probe_still_reads_cmd_shim_output() {
        let fixture = Fixture::new();
        let shim = fixture.path("claude.cmd");
        sage_test_support::test_executable::write(&shim, "@echo off\r\necho %~n0 %1 4.5.6\r\n")
            .unwrap();

        let output = run_version_command(&shim, ProbeLimits::PRODUCTION)
            .expect("the .cmd shim's version output is captured");
        assert_eq!(
            std::str::from_utf8(&output.stdout.bytes).unwrap().trim(),
            "claude --version 4.5.6"
        );
        assert!(!output.stdout.truncated);
        assert!(output.stderr.bytes.is_empty());
    }
}
