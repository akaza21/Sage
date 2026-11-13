//! The environment the terminal's shell starts with: the allowlist itself, and
//! the exact environment a process spawned on the PTY receives.

use std::{ffi::OsString, fs::File, path::Path, time::Instant};

use super::{super::shell::environment_from, *};

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

fn session_value(name: &str) -> Option<OsString> {
    let value = match name {
        "DISPLAY" => ":0",
        "WAYLAND_DISPLAY" => "wayland-0",
        "XAUTHORITY" => "/run/user/1000/.mutter-Xwaylandauth.ABC123",
        "XDG_RUNTIME_DIR" => "/run/user/1000",
        "DBUS_SESSION_BUS_ADDRESS" => "unix:path=/run/user/1000/bus",
        "XDG_SESSION_TYPE" => "wayland",
        "XDG_CURRENT_DESKTOP" => "GNOME",
        _ => return None,
    };
    Some(OsString::from(value))
}

#[test]
fn environment_passes_the_desktop_session_through_on_linux_only() {
    let host = |name: &str| match name {
        "HOME" => Some(OsString::from("/home/sage")),
        "CARGO" => Some(OsString::from("/opt/cargo")),
        "AWS_SECRET_ACCESS_KEY" => Some(OsString::from("secret")),
        "LD_PRELOAD" => Some(OsString::from("/tmp/evil.so")),
        "XDG_CONFIG_HOME" => Some(OsString::from("/home/sage/.config")),
        _ => session_value(name),
    };
    let entries = environment_from(Path::new("/bin/zsh"), host).unwrap();
    let names = entries.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let session: &[&str] = if cfg!(target_os = "linux") {
        &[
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_SESSION_TYPE",
            "XDG_CURRENT_DESKTOP",
        ]
    } else {
        &[]
    };
    let expected = ["TERM", "COLORTERM", "SHELL", "HOME"]
        .iter()
        .chain(session)
        .chain(&["LC_CTYPE"])
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(names, expected);
    for (name, value) in &entries {
        if let Some(host_value) = session_value(name) {
            assert_eq!(*value, host_value, "{name}");
        }
    }
    for unlisted in [
        "CARGO",
        "AWS_SECRET_ACCESS_KEY",
        "LD_PRELOAD",
        "XDG_CONFIG_HOME",
    ] {
        assert!(!names.contains(&unlisted), "{unlisted} was passed through");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn each_session_variable_is_kept_only_when_the_host_has_it() {
    for &only in &super::super::shell::SESSION_VARIABLES {
        let host = |name: &str| (name == only).then(|| session_value(name)).flatten();
        let entries = environment_from(Path::new("/bin/zsh"), host).unwrap();
        let names = entries.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        assert_eq!(
            names,
            ["TERM", "COLORTERM", "SHELL", only, "LC_CTYPE"],
            "{only}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn session_variables_fail_closed_when_oversized_or_holding_nul() {
    let oversized = |name: &str| {
        (name == "DBUS_SESSION_BUS_ADDRESS")
            .then(|| OsString::from(format!("unix:path={}", "/".repeat(9_000))))
    };
    assert!(environment_from(Path::new("/bin/zsh"), oversized).is_err());

    let with_nul = |name: &str| {
        use std::os::unix::ffi::OsStringExt;
        (name == "DISPLAY").then(|| OsString::from_vec(b":0\0x".to_vec()))
    };
    assert!(environment_from(Path::new("/bin/zsh"), with_nul).is_err());

    // Each fits alone, but together they pass the total budget.
    let crowded = |name: &str| match name {
        "XAUTHORITY" | "DBUS_SESSION_BUS_ADDRESS" => Some(OsString::from("/".repeat(8_000))),
        "PATH" => Some(OsString::from("/".repeat(1_000))),
        _ => None,
    };
    assert!(environment_from(Path::new("/bin/zsh"), crowded).is_err());
    let within = |name: &str| match name {
        "XAUTHORITY" | "DBUS_SESSION_BUS_ADDRESS" => Some(OsString::from("/".repeat(8_000))),
        _ => None,
    };
    assert!(environment_from(Path::new("/bin/zsh"), within).is_ok());
}

/// Set in the copy of the test binary that the exact environment test starts.
const EXACT_MARKER: &str = "SAGE_PTY_TEST_EXACT_ENVIRONMENT";

/// The whole host environment of that copy: allowed names, a lowercase
/// variant of one, credentials and Sage's own variables.
const HOST: [(&str, &str); 12] = [
    ("HOME", "/home/sage"),
    ("PATH", "/usr/bin:/bin"),
    ("LANG", "en_GB.UTF-8"),
    ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
    ("DISPLAY", ":7"),
    ("display", ":8"),
    ("XDG_RUNTIME_DIR", "/run/user/4242"),
    ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/4242/bus"),
    ("AWS_SECRET_ACCESS_KEY", "not-for-the-shell"),
    ("GITHUB_TOKEN", "not-for-the-shell"),
    ("SAGE_TERMINAL_TOKEN", "not-for-the-shell"),
    ("CARGO", "/opt/cargo"),
];

/// `env` stands in for the login shell. With no arguments it prints the
/// environment exec gave it and exits, so what reaches the PTY is exactly
/// what the spawn path passed, with nothing a shell profile added.
#[test]
fn a_process_spawned_on_the_pty_receives_exactly_the_allowlisted_environment() {
    if std::env::var_os(EXACT_MARKER).is_none() {
        // Setting variables in this multithreaded process would race other
        // tests, so the test reruns itself alone in a child process whose
        // host environment is exactly `HOST` and the marker.
        let module = module_path!()
            .split_once("::")
            .map_or(module_path!(), |(_, path)| path);
        let name = format!(
            "{module}::a_process_spawned_on_the_pty_receives_exactly_the_allowlisted_environment"
        );
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([name.as_str(), "--exact", "--test-threads=1", "--nocapture"])
            .env_clear()
            .envs(HOST)
            .env(EXACT_MARKER, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    let root = root();
    let env = LoginShell {
        declared: PathBuf::from("/usr/bin/env"),
        executable: PathBuf::from("/usr/bin/env"),
    };
    let output = TerminalOutputSink::new();
    let directory = File::open(&root).unwrap();
    let Ok(mut process) = launch(&directory, &env, 80, 24, output.clone()) else {
        panic!("env did not start");
    };
    // The output reader ends only once env exits and its end of the PTY
    // closes, so the output is complete then.
    let deadline = Instant::now() + DEADLINE;
    while !process.threads[0].is_finished() {
        assert!(Instant::now() < deadline, "env output did not end");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let text = output_text(&output);
    let mut received = text
        .lines()
        .map(|line| {
            let (name, value) = line
                .split_once('=')
                .unwrap_or_else(|| panic!("not a variable: {line:?} in {text:?}"));
            (name.to_owned(), value.to_owned())
        })
        .collect::<Vec<_>>();
    received.sort();
    let session: &[(&str, &str)] = if cfg!(target_os = "linux") {
        &[
            ("DISPLAY", ":7"),
            ("XDG_RUNTIME_DIR", "/run/user/4242"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/4242/bus"),
        ]
    } else {
        &[]
    };
    let mut expected = [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("SHELL", "/usr/bin/env"),
        ("HOME", "/home/sage"),
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "en_GB.UTF-8"),
        ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
    ]
    .iter()
    .chain(session)
    .map(|&(name, value)| (name.to_owned(), value.to_owned()))
    .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(received, expected, "{text:?}");
    process.terminate().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
