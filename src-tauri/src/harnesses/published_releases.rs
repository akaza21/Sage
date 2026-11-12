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
    releases: Vec<PublishedRelease>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PublishedReleases(Arc<RwLock<Checked>>);

impl PublishedReleases {
    /// Claims the next check when none has started within the interval, so
    /// overlapping refreshes run one lookup.
    pub(super) fn begin_check(&self) -> bool {
        let mut checked = self.0.write().unwrap_or_else(|error| error.into_inner());
        if checked
            .started
            .is_some_and(|started| started.elapsed() < CHECK_INTERVAL)
        {
            return false;
        }
        checked.started = Some(Instant::now());
        true
    }

    pub(super) fn replace(&self, releases: Vec<PublishedRelease>) {
        self.0
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .releases = releases;
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

    fn published(version: &str) -> PublishedReleases {
        let releases = PublishedReleases::default();
        releases.replace(vec![PublishedRelease {
            harness_id: "codex",
            version: version.into(),
            command: "npm install -g @openai/codex@latest".into(),
        }]);
        releases
    }

    #[test]
    fn an_update_shows_only_while_the_installed_release_is_older() {
        let releases = published("0.168.0");
        let update = releases.update_for("codex", Some("0.167.1")).unwrap();
        assert_eq!(update.latest_version, "0.168.0");
        assert_eq!(update.command, "npm install -g @openai/codex@latest");
        for installed in [Some("0.168.0"), Some("0.169.0"), Some("unknown"), None] {
            assert!(
                releases.update_for("codex", installed).is_none(),
                "{installed:?}"
            );
        }
        assert!(releases.update_for("pi", Some("0.74.2")).is_none());
    }

    #[test]
    fn one_check_runs_per_interval() {
        let releases = PublishedReleases::default();
        assert!(releases.begin_check());
        assert!(!releases.begin_check());
    }
}
