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
    // Release builds use the GUI subsystem, so a console harness or `.cmd`
    // shim would otherwise open a visible console window for every probe.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
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
    use std::{
        path::{Path, PathBuf},
        process::{Child, Command, ExitStatus, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use super::run_version_command;
    use crate::harness_discovery::ProbeLimits;

    const CONSOLE_FIXTURE: &str = r#"
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
        }
        fn main() {
            let console = unsafe { GetConsoleWindow() as usize };
            let arguments: Vec<String> = std::env::args().skip(1).collect();
            println!("console={console} arguments={arguments:?}");
        }
    "#;

    const COMPILE_DEADLINE: Duration = Duration::from_secs(180);
    const FIXTURE_DEADLINE: Duration = Duration::from_secs(15);
    const DETACHED_RUN_DEADLINE: Duration = Duration::from_secs(60);
    const SUBPROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

    const DETACHED_FIXTURE_ENV: &str = "SAGE_VERSION_PROBE_DETACHED_FIXTURE";

    struct Finished {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    struct Fixture {
        directory: PathBuf,
        child: Option<Child>,
    }

    impl Fixture {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("sage-version-probe-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            Self {
                directory,
                child: None,
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.directory.join(name)
        }

        fn run(&mut self, stage: &str, command: &mut Command, deadline: Duration) -> Finished {
            let stdout_path = self.path("stdout.txt");
            let stderr_path = self.path("stderr.txt");
            let child = command
                .stdin(Stdio::null())
                .stdout(std::fs::File::create(&stdout_path).unwrap())
                .stderr(std::fs::File::create(&stderr_path).unwrap())
                .spawn()
                .unwrap_or_else(|error| panic!("{stage} could not start: {error}"));
            let child = self.child.insert(child);
            let started = Instant::now();
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) if started.elapsed() < deadline => {
                        thread::sleep(SUBPROCESS_POLL_INTERVAL)
                    }
                    Ok(None) => panic!("{stage} did not finish within {deadline:?}"),
                    Err(error) => panic!("{stage} could not be waited on: {error}"),
                }
            };
            self.child = None;
            Finished {
                status,
                stdout: std::fs::read(&stdout_path).unwrap(),
                stderr: std::fs::read(&stderr_path).unwrap(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn compile_console_fixture(fixture: &mut Fixture) -> PathBuf {
        let source = fixture.path("fixture.rs");
        let executable = fixture.path("codex.exe");
        std::fs::write(&source, CONSOLE_FIXTURE).unwrap();
        let mut rustc = Command::new("rustc");
        rustc.arg(&source).arg("-o").arg(&executable);
        let compiled = fixture.run(
            "compiling the console fixture",
            &mut rustc,
            COMPILE_DEADLINE,
        );
        assert!(
            compiled.status.success(),
            "rustc failed to build the console fixture: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        executable
    }

    fn console_window_seen_by(stdout: &[u8]) -> usize {
        let line = std::str::from_utf8(stdout).unwrap().trim();
        let (console, arguments) = line
            .strip_prefix("console=")
            .and_then(|rest| rest.split_once(' '))
            .unwrap_or_else(|| panic!("unexpected fixture output {line:?}"));
        assert_eq!(arguments, r#"arguments=["--version"]"#);
        console.parse().unwrap()
    }

    /// Not stdout, which a console-less process may not have wired up.
    fn report_path(fixture: &Path) -> PathBuf {
        fixture.with_file_name("report.txt")
    }

    /// Like a GUI-subsystem release build this has no console, so a console child
    /// started without CREATE_NO_WINDOW gets a new console window of its own.
    fn report_from_detached_process(executable: &Path) {
        assert_eq!(
            unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() } as usize,
            0
        );
        let mut control = Fixture::new();
        let mut plain = Command::new(executable);
        plain.arg("--version");
        let unflagged = control.run(
            "the unflagged control fixture",
            &mut plain,
            FIXTURE_DEADLINE,
        );
        assert!(unflagged.status.success());
        let probed = run_version_command(executable, ProbeLimits::PRODUCTION)
            .expect("the console fixture's version output is captured");
        assert!(!probed.stdout.truncated);
        assert!(probed.stderr.bytes.is_empty());
        let report = format!(
            "unflagged={}\nprobed={}\n",
            console_window_seen_by(&unflagged.stdout),
            console_window_seen_by(&probed.stdout.bytes)
        );
        std::fs::write(report_path(executable), report).unwrap();
    }

    #[test]
    fn version_probe_starts_harness_without_a_console_window() {
        if let Some(executable) = std::env::var_os(DETACHED_FIXTURE_ENV) {
            report_from_detached_process(Path::new(&executable));
            return;
        }
        use std::os::windows::process::CommandExt;
        let mut fixture = Fixture::new();
        let executable = compile_console_fixture(&mut fixture);
        let test_path = module_path!().split_once("::").unwrap().1;
        let test_name =
            format!("{test_path}::version_probe_starts_harness_without_a_console_window");
        let mut rerun = Command::new(std::env::current_exe().unwrap());
        rerun
            .args([test_name.as_str(), "--exact", "--test-threads=1"])
            .env(DETACHED_FIXTURE_ENV, &executable)
            .creation_flags(windows_sys::Win32::System::Threading::DETACHED_PROCESS);
        let detached = fixture.run("the detached re-run", &mut rerun, DETACHED_RUN_DEADLINE);
        assert!(
            detached.status.success(),
            "the detached re-run failed ({}): {}{}",
            detached.status,
            String::from_utf8_lossy(&detached.stdout),
            String::from_utf8_lossy(&detached.stderr)
        );
        let report = std::fs::read_to_string(report_path(&executable)).unwrap();
        // Control: started without the flag, the same fixture reports the new
        // console window it was given, so a 0 below is not a blind fixture.
        let unflagged = report
            .lines()
            .find_map(|line| line.strip_prefix("unflagged="));
        assert!(
            matches!(unflagged, Some(window) if window != "0"),
            "{report}"
        );
        let probed = report.lines().find_map(|line| line.strip_prefix("probed="));
        assert_eq!(probed, Some("0"), "{report}");
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
