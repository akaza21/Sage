//! A validated executable whose version cannot be read stays
//! present-but-unreadable rather than becoming absent or acquiring a guess.

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    time::Duration,
};

mod candidates;
mod claude;
mod display_versions;
#[cfg(unix)]
mod native_claude;
#[cfg(all(test, unix))]
mod native_claude_tests;
mod npm_package;
mod parsing;
mod pi;
mod updates;
mod validation;
mod version_probe;

#[cfg(all(test, windows))]
mod hash_tests;
#[cfg(test)]
mod tests;

pub(crate) use claude::cached_sha256_file;
pub(crate) use claude::{
    ClaudeLaunchDiscovery, DiscoveredClaudeLaunch, bundled_claude_code_version,
    discover_claude_launch, sha256_file,
};
pub(crate) use display_versions::{
    DisplayVersionCache, discover_installed_harness_presence, enrich_installed_harness_versions,
};
#[cfg(all(test, windows))]
pub(crate) use npm_package::npm_package_bin;
#[cfg(windows)]
pub(crate) use npm_package::npm_package_bin_from_manifest;
use parsing::parse_harness_display_version;
#[cfg(test)]
use parsing::parse_harness_version;
pub(crate) use pi::{DiscoveredPiLaunch, PiLaunchDiscovery, discover_pi_launch};
use sage_domain::harness::HarnessBrand;
pub(crate) use updates::{PublishedRelease, published_releases, update_sources};
use validation::authentic_harness_file as validate_harness_file;
use version_probe::run_version_command;

/// In the path's platform-native bytes.
pub(crate) const PATH_BYTES_MAX: usize = 32 * 1024;
pub(super) const KNOWN_INSTALL_ROOT_CAP: usize = 4;
pub(super) const VERSION_OUTPUT_BYTES_MAX: usize = 16 * 1024;
/// Long enough for a first run after an update, when the OS scans the new
/// binary or Node compiles its cache.
pub(super) const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const VERSION_REAP_TIMEOUT: Duration = Duration::from_millis(250);
pub(super) const VERSION_POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HarnessExecutable {
    Codex,
    Claude,
    Pi,
    Gemini,
    OpenCode,
    CursorAgent,
    OhMyPi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallLayout {
    NpmGlobal,
    CursorAgent,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct KnownHarnessSpec {
    pub(crate) id: &'static str,
    pub(crate) executable: HarnessExecutable,
    pub(crate) install_layout: InstallLayout,
    pub(crate) sign_in: Option<KnownHarnessSignIn>,
    pub(crate) brand: Option<HarnessBrand>,
    /// Curated known commands, whose `since` versions say what an update unlocks.
    pub(crate) known_commands: Option<KnownCommands>,
}

type KnownCommands = fn() -> &'static crate::acp::command_placement::KnownCommandTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KnownHarnessSignIn {
    pub(crate) command: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) status_check: Option<sage_domain::harness::HarnessSignInCheck>,
}

pub(crate) const KNOWN_HARNESS_SPECS: &[KnownHarnessSpec] = &[
    KnownHarnessSpec {
        id: "codex",
        executable: HarnessExecutable::Codex,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "codex login",
            hint: "Sign in to your Codex account.",
            status_check: None,
        }),
        brand: Some(HarnessBrand::Codex),
        known_commands: Some(crate::acp::bindings::codex::known_commands),
    },
    KnownHarnessSpec {
        id: "claude-code",
        executable: HarnessExecutable::Claude,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "claude auth login",
            hint: "Complete sign-in in the terminal.",
            status_check: Some(sage_domain::harness::HarnessSignInCheck::ClaudeCli),
        }),
        brand: Some(HarnessBrand::Claude),
        known_commands: Some(crate::acp::bindings::claude::known_commands),
    },
    KnownHarnessSpec {
        id: "pi",
        executable: HarnessExecutable::Pi,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "pi",
            hint: "Then run /login.",
            status_check: None,
        }),
        brand: Some(HarnessBrand::Pi),
        known_commands: Some(crate::acp::bindings::pi::known_commands),
    },
    KnownHarnessSpec {
        id: "gemini-cli",
        executable: HarnessExecutable::Gemini,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "gemini",
            hint: "Follow the sign-in prompts.",
            status_check: None,
        }),
        brand: None,
        known_commands: None,
    },
    KnownHarnessSpec {
        id: "opencode",
        executable: HarnessExecutable::OpenCode,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "opencode auth login",
            hint: "Choose the provider to sign in.",
            status_check: None,
        }),
        brand: Some(HarnessBrand::OpenCode),
        known_commands: None,
    },
    KnownHarnessSpec {
        id: "cursor-agent",
        executable: HarnessExecutable::CursorAgent,
        install_layout: InstallLayout::CursorAgent,
        sign_in: Some(KnownHarnessSignIn {
            command: "cursor-agent login",
            hint: "Sign in to Cursor Agent.",
            status_check: None,
        }),
        brand: None,
        known_commands: None,
    },
    KnownHarnessSpec {
        id: "oh-my-pi",
        executable: HarnessExecutable::OhMyPi,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: None,
        brand: None,
        known_commands: None,
    },
];

