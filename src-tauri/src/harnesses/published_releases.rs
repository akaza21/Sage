use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use crate::{acp::bindings::release_version::ReleaseVersion, harness_discovery::PublishedRelease};
use sage_domain::harness::HarnessUpdate;

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Default)]
struct Checked {
    /// Bumped by `forget`, so a check begun before it cannot write back.
    epoch: u64,
    started: Option<Instant>,
    covered: Vec<&'static str>,
    releases: Vec<PublishedRelease>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PublishedReleases(Arc<RwLock<Checked>>);

impl PublishedReleases {
    pub(super) fn check_current(&self, epoch: u64) -> bool {
        self.0
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .epoch
            == epoch
    }

    /// `None` while a check started within the interval already covers them;
    /// otherwise the epoch `finish_check` must present.
    pub(super) fn begin_check(&self, harness_ids: &[&'static str]) -> Option<u64> {
        let mut checked = self.0.write().unwrap_or_else(|error| error.into_inner());
        let recent = checked
            .started
            .is_some_and(|started| started.elapsed() < CHECK_INTERVAL);
        if harness_ids.is_empty()
            || (recent && harness_ids.iter().all(|id| checked.covered.contains(id)))
        {
            return None;
        }
        checked.started = Some(Instant::now());
        checked.covered = harness_ids.to_vec();
        Some(checked.epoch)
    }

    /// `None` (failed) keeps earlier results and lets the next refresh retry; a
    /// harness missing from `found` keeps its earlier result and is retried too.
    /// A check begun before the last `forget` is dropped.
    pub(super) fn finish_check(&self, epoch: u64, found: Option<Vec<PublishedRelease>>) {
        let mut checked = self.0.write().unwrap_or_else(|error| error.into_inner());
        if checked.epoch != epoch {
            return;
        }
        let Some(found) = found else {
            checked.started = None;
            return;
        };
        checked
            .covered
            .retain(|id| found.iter().any(|release| release.harness_id == *id));
        for release in found {
            checked
                .releases
                .retain(|known| known.harness_id != release.harness_id);
            checked.releases.push(release);
        }
    }

    /// Turning checks off drops what earlier checks found, so nothing stale
    /// shows, and lets the next enabled refresh check at once. A check still
    /// running belongs to the old epoch and cannot write back.
    pub(super) fn forget(&self) {
        let mut checked = self.0.write().unwrap_or_else(|error| error.into_inner());
        *checked = Checked {
            epoch: checked.epoch.wrapping_add(1),
            ..Checked::default()
        };
    }

    pub(super) fn latest_for(&self, harness_id: &str) -> Option<String> {
        let checked = self.0.read().unwrap_or_else(|error| error.into_inner());
        checked
            .releases
            .iter()
            .find(|release| release.harness_id == harness_id)
            .map(|release| release.version.clone())
    }

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
            unlocks: Vec::new(),
        })
    }
}

impl super::HarnessCatalog {
    /// What a check begun at `epoch` asks before each npm lookup: the switch,
    /// read again each time, is on, and turning it off since (even back on)
    /// has not replaced the check's epoch.
    pub(crate) fn update_check_live<'a>(
        &'a self,
        epoch: u64,
        enabled: impl Fn() -> bool + 'a,
    ) -> impl Fn() -> bool + 'a {
        move || enabled() && self.published.check_current(epoch)
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
        let epoch = releases.begin_check(&["codex"]).unwrap();
        releases.finish_check(epoch, Some(vec![release("codex", version)]));
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
        assert!(releases.begin_check(&[]).is_none());
        assert!(releases.begin_check(&["codex"]).is_some());
        assert!(releases.begin_check(&["codex"]).is_none());
        assert!(releases.begin_check(&["codex", "pi"]).is_some());
        assert!(releases.begin_check(&["pi"]).is_none());
    }

    #[test]
    fn a_failed_check_keeps_what_was_known_and_can_run_again() {
        let releases = published("0.168.0");
        let epoch = releases.begin_check(&["codex", "pi"]).unwrap();
        releases.finish_check(epoch, None);
        assert!(releases.update_for("codex", Some("0.167.1")).is_some());
        let epoch = releases.begin_check(&["codex", "pi"]).unwrap();
        releases.finish_check(epoch, Some(vec![release("pi", "0.75.0")]));
        assert!(releases.update_for("codex", Some("0.167.1")).is_some());
        assert!(releases.update_for("pi", Some("0.74.2")).is_some());
        assert!(releases.begin_check(&["pi"]).is_none());
        assert!(releases.begin_check(&["codex", "pi"]).is_some());
    }

    #[test]
    fn a_check_begun_before_checking_turned_off_writes_nothing_back() {
        let releases = PublishedReleases::default();
        let registry = crate::acp::bindings::registry::BindingRegistry::default();
        let mut definition = super::super::catalog_definition::definition(
            "codex",
            "Codex",
            "ACP",
            "available",
            "builtIn",
            super::super::catalog_definition::CuratedProtocol::acp(Some("codex-app-server")),
        );
        definition.version = Some("0.167.1".into());
        let epoch = releases.begin_check(&["codex"]).unwrap();
        releases.forget();
        let current_epoch = releases.begin_check(&["codex"]).unwrap();
        releases.finish_check(epoch, Some(vec![release("codex", "0.168.0")]));
        super::super::version_details::annotate(&mut definition, None, &releases, &registry);
        assert_eq!(definition.latest_version, None);
        assert_eq!(definition.update, None);
        releases.finish_check(current_epoch, Some(vec![release("codex", "0.168.0")]));
        super::super::version_details::annotate(&mut definition, None, &releases, &registry);
        assert_eq!(definition.latest_version.as_deref(), Some("0.168.0"));
        assert_eq!(definition.update.unwrap().latest_version, "0.168.0");
    }
}
