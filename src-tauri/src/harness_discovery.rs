//! Bounded load-time discovery of user-installed harness CLIs.
//!
//! Discovery proves only that a named, runnable executable is currently
//! available. Protocol capability and session admission remain separate.

use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_CANDIDATES: usize = 128;
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledHarness {
    pub(crate) id: &'static str,
    pub(crate) executable: PathBuf,
    pub(crate) version: String,
}

#[derive(Clone, Copy)]
struct HarnessSpec {
    id: &'static str,
    command: &'static str,
}

const HARNESS_SPECS: &[HarnessSpec] = &[
    HarnessSpec {
        id: "codex",
        command: "codex",
    },
    HarnessSpec {
        id: "claude-code",
        command: "claude",
    },
    HarnessSpec {
        id: "oh-my-pi",
        command: "omp",
    },
    HarnessSpec {
        id: "pi",
        command: "pi",
    },
];

/// Runs during bootstrap and never treats a PATH string or a filename as
/// availability. The candidate must successfully execute its own version
/// command before it is returned.
pub(crate) fn discover_installed_harnesses() -> Vec<InstalledHarness> {
    HARNESS_SPECS.iter().filter_map(discover).collect()
}

fn discover(spec: &HarnessSpec) -> Option<InstalledHarness> {
    candidate_paths(spec.command)
        .into_iter()
        .find_map(|executable| {
            run_version(&executable).map(|version| InstalledHarness {
                id: spec.id,
                executable,
                version,
            })
        })
}

fn candidate_paths(command: &str) -> Vec<PathBuf> {
    let mut roots = path_roots();
    roots.extend(typical_user_roots());
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|root| seen.insert(root.clone()))
        .take(MAX_CANDIDATES)
        .flat_map(|root| {
            executable_names(command)
                .into_iter()
                .map(move |name| root.join(name))
        })
        .filter(|candidate| candidate.is_file())
        .collect()
}

fn path_roots() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).take(MAX_CANDIDATES).collect())
        .unwrap_or_default()
}

fn typical_user_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    #[cfg(windows)]
    {
        let mut roots = vec![
            std::env::var_os("APPDATA").map(|value| PathBuf::from(value).join("npm")),
            std::env::var_os("LOCALAPPDATA").map(|value| PathBuf::from(value).join("npm")),
        ];
        if let Some(home) = home {
            roots.push(Some(home.join(".local").join("bin")));
        }
        roots.into_iter().flatten().collect()
    }
    #[cfg(not(windows))]
    {
        let mut roots = vec![
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
        ];
        if let Some(home) = home {
            roots.extend([
                home.join(".local").join("bin"),
                home.join(".npm-global").join("bin"),
                home.join("Library").join("pnpm"),
            ]);
        }
        roots
    }
}

fn executable_names(command: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        vec![
            format!("{command}.exe").into(),
            format!("{command}.cmd").into(),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![command.into()]
    }
}

fn run_version(executable: &Path) -> Option<String> {
    let mut child = Command::new(executable)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + VERSION_TIMEOUT;
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => {
                let output = child.wait_with_output().ok()?;
                return normalized_version(&output.stdout)
                    .or_else(|| normalized_version(&output.stderr));
            }
            Some(_) => return None,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn normalized_version(bytes: &[u8]) -> Option<String> {
    let output = String::from_utf8_lossy(bytes);
    let version = output.trim().lines().next()?.trim();
    (!version.is_empty() && version.len() <= 512).then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_output_requires_nonempty_bounded_first_line() {
        assert_eq!(
            normalized_version(b"codex 1.2.3\nignored"),
            Some("codex 1.2.3".into())
        );
        assert_eq!(normalized_version(b" \n"), None);
        assert_eq!(normalized_version(&vec![b'a'; 513]), None);
    }

    #[test]
    fn cli_names_cover_the_curated_installation_catalog() {
        assert_eq!(
            HARNESS_SPECS.iter().map(|spec| spec.id).collect::<Vec<_>>(),
            ["codex", "claude-code", "oh-my-pi", "pi"]
        );
    }
}
