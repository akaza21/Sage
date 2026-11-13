use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde_json::Value;
use tauri::State;
use tokio::io::AsyncReadExt;

use crate::state::AppState;

const STATUS_LIMIT: u64 = 4_096;
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
type ClaudeEnvironment = Vec<(OsString, OsString)>;

#[cfg(windows)]
mod windows_login;
#[cfg(windows)]
use windows_login::launch_login;

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAuthStatus {
    logged_in: bool,
}

fn parse_status(bytes: &[u8]) -> Option<ClaudeAuthStatus> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let logged_in = value.get("loggedIn")?.as_bool()?;
    Some(ClaudeAuthStatus { logged_in })
}

#[tauri::command]
pub async fn get_claude_auth_status(
    state: State<'_, AppState>,
) -> Result<ClaudeAuthStatus, &'static str> {
    let path = state
        .harnesses
        .claude_cli_path()
        .ok_or("Claude Code CLI is unavailable.")?;
    let environment = state
        .harnesses
        .claude_auth_environment()
        .ok_or("Claude Code binding is unavailable.")?;
    run_status(&path, &environment).await
}

#[tauri::command]
pub fn start_claude_auth_login(state: State<'_, AppState>) -> Result<(), &'static str> {
    let path = state
        .harnesses
        .claude_cli_path()
        .ok_or("Claude Code CLI is unavailable.")?;
    let environment = state
        .harnesses
        .claude_auth_environment()
        .ok_or("Claude Code binding is unavailable.")?;
    let cwd = auth_cwd(&environment)?;
    launch_login(&path, &environment, &cwd)
}

fn auth_cwd(environment: &ClaudeEnvironment) -> Result<PathBuf, &'static str> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let profile = environment
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| PathBuf::from(value))
        .ok_or("Claude Code profile directory is unavailable.")?;
    if !profile.is_absolute() || !profile.is_dir() {
        return Err("Claude Code profile directory is unavailable.");
    }
    #[cfg(windows)]
    if !matches!(
        profile.components().next(),
        Some(std::path::Component::Prefix(prefix))
            if matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            )
    ) {
        return Err("Claude Code profile directory is unavailable.");
    }
    Ok(profile)
}

fn status_command(path: &Path, environment: &ClaudeEnvironment) -> Result<Command, &'static str> {
    let mut command = Command::new(path);
    command.args(["auth", "status", "--json"]);
    command.env_clear().envs(environment.iter().cloned());
    command.current_dir(auth_cwd(environment)?);
    Ok(command)
}

async fn run_status(
    path: &Path,
    environment: &ClaudeEnvironment,
) -> Result<ClaudeAuthStatus, &'static str> {
    run_status_with_timeout(path, environment, STATUS_TIMEOUT).await
}

/// Kills the status check's whole process group if it is dropped while armed,
/// so a CLI that forks (e.g. a node wrapper) leaves no descendants behind.
#[cfg(unix)]
struct ProcessGroupGuard(Option<libc::pid_t>);

#[cfg(unix)]
impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            // SAFETY: the group was created for this child, and its leader is
            // not yet reaped, so the id cannot name an unrelated group.
            unsafe { libc::killpg(group, libc::SIGKILL) };
        }
    }
}

/// Runs `claude auth status --json` and waits for its output and exit,
/// both within `timeout`. Any early return (timeout, oversized output)
/// kills the child and, on Unix, every process in its group.
async fn run_status_with_timeout(
    path: &Path,
    environment: &ClaudeEnvironment,
    timeout: Duration,
) -> Result<ClaudeAuthStatus, &'static str> {
    let mut command = status_command(path, environment)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Claude Code sign-in check could not start.")?;
    // Declared after `child` so it drops first, while the leader is unreaped.
    #[cfg(unix)]
    let mut group = ProcessGroupGuard(child.id().and_then(|id| libc::pid_t::try_from(id).ok()));
    let stdout = child
        .stdout
        .take()
        .ok_or("Claude Code sign-in check failed.")?;
    let check = async {
        let mut bytes = Vec::new();
        stdout
            .take(STATUS_LIMIT + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "Claude Code sign-in check failed.")?;
        if bytes.len() > STATUS_LIMIT as usize {
            return Err("Claude Code sign-in response was too large.");
        }
        let exit = child
            .wait()
            .await
            .map_err(|_| "Claude Code sign-in check failed.")?;
        // The leader is reaped, so its id may be reused: never signal it now.
        // A descendant that closed stdout and outlives a normal exit is left
        // alone; reaching it would need a non-reaping wait tokio lacks.
        #[cfg(unix)]
        {
            group.0 = None;
        }
        Ok((bytes, exit))
    };
    let (bytes, exit) = tokio::time::timeout(timeout, check)
        .await
        .map_err(|_| "Claude Code sign-in check timed out.")??;
    let status = parse_status(&bytes).ok_or("Claude Code sign-in response was invalid.")?;
    if !exit.success() && status.logged_in {
        return Err("Claude Code sign-in check failed.");
    }
    Ok(status)
}

