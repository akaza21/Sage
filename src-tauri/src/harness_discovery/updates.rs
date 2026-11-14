//! Sage never runs the update command it shows. The lookup runs npm through
//! the user's own Node, so the user's registry, proxy and certificate settings
//! apply.

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

const NPM_PACKAGES: &[(&str, &str)] = &[
    ("codex", "@openai/codex"),
    ("claude-code", "@anthropic-ai/claude-code"),
    ("pi", crate::acp::bindings::pi::PI_PACKAGE),
    ("gemini-cli", "@google/gemini-cli"),
    ("opencode", "opencode-ai"),
];

/// npm starts Node and reaches the registry; its own fetch timeout is
/// shorter, so the deadline only catches a stalled process. npm config
/// warnings on stderr can be long, and truncated output reads as a failure.
const LOOKUP_LIMITS: ProbeLimits = ProbeLimits {
    command_deadline: Duration::from_secs(20),
    reap_deadline: Duration::from_millis(500),
    output_bytes_max: 16 * 1024,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishedRelease {
    pub(crate) harness_id: &'static str,
    pub(crate) version: String,
    pub(crate) command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateSource {
    pub(crate) harness_id: &'static str,
    package: &'static str,
    command: String,
}

pub(crate) fn update_sources(installed: &[InstalledHarness]) -> Vec<UpdateSource> {
    installed
        .iter()
        .filter_map(|harness| {
            let package = npm_package(harness.id)?;
            Some(UpdateSource {
                harness_id: harness.id,
                package,
                command: update_command(harness.id, package, &harness.executable)?,
            })
        })
        .collect()
}

/// `None` when every lookup failed (offline, no npm), so the caller can try
/// again rather than wait out the check interval.
pub(crate) fn published_releases(sources: &[UpdateSource]) -> Option<Vec<PublishedRelease>> {
    let (node, npm_cli) = npm_cli()?;
    let found: Vec<_> = sources
        .iter()
        .filter_map(|source| {
            Some(PublishedRelease {
                harness_id: source.harness_id,
                version: latest_version(&node, &npm_cli, source.package)?,
                command: source.command.clone(),
            })
        })
        .collect();
    (!found.is_empty()).then_some(found)
}

fn npm_package(harness_id: &str) -> Option<&'static str> {
    NPM_PACKAGES
        .iter()
        .find_map(|(id, package)| (*id == harness_id).then_some(*package))
}

fn update_command(harness_id: &str, package: &str, executable: &Path) -> Option<String> {
    if npm_owned(executable, package) {
        Some(format!("npm install -g {package}@latest"))
    } else if harness_id == "claude-code" && claude_installer_owned(executable) {
        Some("claude update".into())
    } else {
        None
    }
}

/// On Unix npm links its bins, and discovery does not follow links, so only
/// the Windows layout reaches this.
fn npm_owned(executable: &Path, package: &str) -> bool {
    executable.parent().is_some_and(|bin| {
        authentic_descendant_file(
            bin,
            Path::new("node_modules").join(package).join("package.json"),
        )
    })
}

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

/// Running npm's CLI script with Node directly keeps the lookup one process,
/// which the deadline can stop, rather than a shell wrapper that would outlive it.
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

fn latest_version(node: &Path, npm_cli: &Path, package: &str) -> Option<String> {
    let args = [
        npm_cli.as_os_str(),
        OsStr::new("view"),
        OsStr::new(package),
        OsStr::new("version"),
        OsStr::new("--fetch-retries=0"),
        OsStr::new("--fetch-timeout=15000"),
        OsStr::new("--no-update-notifier"),
        OsStr::new("--logs-max=0"),
    ];
    // From the folder that holds npm itself, so the only project config npm
    // can load is one beside the trusted runtime: never a project's
    // `.npmrc` (and its registry tokens), nor a file another user planted in
    // a shared temporary folder. The user's own and global config still apply.
    let npm_root = npm_cli.ancestors().nth(4)?;
    let output = run_bounded_command(node, &args, Some(npm_root), LOOKUP_LIMITS)?;
    parse_published_version(&output.stdout.bytes)
}

fn parse_published_version(stdout: &[u8]) -> Option<String> {
    let version = std::str::from_utf8(stdout)
        .ok()?
        .trim()
        .lines()
        .next_back()?
        .trim();
    ReleaseVersion::parse(version)
        .filter(|parsed| !parsed.is_prerelease() && !version.contains(char::is_whitespace))
        .map(|_| version.to_owned())
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
