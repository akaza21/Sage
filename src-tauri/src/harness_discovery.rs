//! Bounded, read-only discovery of user-installed harness CLIs.
//!
//! Discovery is separate from protocol characterisation and launch admission.
//! A validated executable whose version cannot be read remains truthfully
//! present-but-unreadable rather than becoming absent or acquiring a guess.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

mod candidates;
mod claude;
#[cfg(unix)]
mod native_claude;
#[cfg(all(test, unix))]
mod native_claude_tests;
mod parsing;
mod pi;
mod updates;
mod validation;
mod version_probe;

#[cfg(all(test, windows))]
mod hash_tests;
#[cfg(test)]
mod tests;

use crate::domain::HarnessBrand;
pub(crate) use claude::cached_sha256_file;
pub(crate) use claude::{
    ClaudeLaunchDiscovery, DiscoveredClaudeLaunch, discover_claude_launch, sha256_file,
};
use parsing::parse_harness_display_version;
#[cfg(test)]
use parsing::parse_harness_version;
pub(crate) use pi::{DiscoveredPiLaunch, PiLaunchDiscovery, discover_pi_launch};
pub(crate) use updates::{PublishedRelease, published_releases};
use validation::authentic_harness_file as validate_harness_file;
use version_probe::run_version_command;

/// The probe rejects paths whose platform-native byte representation is larger
/// than this value.
pub(crate) const PATH_BYTES_MAX: usize = 32 * 1024;
pub(super) const KNOWN_INSTALL_ROOT_CAP: usize = 4;
pub(super) const VERSION_OUTPUT_BYTES_MAX: usize = 16 * 1024;
pub(super) const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KnownHarnessSpec {
    pub(crate) id: &'static str,
    pub(crate) executable: HarnessExecutable,
    pub(crate) install_layout: InstallLayout,
    pub(crate) sign_in: Option<KnownHarnessSignIn>,
    pub(crate) brand: Option<HarnessBrand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KnownHarnessSignIn {
    pub(crate) command: &'static str,
    pub(crate) hint: &'static str,
}

pub(crate) const KNOWN_HARNESS_SPECS: &[KnownHarnessSpec] = &[
    KnownHarnessSpec {
        id: "codex",
        executable: HarnessExecutable::Codex,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "codex login",
            hint: "Sign in to your Codex account.",
        }),
        brand: Some(HarnessBrand::Codex),
    },
    KnownHarnessSpec {
        id: "claude-code",
        executable: HarnessExecutable::Claude,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "claude auth login",
            hint: "Complete sign-in in the terminal.",
        }),
        brand: Some(HarnessBrand::Claude),
    },
    KnownHarnessSpec {
        id: "pi",
        executable: HarnessExecutable::Pi,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "pi",
            hint: "Then run /login.",
        }),
        brand: Some(HarnessBrand::Pi),
    },
    KnownHarnessSpec {
        id: "gemini-cli",
        executable: HarnessExecutable::Gemini,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "gemini",
            hint: "Follow the sign-in prompts.",
        }),
        brand: None,
    },
    KnownHarnessSpec {
        id: "opencode",
        executable: HarnessExecutable::OpenCode,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: Some(KnownHarnessSignIn {
            command: "opencode auth login",
            hint: "Choose the provider to sign in.",
        }),
        brand: Some(HarnessBrand::OpenCode),
    },
    KnownHarnessSpec {
        id: "cursor-agent",
        executable: HarnessExecutable::CursorAgent,
        install_layout: InstallLayout::CursorAgent,
        sign_in: Some(KnownHarnessSignIn {
            command: "cursor-agent login",
            hint: "Sign in to Cursor Agent.",
        }),
        brand: None,
    },
    KnownHarnessSpec {
        id: "oh-my-pi",
        executable: HarnessExecutable::OhMyPi,
        install_layout: InstallLayout::NpmGlobal,
        sign_in: None,
        brand: None,
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

/// Where the executable's validated parent directory was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscoverySource {
    Path,
    KnownInstallRoot,
}