#[cfg(not(windows))]
fn launch_login(_: &Path, _: &ClaudeEnvironment, _: &Path) -> Result<(), &'static str> {
    Err("Open a terminal and run claude auth login.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::{sync::mpsc, thread, time::Instant};

    /// Runs one status check to completion on its own runtime.
    fn check(path: &Path, timeout: Duration) -> Result<ClaudeAuthStatus, &'static str> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_status_with_timeout(
                path,
                &std::env::vars_os().collect(),
                timeout,
            ))
    }

    #[test]
    fn status_accepts_only_a_boolean_login_fact() {
        assert_eq!(
            parse_status(br#"{"loggedIn":false,"authMethod":"none","token":"secret"}"#),
            Some(ClaudeAuthStatus { logged_in: false })
        );
        assert_eq!(
            parse_status(br#"{"loggedIn":true,"authMethod":"claude.ai"}"#),
            Some(ClaudeAuthStatus { logged_in: true })
        );
        assert_eq!(parse_status(br#"{"loggedIn":"true"}"#), None);
        assert_eq!(parse_status(b"not json"), None);
    }

    #[test]
    fn auth_commands_require_an_existing_absolute_profile_directory() {
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        assert!(auth_cwd(&Vec::new()).is_err());
        assert!(auth_cwd(&vec![(OsString::from(key), OsString::from("relative"))]).is_err());
        assert_eq!(
            auth_cwd(&vec![(
                OsString::from(key),
                std::env::temp_dir().into_os_string()
            )]),
            Ok(std::env::temp_dir())
        );
    }

    #[cfg(unix)]
    fn unix_fixture(directory: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn status_reads_the_login_fact_and_fails_closed_on_unix() {
        let directory =
            std::env::temp_dir().join(format!("sage-claude-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let fixture = |name, body| unix_fixture(&directory, name, body);
        let signed_out = fixture(
            "signed-out",
            r#"echo '{"loggedIn":false,"authMethod":"none"}'; exit 1"#,
        );
        assert_eq!(
            check(&signed_out, STATUS_TIMEOUT),
            Ok(ClaudeAuthStatus { logged_in: false })
        );
        let failed = fixture("failed", r#"echo '{"loggedIn":true}'; exit 1"#);
        assert_eq!(
            check(&failed, STATUS_TIMEOUT),
            Err("Claude Code sign-in check failed.")
        );
        let invalid = fixture("invalid", "echo not-json");
        assert_eq!(
            check(&invalid, STATUS_TIMEOUT),
            Err("Claude Code sign-in response was invalid.")
        );
        let oversized = fixture("oversized", "head -c 8192 /dev/zero; exec sleep 30");
        assert_eq!(
            check(&oversized, STATUS_TIMEOUT),
            Err("Claude Code sign-in response was too large.")
        );
        // A CLI that never exits is killed at the deadline, not waited on.
        let hung = fixture("hung", "exec sleep 30");
        let started = std::time::Instant::now();
        assert_eq!(
            check(&hung, Duration::from_millis(100)),
            Err("Claude Code sign-in check timed out.")
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        // A descendant that holds stdout open is killed with the group, so
        // the check neither waits on it nor leaves it running.
        let marker = directory.join("descendant.pid");
        let forked = fixture(
            "forked",
            &format!("sleep 30 & echo $! > '{}'; wait", marker.display()),
        );
        assert_eq!(
            // Long enough for a slow runner to start the shell and write the marker.
            check(&forked, Duration::from_secs(2)),
            Err("Claude Code sign-in check timed out.")
        );
        let descendant: libc::pid_t = std::fs::read_to_string(&marker)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only probes whether the pid still exists.
        while unsafe { libc::kill(descendant, 0) } == 0 {
            assert!(std::time::Instant::now() < deadline, "descendant survived");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            check(&directory.join("missing"), STATUS_TIMEOUT),
            Err("Claude Code sign-in check could not start.")
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(windows)]
    fn fixture_script(body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("sage-claude-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("claude.cmd");
        std::fs::write(&path, format!("@echo off\r\n{body}\r\n")).unwrap();
        (path, directory)
    }

    #[cfg(windows)]
    fn remove_fixture_directory(directory: &Path) -> std::io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match std::fs::remove_dir_all(directory) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error)
                    if matches!(error.raw_os_error(), Some(32 | 33))
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn fixture_cleanup_waits_for_a_sharing_handle_to_be_released() {
        use std::os::windows::fs::OpenOptionsExt;

        let (path, directory) = fixture_script("exit /b 0");
        thread::scope(|scope| {
            let handle = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap();
            let (sender, receiver) = mpsc::sync_channel(1);
            let cleanup_directory = &directory;
            scope.spawn(move || {
                sender
                    .send(remove_fixture_directory(cleanup_directory))
                    .unwrap()
            });
            assert!(matches!(
                receiver.recv_timeout(Duration::from_millis(30)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            drop(handle);
            receiver
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
        });
        assert!(!directory.exists());
    }

    #[cfg(windows)]
    #[test]
    fn signed_out_cli_exit_is_reported_without_exposing_other_json_fields() {
        let (path, directory) =
            fixture_script("echo {\"loggedIn\":false,\"authMethod\":\"none\"}\r\nexit /b 1");
        assert_eq!(
            check(&path, STATUS_TIMEOUT),
            Ok(ClaudeAuthStatus { logged_in: false })
        );
        remove_fixture_directory(&directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn malformed_or_failed_cli_status_cannot_be_reported_as_signed_in() {
        let (path, directory) = fixture_script("echo {\"loggedIn\":true}\r\nexit /b 1");
        assert_eq!(
            check(&path, STATUS_TIMEOUT),
            Err("Claude Code sign-in check failed.")
        );
        std::fs::write(&path, "@echo off\r\necho not-json\r\n").unwrap();
        assert_eq!(
            check(&path, STATUS_TIMEOUT),
            Err("Claude Code sign-in response was invalid.")
        );
        remove_fixture_directory(&directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn missing_and_slow_cli_status_fail_closed() {
        let (path, directory) = fixture_script(":loop\r\ngoto loop");
        assert_eq!(
            check(&path, Duration::from_millis(30)),
            Err("Claude Code sign-in check timed out.")
        );
        let missing = directory.join("missing.exe");
        assert_eq!(
            check(&missing, STATUS_TIMEOUT),
            Err("Claude Code sign-in check could not start.")
        );
        remove_fixture_directory(&directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn login_uses_fixed_arguments_in_a_real_console_with_a_fixture() {
        const SOURCE: &str = r#"
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetConsoleWindow() -> *mut std::ffi::c_void;
                fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
                fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
            }
            fn main() {
                let marker = std::env::var_os("SAGE_CLAUDE_AUTH_FIXTURE_MARKER").unwrap();
                let arguments: Vec<String> = std::env::args().skip(1).collect();
                let console = unsafe { GetConsoleWindow() as usize };
                let mut mode = 0;
                let input = unsafe {
                    GetConsoleMode(GetStdHandle(-10_i32 as u32), &mut mode) != 0
                };
                let cwd = std::env::current_dir().unwrap();
                let staged = std::path::PathBuf::from(&marker).with_extension("pending");
                std::fs::write(&staged, format!("{}|{:?}|{console}|{input}|{}", std::process::id(), arguments, cwd.display())).unwrap();
                std::fs::rename(staged, marker).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        "#;
        let directory =
            std::env::temp_dir().join(format!("sage-claude-console-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("fixture.rs");
        let executable = directory.join("claude.exe");
        let marker = directory.join("marker.txt");
        std::fs::write(&source, SOURCE).unwrap();
        assert!(
            Command::new("rustc")
                .args([
                    source.as_os_str(),
                    std::ffi::OsStr::new("-o"),
                    executable.as_os_str()
                ])
                .status()
                .unwrap()
                .success()
        );
        let mut environment: ClaudeEnvironment = std::env::vars_os().collect();
        environment.push((
            OsString::from("SAGE_CLAUDE_AUTH_FIXTURE_MARKER"),
            marker.as_os_str().to_owned(),
        ));
        environment.retain(|(key, _)| key != "USERPROFILE");
        environment.push((
            OsString::from("USERPROFILE"),
            directory.as_os_str().to_owned(),
        ));
        assert_eq!(
            launch_login(&executable, &environment, &auth_cwd(&environment).unwrap()),
            Ok(())
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let content = std::fs::read_to_string(&marker).unwrap();
        let (process_id, content) = content.split_once('|').unwrap();
        let process_id = process_id.parse().unwrap();
        let (arguments, rest) = content.split_once('|').unwrap();
        let (console, rest) = rest.split_once('|').unwrap();
        let (input, cwd) = rest.split_once('|').unwrap();
        assert_eq!(arguments, "[\"auth\", \"login\"]");
        assert_eq!(input, "true");
        assert_eq!(
            PathBuf::from(cwd).canonicalize().unwrap(),
            directory.canonicalize().unwrap()
        );
        let console: usize = console.parse().unwrap();
        assert_ne!(console, 0);
        assert_ne!(
            console,
            unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() } as usize
        );
        use windows_sys::Win32::{
            Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, process_id) };
        if process.is_null() {
            assert_eq!(unsafe { GetLastError() }, ERROR_INVALID_PARAMETER);
        } else {
            let exited = unsafe { WaitForSingleObject(process, 3_000) };
            unsafe { CloseHandle(process) };
            assert_eq!(exited, WAIT_OBJECT_0);
        }
        remove_fixture_directory(&directory).unwrap();
    }
}
