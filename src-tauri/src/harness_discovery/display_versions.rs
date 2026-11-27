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

#[cfg(test)]
pub(crate) fn enrich_installed_harness_versions(
    present: &[InstalledHarness],
    cache: &DisplayVersionCache,
) -> Vec<InstalledHarness> {
    enrich_installed_harness_versions_each(present, cache, |_| {})
}

/// Probes every present harness at once and hands each result to `on_each`
/// as soon as that harness answers, so a slow `--version` (a Node CLI's cold
/// start) never holds back the harnesses that already answered.
pub(crate) fn enrich_installed_harness_versions_each(
    present: &[InstalledHarness],
    cache: &DisplayVersionCache,
    on_each: impl Fn(&InstalledHarness) + Sync,
) -> Vec<InstalledHarness> {
    let identities = present
        .iter()
        .filter_map(|harness| display_file_identity(&harness.executable))
        .collect::<HashSet<_>>();
    cache.retain(&identities);
    each_probed(present, |harness| display_version(harness, cache), on_each)
}

pub(super) fn each_probed(
    present: &[InstalledHarness],
    probe: impl Fn(&InstalledHarness) -> Option<String> + Sync,
    on_each: impl Fn(&InstalledHarness) + Sync,
) -> Vec<InstalledHarness> {
    let (probe, on_each) = (&probe, &on_each);
    std::thread::scope(|scope| {
        let probes = present
            .iter()
            .map(|harness| {
                scope.spawn(move || {
                    let probed = InstalledHarness {
                        version: probe(harness),
                        ..harness.clone()
                    };
                    on_each(&probed);
                    probed
                })
            })
            .collect::<Vec<_>>();
        probes
            .into_iter()
            .filter_map(|probe| probe.join().ok())
            .collect()
    })
}

fn display_version(harness: &InstalledHarness, cache: &DisplayVersionCache) -> Option<String> {
    let spec = known_harness_spec(harness.id);
    let target = spec.and_then(|spec| probe_target(spec.executable, &harness.executable));
    let identity = target.as_deref().and_then(display_file_identity);
    if let Some(version) = identity
        .as_ref()
        .and_then(|identity| cache.version(identity))
    {
        return Some(version);
    }
    let version = spec.zip(target.as_deref()).and_then(|(spec, target)| {
        let output = run_version_command(target, ProbeLimits::PRODUCTION)?;
        parse_harness_display_version(spec.executable, &output.stdout)
            .or_else(|| parse_harness_display_version(spec.executable, &output.stderr))
            .map(|version| version.to_string())
    });
    // A failed probe is not remembered: the first run of a just-updated CLI
    // can miss the deadline, and a refresh must try it again.
    if let (Some(identity), Some(version)) = (identity, &version) {
        cache.insert(identity, version.clone());
    }
    version
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use super::{InstalledHarness, each_probed};

    fn present(id: &'static str) -> InstalledHarness {
        InstalledHarness {
            id,
            executable: PathBuf::from(id),
            version: None,
        }
    }

    #[test]
    fn a_harness_that_answers_is_handed_on_while_a_slow_one_is_still_probing() {
        let (answered, seen) = mpsc::channel();
        let seen = std::sync::Mutex::new(seen);
        let started = Instant::now();
        let probed = each_probed(
            &[present("pi"), present("claude-code")],
            |harness| {
                if harness.id == "pi" {
                    // Pi answers only once Claude Code was handed on first.
                    let first = seen
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(10))
                        .ok();
                    return first.map(|id: &str| format!("after {id}"));
                }
                Some("2.1.0".into())
            },
            |harness| {
                if harness.id == "claude-code" {
                    answered.send(harness.id).unwrap();
                }
            },
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        let version = |id| {
            probed
                .iter()
                .find(|harness| harness.id == id)
                .and_then(|harness| harness.version.clone())
        };
        assert_eq!(version("claude-code").as_deref(), Some("2.1.0"));
        assert_eq!(version("pi").as_deref(), Some("after claude-code"));
    }

    #[test]
    fn a_harness_whose_probe_fails_is_still_handed_on_without_a_version() {
        let handed = std::sync::Mutex::new(Vec::new());
        each_probed(
            &[present("codex")],
            |_| None,
            |harness| handed.lock().unwrap().push(harness.clone()),
        );
        let handed = handed.into_inner().unwrap();
        assert_eq!(handed.len(), 1);
        assert_eq!(handed[0].id, "codex");
        assert!(handed[0].version.is_none());
    }
}
