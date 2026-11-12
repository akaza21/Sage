//! Finds the newest published release of each installed harness through the
//! package manager that owns the install, so Sage can say an update exists
//! and show the command that applies it. Sage never runs that command.
//!
//! The lookup runs npm through the user's own Node, so the user's registry,
//! proxy and certificate settings apply. An install Sage cannot attribute to
//! npm or the native Claude installer gets no lookup at all.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    InstalledHarness, ProbeLimits,
    validation::{authentic_descendant_file, authentic_regular_file},
    version_probe::run_bounded_command,
};
use crate::acp::bindings::release_version::ReleaseVersion;

/// The npm package that ships each curated harness CLI.
const NPM_PACKAGES: &[(&str, &str)] = &[
    ("codex", "@openai/codex"),
    ("claude-code", "@anthropic-ai/claude-code"),
    ("pi", crate::acp::bindings::pi::PI_PACKAGE),
    ("gemini-cli", "@google/gemini-cli"),
    ("opencode", "opencode-ai"),
];

/// npm starts Node and reaches the registry; its own fetch timeout is
/// shorter, so the deadline only catches a stalled process.
const LOOKUP_LIMITS: ProbeLimits = ProbeLimits {
    command_deadline: Duration::from_secs(20),
    reap_deadline: Duration::from_millis(500),
    output_bytes_max: 4 * 1024,
};

/// The newest published release of an installed harness, and the command
/// that installs it through the install's owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishedRelease {
    pub(crate) harness_id: &'static str,
    pub(crate) version: String,
    pub(crate) command: String,
}

/// Looks up each attributable install in turn, one npm process at a time.
pub(crate) fn published_releases(installed: &[InstalledHarness]) -> Vec<PublishedRelease> {
    let owned: Vec<_> = installed
        .iter()
        .filter_map(|harness| {
            let package = npm_package(harness.id)?;
            let command = update_command(harness.id, package, &harness.executable)?;
            Some((harness.id, package, command))
        })
        .collect();
    if owned.is_empty() {
        return Vec::new();
    }
    let Some((node, npm_cli)) = npm_cli() else {
        return Vec::new();
    };
    owned
        .into_iter()
        .filter_map(|(harness_id, package, command)| {
            Some(PublishedRelease {
                harness_id,
                version: latest_version(&node, &npm_cli, package)?,
                command,
            })
        })
        .collect()
}

fn npm_package(harness_id: &str) -> Option<&'static str> {
    NPM_PACKAGES
        .iter()
        .find_map(|(id, package)| (*id == harness_id).then_some(*package))
}

/// The update command for the install's owner, or `None` when the install
/// belongs to something Sage cannot attribute (Homebrew, a manual copy).
fn update_command(harness_id: &str, package: &str, executable: &Path) -> Option<String> {
    if npm_owned(executable, package) {
        Some(format!("npm install -g {package}@latest"))
    } else if harness_id == "claude-code" && claude_installer_owned(executable) {
        Some("claude update".into())
    } else {
        None
    }
}

/// Whether the executable is the bin of a global npm install of `package`:
/// a shim beside `node_modules` (Windows), or a link from `<prefix>/bin` into
/// `<prefix>/lib/node_modules/<package>` (Unix).
fn npm_owned(executable: &Path, package: &str) -> bool {
    let package_dir = Path::new("node_modules").join(package);
    let manifest = package_dir.join("package.json");
    let Some(bin) = executable.parent() else {
        return false;
    };
    if authentic_descendant_file(bin, &manifest) {
        return true;
    }
    let (Some(prefix), Ok(target)) = (bin.parent(), executable.canonicalize()) else {
        return false;
    };
    let root = prefix.join("lib");
    authentic_descendant_file(&root, &manifest)
        && root
            .join(&package_dir)
            .canonicalize()
            .is_ok_and(|package_root| target.starts_with(package_root))
}

/// The native installer's launcher in `~/.local/bin`.
fn claude_installer_owned(executable: &Path) -> bool {
    #[cfg(unix)]
    {
        super::native_claude::target(executable).is_some()
    }
    #[cfg(windows)]
    {
        let bin = executable.parent();
        executable.file_name() == Some(OsStr::new("claude.exe"))
            && bin.and_then(Path::file_name) == Some(OsStr::new("bin"))
            && bin.and_then(Path::parent).and_then(Path::file_name) == Some(OsStr::new(".local"))
    }
}

/// The first Node that ships npm beside it, with npm's CLI script. Running
/// the script with Node directly keeps the lookup one process, which the
/// deadline can stop, rather than a shell wrapper that would outlive it.
fn npm_cli() -> Option<(PathBuf, PathBuf)> {
    let script = Path::new("node_modules/npm/bin/npm-cli.js");
    super::claude::node_candidates()
        .into_iter()
        .find_map(|node| {
            let dir = node.parent()?;
            if !authentic_regular_file(&node) {
                return None;
            }
            [dir.to_path_buf(), dir.parent()?.join("lib")]
                .into_iter()
                .find(|root| authentic_descendant_file(root, script))
                .map(|root| (node.clone(), root.join(script)))
        })
}

/// The version npm reports for the package's `latest` tag, if it is a final
/// release.
fn latest_version(node: &Path, npm_cli: &Path, package: &str) -> Option<String> {
    let args = [
        npm_cli.as_os_str(),
        OsStr::new("view"),
        OsStr::new(package),
        OsStr::new("version"),
        OsStr::new("--fetch-retries=0"),
        OsStr::new("--fetch-timeout=15000"),
    ];
    let output = run_bounded_command(node, &args, LOOKUP_LIMITS)?;
    parse_published_version(&output.stdout.bytes)
}

fn parse_published_version(stdout: &[u8]) -> Option<String> {
    let version = std::str::from_utf8(stdout).ok()?.trim();
    ReleaseVersion::parse(version)
        .filter(|parsed| !parsed.is_prerelease())
        .map(|_| version.to_owned())
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
