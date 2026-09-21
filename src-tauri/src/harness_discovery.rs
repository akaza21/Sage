//! Bounded, read-only discovery of user-installed harness CLIs.
//!
//! Discovery is separate from protocol characterisation and launch admission.
//! A validated executable whose version cannot be read remains truthfully
//! present-but-unreadable rather than becoming absent or acquiring a guess.

use std::{path::Path, path::PathBuf, time::Duration};

mod candidates;
mod claude;
mod parsing;
mod validation;
mod version_probe;

#[cfg(test)]
mod tests;

use candidates::{legacy_candidates, production_candidates};
pub(crate) use claude::{
    ClaudeLaunchDiscovery, DiscoveredClaudeLaunch, discover_claude_launch, sha256_file,
};
use parsing::{normalized_legacy_version, parse_harness_version};
use validation::{authentic_harness_file as validate_harness_file, legacy_basenames};
use version_probe::run_version_command;

/// At most this many `PATH` directories are inspected for one harness.
pub(crate) const PATH_DIR_CAP: usize = 16;
/// The probe rejects paths whose platform-native byte representation is larger
/// than this value.
pub(crate) const PATH_BYTES_MAX: usize = 32 * 1024;
pub(super) const KNOWN_INSTALL_ROOT_CAP: usize = 4;
pub(super) const VERSION_OUTPUT_BYTES_MAX: usize = 16 * 1024;
pub(super) const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
pub(super) const VERSION_REAP_TIMEOUT: Duration = Duration::from_millis(250);
pub(super) const VERSION_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The two harness executables covered by host discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HarnessExecutable {
    Codex,
    Claude,
}

impl HarnessExecutable {
    pub(super) fn identifier(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude-code",
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredHarness {
    pub(crate) executable: PathBuf,
    pub(crate) version: HarnessVersion,
    pub(crate) source: DiscoverySource,
}

/// The complete truthful result of inspecting one named harness.
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

/// Discovers Codex or Claude using only bounded, validated candidate paths.
///
/// The worst case for one call is inspection of 16 `PATH` roots plus four
/// known-install roots, followed by one version probe capped at 2 seconds and
/// a 250 ms reap attempt. Stdout and stderr retain at most 16 KiB each.
pub(crate) fn discover_harness(harness: HarnessExecutable) -> HarnessDiscovery {
    discover_from_candidates(
        harness,
        production_candidates(harness),
        ProbeLimits::PRODUCTION,
    )
}

/// Shared validation used by legacy Codex host-presence reporting. This checks
/// the same path and link discipline as full discovery without running a child.
pub(crate) fn authentic_harness_file(path: &Path, expected_basename: &str) -> bool {
    validate_harness_file(path, expected_basename)
}

/// Returns candidates formed from the first bounded set of valid PATH roots.
/// Kept public within the crate so Codex's compatibility presence API can
/// retain its exact historical PATH-only candidate set.
pub(crate) fn path_candidates_for_basename(expected_basename: &str) -> Vec<PathBuf> {
    candidates::path_candidates_for_basename(expected_basename)
}

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

    let Some(output) = run_version_command(&candidate.executable, limits) else {
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

// The pre-HR-1 projection still consumes the legacy installed-harness shape.
// Keep that adapter here so it shares candidate validation and the bounded
// version process rather than growing a parallel discovery path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledHarness {
    pub(crate) id: &'static str,
    pub(crate) executable: PathBuf,
    pub(crate) version: String,
}

#[derive(Clone, Copy)]
enum InstalledHarnessSpec {
    Shared(HarnessExecutable),
    Legacy {
        id: &'static str,
        command: &'static str,
    },
}

const INSTALLED_HARNESS_SPECS: &[InstalledHarnessSpec] = &[
    InstalledHarnessSpec::Shared(HarnessExecutable::Codex),
    InstalledHarnessSpec::Shared(HarnessExecutable::Claude),
    InstalledHarnessSpec::Legacy {
        id: "oh-my-pi",
        command: "omp",
    },
    InstalledHarnessSpec::Legacy {
        id: "pi",
        command: "pi",
    },
];

pub(crate) fn discover_installed_harnesses() -> Vec<InstalledHarness> {
    INSTALLED_HARNESS_SPECS
        .iter()
        .filter_map(|spec| match *spec {
            InstalledHarnessSpec::Shared(harness) => match discover_harness(harness) {
                HarnessDiscovery::Present(discovered) => Some(InstalledHarness {
                    id: harness.identifier(),
                    executable: discovered.executable,
                    version: discovered.version.to_string(),
                }),
                HarnessDiscovery::Absent | HarnessDiscovery::PresentVersionUnreadable { .. } => {
                    None
                }
            },
            InstalledHarnessSpec::Legacy { id, command } => discover_legacy(id, command),
        })
        .collect()
}

fn discover_legacy(id: &'static str, command: &'static str) -> Option<InstalledHarness> {
    let candidate = legacy_candidates(command).into_iter().find(|candidate| {
        legacy_basenames(command)
            .iter()
            .any(|basename| validate_harness_file(&candidate.executable, basename))
    })?;
    let output = run_version_command(&candidate.executable, ProbeLimits::PRODUCTION)?;
    let version = normalized_legacy_version(&output.stdout)
        .or_else(|| normalized_legacy_version(&output.stderr))?;
    Some(InstalledHarness {
        id,
        executable: candidate.executable,
        version,
    })
}
