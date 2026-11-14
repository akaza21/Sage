//! Nothing here comes from the WebView, and an unusable value fails closed.

use std::{
    ffi::OsString,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
};

use super::super::TerminalError;

const ENV_VALUE_BYTES: usize = 8_192;
const ENV_TOTAL_BYTES: usize = 16_384;

pub(super) struct LoginShell {
    /// The path as the user set it, for argv[0], SHELL and the dock label.
    pub(super) declared: PathBuf,
    /// The resolved file that was checked and is executed.
    pub(super) executable: PathBuf,
}

pub(super) fn login_shell() -> Result<LoginShell, TerminalError> {
    let declared = PathBuf::from(std::env::var_os("SHELL").ok_or(TerminalError::Backend)?);
    if !declared.is_absolute() {
        return Err(TerminalError::Backend);
    }
    let executable = declared
        .canonicalize()
        .map_err(|_| TerminalError::Backend)?;
    let metadata = executable.metadata().map_err(|_| TerminalError::Backend)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(TerminalError::Backend);
    }
    match std::fs::read_to_string("/etc/shells") {
        Ok(listed) if listed_shell(&listed, &declared, &executable) => {}
        // Only a host without the list skips the check.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(TerminalError::Backend),
    }
    Ok(LoginShell {
        declared,
        executable,
    })
}

pub(super) fn listed_shell(listed: &str, declared: &Path, executable: &Path) -> bool {
    listed
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/'))
        .any(|line| {
            let entry = Path::new(line);
            entry == declared || entry.canonicalize().is_ok_and(|entry| entry == executable)
        })
}

const HOST_VARIABLES: [&str; 9] = [
    "HOME",
    "USER",
    "LOGNAME",
    "PATH",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSH_AUTH_SOCK",
];

/// The Linux desktop session's variables. Only the session sets them, so a
/// login shell's profile files cannot restore them, and without them
/// `xdg-open`, GUI editors, keyring credential helpers and pinentry prompts
/// cannot reach the display or the session bus. They are socket and file
/// paths or plain names, not credentials.
pub(super) const SESSION_VARIABLES: [&str; 7] = [
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
];

/// SSH_AUTH_SOCK is kept so `git` over SSH works as it does in the user's own
/// terminal.
pub(super) fn environment(shell: &Path) -> Result<Vec<(&'static str, OsString)>, TerminalError> {
    environment_from(shell, |name| std::env::var_os(name))
}

pub(super) fn environment_from(
    shell: &Path,
    host: impl Fn(&str) -> Option<OsString>,
) -> Result<Vec<(&'static str, OsString)>, TerminalError> {
    let mut entries = vec![
        ("TERM", OsString::from("xterm-256color")),
        ("COLORTERM", OsString::from("truecolor")),
        ("SHELL", shell.as_os_str().to_owned()),
    ];
    let session: &[&'static str] = if cfg!(target_os = "linux") {
        &SESSION_VARIABLES
    } else {
        &[]
    };
    for &name in HOST_VARIABLES.iter().chain(session) {
        if let Some(value) = host(name) {
            if value.as_bytes().contains(&0) || value.len() > ENV_VALUE_BYTES {
                return Err(TerminalError::Backend);
            }
            entries.push((name, value));
        }
    }
    // Apps opened from Finder get no locale, which leaves shells unable to
    // show non-ASCII text. A UTF-8 character type fixes that and changes
    // nothing else.
    if !entries
        .iter()
        .any(|(name, _)| matches!(*name, "LANG" | "LC_ALL" | "LC_CTYPE"))
    {
        let utf8 = if cfg!(target_os = "macos") {
            "UTF-8"
        } else {
            "C.UTF-8"
        };
        entries.push(("LC_CTYPE", OsString::from(utf8)));
    }
    let total: usize = entries
        .iter()
        .map(|(name, value)| name.len() + value.len() + 2)
        .sum();
    if total > ENV_TOTAL_BYTES {
        return Err(TerminalError::Backend);
    }
    Ok(entries)
}
