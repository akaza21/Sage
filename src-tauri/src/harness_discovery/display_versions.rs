use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

use super::{
    InstalledHarness, KNOWN_HARNESS_SPECS, ProbeLimits, candidates, known_harness_spec,
    parse_harness_display_version, probe_target, run_version_command,
};

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
