//! The newest published release of each installed harness, from the last
//! update check, kept only in memory.

use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use crate::{
    acp::bindings::release_version::ReleaseVersion, domain::HarnessUpdate,
    harness_discovery::PublishedRelease,
};

/// How long one update check stands before a refresh checks again.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Default)]
struct Checked {
    started: Option<Instant>,
    /// The harnesses the current check covers.
    covered: Vec<&'static str>,
    releases: Vec<PublishedRelease>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PublishedReleases(Arc<RwLock<Checked>>);

impl PublishedReleases {
    /// Claims a check of `harness_ids` unless one covering them started within
    /// the interval, so overlapping refreshes run one lookup and a harness
    /// installed since the last check is looked up on the next refresh.
    pub(super) fn begin_check(&self, harness_ids: &[&'static str]) -> bool {
        let mut checked = self.0.write().unwrap_or_else(|error| error.into_inner());
        let recent = checked
            .started
            .is_some_and(|started| started.elapsed() < CHECK_INTERVAL);
        if harness_ids.is_empty()
            || (recent && harness_ids.iter().all(|id| checked.covered.contains(id)))
        {
            return false;
        }
        checked.started = Some(Instant::now());
        checked.covered = harness_ids.to_vec();
        true
    }

    /// Records what a check found. A failed check leaves the earlier results
    /// and lets the next refresh try again; a harness whose lookup failed
    /// keeps its earlier result.
    pub(super) fn finish_check(&self, found: Option<Vec<PublishedRelease>>) {
        let mut checked = self.0.write().unwrap_or_else(|error| error.into_inner());
        let Some(found) = found else {
            checked.started = None;
            return;
        };
        for release in found {
            checked
                .releases
                .retain(|known| known.harness_id != release.harness_id);
            checked.releases.push(release);
        }
    }

    /// An update only while the published release is later than the one
    /// installed now, so updating and refreshing clears it.
    pub(super) fn update_for(
        &self,
        harness_id: &str,
        installed: Option<&str>,
    ) -> Option<HarnessUpdate> {
        let installed = ReleaseVersion::parse(installed?)?;
        let checked = self.0.read().unwrap_or_else(|error| error.into_inner());
        let release = checked
            .releases
            .iter()
            .find(|release| release.harness_id == harness_id)?;
        (ReleaseVersion::parse(&release.version)? > installed).then(|| HarnessUpdate {
            latest_version: release.version.clone(),
            command: release.command.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(harness_id: &'static str, version: &str) -> PublishedRelease {
        PublishedRelease {
            harness_id,
            version: version.into(),
            command: format!("npm install -g {harness_id}@latest"),
        }
    }

    fn published(version: &str) -> PublishedReleases {
        let releases = PublishedReleases::default();
        assert!(releases.begin_check(&["codex"]));
        releases.finish_check(Some(vec![release("codex", version)]));
        releases
    }

    #[test]
    fn an_update_shows_only_while_the_installed_release_is_older() {
        let releases = published("0.168.0");
        let update = releases.update_for("codex", Some("0.167.1")).unwrap();
        assert_eq!(update.latest_version, "0.168.0");
        assert_eq!(update.command, "npm install -g codex@latest");
        for installed in [Some("0.168.0"), Some("0.169.0"), Some("unknown"), None] {
            assert!(
                releases.update_for("codex", installed).is_none(),
                "{installed:?}"
            );
        }
        assert!(releases.update_for("pi", Some("0.74.2")).is_none());
    }

    #[test]
    fn one_check_runs_per_interval_unless_a_harness_is_new() {
        let releases = PublishedReleases::default();
        assert!(!releases.begin_check(&[]));
        assert!(releases.begin_check(&["codex"]));
        assert!(!releases.begin_check(&["codex"]));
        // Pi was installed after the last check.
        assert!(releases.begin_check(&["codex", "pi"]));
        assert!(!releases.begin_check(&["pi"]));
    }

    #[test]
    fn a_failed_check_keeps_what_was_known_and_can_run_again() {
        let releases = published("0.168.0");
        assert!(releases.begin_check(&["codex", "pi"]));
        releases.finish_check(None);
        assert!(releases.update_for("codex", Some("0.167.1")).is_some());
        assert!(releases.begin_check(&["codex", "pi"]));
        // Only Pi's lookup succeeded: Codex keeps its earlier result.
        releases.finish_check(Some(vec![release("pi", "0.75.0")]));
        assert!(releases.update_for("codex", Some("0.167.1")).is_some());
        assert!(releases.update_for("pi", Some("0.74.2")).is_some());
    }
}