/// A validated harness executable with a strictly parsed version.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredHarness {
    pub(crate) executable: PathBuf,
    pub(crate) version: HarnessVersion,
    pub(crate) source: DiscoverySource,
}

/// The complete truthful result of inspecting one named harness.
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

/// Shared validation used by legacy Codex host-presence reporting. This checks
/// the same path and link discipline as full discovery without running a child.
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

/// Returns candidates formed from the first bounded set of valid PATH roots.
/// Kept public within the crate so Codex's compatibility presence API can
/// retain its exact historical PATH-only candidate set.
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

// The pre-HR-1 projection still consumes the legacy installed-harness shape.
// Keep that adapter here so it shares candidate validation and the bounded
// version process rather than growing a parallel discovery path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledHarness {
    pub(crate) id: &'static str,
    pub(crate) executable: PathBuf,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DisplayFileIdentity {
    canonical_path: PathBuf,
    byte_len: u64,
    modified: SystemTime,
}

#[derive(Debug, Default)]
pub(crate) struct DisplayVersionCache {
    entries: Mutex<HashMap<DisplayFileIdentity, String>>,
}

impl DisplayVersionCache {
    fn version(&self, identity: &DisplayFileIdentity) -> Option<String> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(identity)
            .cloned()
    }

    fn insert(&self, identity: DisplayFileIdentity, version: String) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(identity, version);
    }

    fn retain(&self, identities: &HashSet<DisplayFileIdentity>) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|identity, _| identities.contains(identity));
    }
}

fn display_file_identity(path: &Path) -> Option<DisplayFileIdentity> {
    let metadata = fs::metadata(path).ok()?;
    Some(DisplayFileIdentity {
        canonical_path: path.canonicalize().ok()?,
        byte_len: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

pub(crate) fn discover_installed_harness_presence() -> Vec<InstalledHarness> {
    candidates::production_present_candidates(KNOWN_HARNESS_SPECS)
        .into_iter()
        .map(|(id, candidate)| InstalledHarness {
            id,
            executable: candidate.executable,
            version: None,
        })
        .collect()
}

pub(crate) fn enrich_installed_harness_versions(
    present: &[InstalledHarness],
    cache: &DisplayVersionCache,
) -> Vec<InstalledHarness> {
    let identities = present
        .iter()
        .filter_map(|harness| display_file_identity(&harness.executable))
        .collect::<HashSet<_>>();
    cache.retain(&identities);
    std::thread::scope(|scope| {
        let probes = present
            .iter()
            .cloned()
            .map(|harness| {
                scope.spawn(move || {
                    let spec = known_harness_spec(harness.id);
                    let target =
                        spec.and_then(|spec| probe_target(spec.executable, &harness.executable));
                    let identity = target.as_deref().and_then(display_file_identity);
                    let version = identity
                        .as_ref()
                        .and_then(|identity| cache.version(identity))
                        .or_else(|| {
                            let version = spec.zip(target.as_deref()).and_then(|(spec, target)| {
                                let output = run_version_command(target, ProbeLimits::PRODUCTION)?;
                                parse_harness_display_version(spec.executable, &output.stdout)
                                    .or_else(|| {
                                        parse_harness_display_version(
                                            spec.executable,
                                            &output.stderr,
                                        )
                                    })
                                    .map(|version| version.to_string())
                            });
                            // A failed probe is not remembered: the first run of
                            // a just-updated CLI can miss the deadline, and a
                            // refresh must try it again.
                            if let (Some(identity), Some(version)) = (identity, &version) {
                                cache.insert(identity, version.clone());
                            }
                            version
                        });
                    InstalledHarness { version, ..harness }
                })
            })
            .collect::<Vec<_>>();
        probes
            .into_iter()
            .filter_map(|probe| probe.join().ok())
            .collect()
    })
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
