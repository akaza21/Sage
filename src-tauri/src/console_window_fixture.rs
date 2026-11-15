//! Proves a launch path starts a console program without a console window.
//!
//! Release builds use the GUI subsystem and have no console, but a test binary
//! inherits the console it was started from. The check therefore re-runs the
//! calling test as a detached process, which has no console either, and has it
//! start a fixture that prints the console window it was given.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

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

/// The only argument a launch passes to the fixture.
pub(crate) const FIXTURE_ARGUMENT: &str = "--version";

const COMPILE_DEADLINE: Duration = Duration::from_secs(180);
const FIXTURE_DEADLINE: Duration = Duration::from_secs(15);
const DETACHED_RUN_DEADLINE: Duration = Duration::from_secs(60);
const SUBPROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

const DETACHED_FIXTURE_ENV: &str = "SAGE_CONSOLE_WINDOW_FIXTURE";

/// Runs the check for the test named `test_name` (its path below the crate
/// root). `launch` starts the fixture at the given path with
/// [`FIXTURE_ARGUMENT`] through the launch path under test and returns the
/// fixture's stdout.
pub(crate) fn assert_launch_has_no_console_window(
    test_name: &str,
    launch: impl FnOnce(&Path) -> Vec<u8>,
) {
    if let Some(executable) = std::env::var_os(DETACHED_FIXTURE_ENV) {
        report_from_detached_process(Path::new(&executable), launch);
        return;
    }
    use std::os::windows::process::CommandExt;
    let mut fixture = Fixture::new();
    let executable = compile_console_fixture(&mut fixture);
    let mut rerun = Command::new(std::env::current_exe().unwrap());
    rerun
        .args([test_name, "--exact", "--test-threads=1"])
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
    let launched = report
        .lines()
        .find_map(|line| line.strip_prefix("launched="));
    assert_eq!(launched, Some("0"), "{report}");
}

fn report_from_detached_process(executable: &Path, launch: impl FnOnce(&Path) -> Vec<u8>) {
    assert_eq!(
        // SAFETY: GetConsoleWindow takes no arguments and only reads state.
        unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() } as usize,
        0
    );
    let mut control = Fixture::new();
    let mut plain = Command::new(executable);
    plain.arg(FIXTURE_ARGUMENT);
    let unflagged = control.run(
        "the unflagged control fixture",
        &mut plain,
        FIXTURE_DEADLINE,
    );
    assert!(unflagged.status.success());
    let launched = launch(executable);
    let report = format!(
        "unflagged={}\nlaunched={}\n",
        console_window_seen_by(&unflagged.stdout),
        console_window_seen_by(&launched)
    );
    std::fs::write(report_path(executable), report).unwrap();
}

struct Finished {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// A scratch directory, and the child running in it, removed on drop.
pub(crate) struct Fixture {
    directory: PathBuf,
    child: Option<Child>,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("sage-console-window-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        Self {
            directory,
            child: None,
        }
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn run(&mut self, stage: &str, command: &mut Command, deadline: Duration) -> Finished {
        // Capture to files so a full output pipe cannot stall the child while we wait.
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
                Ok(None) if started.elapsed() < deadline => thread::sleep(SUBPROCESS_POLL_INTERVAL),
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
    assert_eq!(arguments, format!("arguments={:?}", [FIXTURE_ARGUMENT]));
    console.parse().unwrap()
}

/// The detached re-run reports here rather than to stdout, which a
/// console-less process may not have wired up.
fn report_path(fixture: &Path) -> PathBuf {
    fixture.with_file_name("report.txt")
}