pub(crate) fn known_harness_spec(id: &str) -> Option<&'static KnownHarnessSpec> {
    KNOWN_HARNESS_SPECS.iter().find(|spec| spec.id == id)
}

/// A conservative, normalized semantic version. Pre-release and build
/// metadata are intentionally not accepted until Sage has a concrete harness
/// output shape for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HarnessVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl std::fmt::Display for HarnessVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscoverySource {
    Path,
    KnownInstallRoot,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredHarness {
    pub(crate) executable: PathBuf,
    pub(crate) version: HarnessVersion,
    pub(crate) source: DiscoverySource,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HarnessDiscovery {
    Absent,
    PresentVersionUnreadable {
        executable: PathBuf,
        source: DiscoverySource,
    },
    Present(DiscoveredHarness),
}

#[derive(Clone, Copy)]
pub(super) struct ProbeLimits {
    pub(super) command_deadline: Duration,
    pub(super) reap_deadline: Duration,
    pub(super) output_bytes_max: usize,
}

impl ProbeLimits {
    pub(super) const PRODUCTION: Self = Self {
        command_deadline: VERSION_TIMEOUT,
        reap_deadline: VERSION_REAP_TIMEOUT,
        output_bytes_max: VERSION_OUTPUT_BYTES_MAX,
    };
}

pub(crate) fn authentic_harness_file(path: &Path, expected_basename: &str) -> bool {
    validate_harness_file(path, expected_basename)
}

/// Resolve the native executable once. The SDK retains the launcher separately
/// to detect updates, but executes and pins the resolved artifact.
pub(crate) fn claude_sdk_executable(launcher: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    if let Some(target) = native_claude::target(launcher) {
        return Some(target);
    }
    validation::authentic_regular_file(launcher)
        .then(|| launcher.canonicalize().ok())
        .flatten()
}

pub(crate) fn authentic_regular_file(path: &Path) -> bool {
    validation::authentic_regular_file(path)
}

pub(crate) fn authentic_npm_global_descendant(root: &Path, relative: impl AsRef<Path>) -> bool {
    validation::authentic_descendant_file(root, relative)
}

/// PATH-only, so legacy Codex presence keeps its exact historical candidate set.
pub(crate) fn path_candidates_for_basename(expected_basename: &str) -> Vec<PathBuf> {
    candidates::path_candidates_for_basename(expected_basename)
}

#[cfg(test)]
fn discover_from_candidates(
    harness: HarnessExecutable,
    candidates: Vec<candidates::Candidate>,
    limits: ProbeLimits,
) -> HarnessDiscovery {
    let Some(candidate) = candidates.into_iter().find(|candidate| {
        validation::expected_basenames(harness)
            .iter()
            .any(|basename| validate_harness_file(&candidate.executable, basename))
    }) else {
        return HarnessDiscovery::Absent;
    };

    let output = probe_target(harness, &candidate.executable)
        .and_then(|target| run_version_command(&target, limits));
    let Some(output) = output else {
        return HarnessDiscovery::PresentVersionUnreadable {
            executable: candidate.executable,
            source: candidate.source,
        };
    };
    let version = parse_harness_version(harness, &output.stdout)
        .or_else(|| parse_harness_version(harness, &output.stderr));
    match version {
        Some(version) => HarnessDiscovery::Present(DiscoveredHarness {
            executable: candidate.executable,
            version,
            source: candidate.source,
        }),
        None => HarnessDiscovery::PresentVersionUnreadable {
            executable: candidate.executable,
            source: candidate.source,
        },
    }
}

#[cfg(test)]
fn present_candidate(
    harness: HarnessExecutable,
    candidates: Vec<candidates::Candidate>,
) -> Option<candidates::Candidate> {
    candidates.into_iter().find(|candidate| {
        validation::expected_basenames(harness)
            .iter()
            .any(|basename| validate_harness_file(&candidate.executable, basename))
    })
}

/// The legacy installed-harness shape, kept here so it shares candidate
/// validation and the bounded version process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledHarness {
    pub(crate) id: &'static str,
    pub(crate) executable: PathBuf,
    pub(crate) version: Option<String>,
}

fn probe_target(harness: HarnessExecutable, executable: &Path) -> Option<Cow<'_, Path>> {
    #[cfg(unix)]
    if harness == HarnessExecutable::Claude
        && let Some(target) = native_claude::target(executable)
    {
        return Some(Cow::Owned(target));
    }
    let expected = validation::expected_basenames(harness)
        .iter()
        .any(|basename| executable.file_name() == Some(std::ffi::OsStr::new(basename)));
    (expected && validation::authentic_regular_file(executable))
        .then_some(Cow::Borrowed(executable))
}

pub(crate) fn revalidate_harness_version(id: &str, executable: &Path) -> Option<String> {
    let spec = known_harness_spec(id)?;
    let target = probe_target(spec.executable, executable)?;
    let output = run_version_command(&target, ProbeLimits::PRODUCTION)?;
    parse_harness_display_version(spec.executable, &output.stdout)
        .or_else(|| parse_harness_display_version(spec.executable, &output.stderr))
}
