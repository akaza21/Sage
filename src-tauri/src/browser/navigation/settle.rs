//! Settling a tab when its native navigation completes. See `outcome` for
//! how a completion is classified.

use std::collections::VecDeque;

use super::outcome::{Commit, Completion, Outcome, Settled, classify, failure_reason};
use crate::browser::{BrowserTabs, admit_url};

/// Native navigation events remembered per tab for diagnosis (ids, flags and
/// statuses only; no URLs).
const MAX_TRACE: usize = 16;

/// What the tab showed before a navigation started, so a navigation that
/// never commits (a download, a cancel, a Stop) can put it back.
struct PageRestore {
    title: Option<String>,
    favicon: Option<String>,
    error: Option<&'static str>,
}

/// Evidence gathered while one navigation loads.
#[derive(Default)]
pub(in crate::browser) struct NavigationEvidence {
    restore: Option<PageRestore>,
    /// `ContentLoading` for this navigation id: a document committed.
    commit: Option<Commit>,
    /// A navigation this one superseded (or a Back/Forward attempt this
    /// retry follows) already committed: the page changed since the last
    /// settle, so this navigation can never be abandoned.
    page_changed: Option<Commit>,
    /// The admitted URL on screen while the tab reports a failure; kept
    /// across settles so a later abandon compares against it.
    shown_failure: Option<String>,
    /// Bounded event trace; kept across settles.
    trace: VecDeque<String>,
}

impl NavigationEvidence {
    fn record(&mut self, event: String) {
        if self.trace.len() == MAX_TRACE {
            self.trace.pop_front();
        }
        self.trace.push_back(event);
    }
}

impl<T> BrowserTabs<T> {
    /// Called by `start_navigation` before it clears the page chrome. A
    /// navigation that supersedes one still loading keeps the first restore
    /// record; everything else is per navigation.
    pub(in crate::browser) fn begin_evidence(&mut self, label: &str, id: u64) {
        if let Some(tab) = self.entries.get_mut(label) {
            let page_changed = if tab.loading {
                tab.evidence.commit.or(tab.evidence.page_changed)
            } else {
                None
            };
            let restore = if tab.loading {
                // Nothing to restore once a superseded navigation committed.
                tab.evidence
                    .restore
                    .take()
                    .filter(|_| page_changed.is_none())
            } else {
                Some(PageRestore {
                    title: tab.title.clone(),
                    favicon: tab.favicon.clone(),
                    error: tab.last_error,
                })
            };
            tab.evidence = NavigationEvidence {
                restore,
                page_changed,
                shown_failure: tab.evidence.shown_failure.take(),
                trace: std::mem::take(&mut tab.evidence.trace),
                ..NavigationEvidence::default()
            };
            tab.evidence.record(format!(
                "start {id}{}",
                if tab.loading { " superseding" } else { "" }
            ));
        }
    }

    /// `ContentLoading` for `id`: a new document (or error page) committed.
    pub(in crate::browser) fn note_content_loading(&mut self, label: &str, id: u64, error: bool) {
        if let Some(tab) = self.entries.get_mut(label) {
            let current = tab.loading && tab.navigation_id == Some(id);
            tab.evidence.record(format!(
                "content {id} error_page={error}{}",
                if current { "" } else { " stale" }
            ));
            if current {
                tab.evidence.commit = Some(if error {
                    Commit::ErrorPage
                } else {
                    Commit::Document
                });
            }
        }
    }

    /// `DownloadStarting` on this tab (allowed or denied). Diagnostic only:
    /// a navigation that became a download commits nothing, which the
    /// missing `ContentLoading` already says.
    pub(in crate::browser) fn note_download(&mut self, label: &str) {
        if let Some(tab) = self.entries.get_mut(label) {
            let event = format!(
                "download loading={} navigation={:?}",
                tab.loading, tab.navigation_id
            );
            tab.evidence.record(event);
        }
    }

    #[cfg(any(test, feature = "native-lane"))]
    pub(in crate::browser) fn navigation_trace(&self, label: &str) -> Vec<String> {
        self.entries
            .get(label)
            .map(|tab| tab.evidence.trace.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(in crate::browser) fn settle_navigation(
        &mut self,
        label: &str,
        navigation_id: u64,
        completion: Completion<'_>,
    ) -> Option<Settled> {
        let tab = self.entries.get_mut(label)?;
        let event = format!(
            "complete {navigation_id} success={} web_error={}",
            completion.success, completion.web_error
        );
        if tab.navigation_id != Some(navigation_id) || !tab.loading {
            tab.evidence.record(format!("{event} stale"));
            return None;
        }
        // Only this id's own evidence may count as a user navigation landing.
        let own_commit = completion.success || tab.evidence.commit == Some(Commit::Document);
        let commit = tab.evidence.commit.or(tab.evidence.page_changed);
        let mut outcome = classify(completion, commit);
        let shown = completion.source.filter(|source| admit_url(source).is_ok());
        // Set when nothing committed by our evidence, yet the screen moved:
        // the tab reports what is on screen together with the failure.
        let mut unexplained_failure = None;
        if outcome == Outcome::Abandoned && !abandon_consistent(tab, shown) {
            // Whatever the events said, the screen does not show the page
            // the tab would restore: never report the old address. A failed
            // load that left another page on screen without any commit
            // evidence (a certificate interstitial, say) is still a failure.
            let reason = failure_reason(completion.web_error);
            unexplained_failure = Some(reason);
            outcome = shown.map_or(Outcome::Failed(reason), Outcome::Committed);
        }
        let settled = match outcome {
            Outcome::Committed(url) => self
                .finish_navigation(label, navigation_id, Some(url))
                .then_some(if own_commit {
                    Settled::Committed
                } else {
                    Settled::Adopted
                }),
            Outcome::Failed(reason) => self
                .fail_navigation(label, navigation_id, reason)
                .then_some(Settled::Failed),
            Outcome::Abandoned => {
                self.abandon_navigation(label, navigation_id, completion.source);
                Some(Settled::Abandoned)
            }
        };
        let Some(tab) = self.entries.get_mut(label) else {
            return settled;
        };
        if let Some(reason) = unexplained_failure
            && settled == Some(Settled::Adopted)
            && !tab.loading
            && tab.last_error.is_none()
        {
            tab.last_error = Some(reason);
        }
        tab.evidence.record(format!("{event} -> {settled:?}"));
        if !tab.loading {
            let shown_failure = if tab.last_error.is_none() {
                None
            } else if settled == Some(Settled::Abandoned) {
                tab.evidence.shown_failure.take()
            } else {
                shown.map(str::to_owned)
            };
            tab.evidence = NavigationEvidence {
                shown_failure,
                trace: std::mem::take(&mut tab.evidence.trace),
                ..NavigationEvidence::default()
            };
        } else {
            // A Back/Forward fallback is pending; if it aborts, the tab
            // reports a failure while this page stays on screen.
            tab.evidence.shown_failure = shown.map(str::to_owned);
        }
        settled
    }

    fn fail_navigation(&mut self, label: &str, navigation_id: u64, reason: &'static str) -> bool {
        if !self.finish_navigation(label, navigation_id, None) {
            return false;
        }
        if let Some(tab) = self.entries.get_mut(label)
            && !tab.loading
            && tab.last_error.is_none()
        {
            tab.last_error = Some(reason);
        }
        true
    }

    /// Nothing committed: the tab returns to the page it showed before,
    /// with its history cursor and chrome. A same-document change the page
    /// made while the navigation was pending (hash, pushState), which
    /// `source_changed` deliberately ignored, is adopted as one now.
    fn abandon_navigation(&mut self, label: &str, navigation_id: u64, source: Option<&str>) {
        let Some(tab) = self.entries.get_mut(label) else {
            return;
        };
        tab.history.abandon(navigation_id);
        tab.fallback_url = None;
        tab.loading = false;
        if let Some(restore) = tab.evidence.restore.take() {
            tab.title = restore.title;
            tab.favicon = restore.favicon;
            tab.last_error = restore.error;
        }
        if tab.last_error.is_none()
            && let (Some(source), Some(committed)) = (source, tab.committed_url.as_deref())
            && source != committed
            && let (Ok(next), Ok(current)) = (admit_url(source), admit_url(committed))
            && next.origin() == current.origin()
        {
            // No traversal is pending after `abandon`, so this appends.
            let _ = tab.history.same_document(source);
            tab.committed_url = Some(source.to_owned());
            self.browsing_history
                .record_same_document(label, source, tab.title.as_deref());
        }
        tab.revision = tab.revision.wrapping_add(1);
        tab.document_revision = tab.document_revision.wrapping_add(1);
    }
}

/// An abandon may only leave on screen what the tab will report: the
/// failure it shows, its committed URL, a same-origin same-document change
/// of it, or nothing admitted on a tab with no committed page.
fn abandon_consistent<T>(tab: &crate::browser::BrowserTab<T>, shown: Option<&str>) -> bool {
    let error = tab
        .evidence
        .restore
        .as_ref()
        .map_or(tab.last_error, |restore| restore.error);
    if error.is_some() {
        return shown.is_none() || shown == tab.evidence.shown_failure.as_deref();
    }
    match (shown, tab.committed_url.as_deref()) {
        (None, None) => true,
        (Some(shown), Some(committed)) => {
            shown == committed
                || matches!(
                    (admit_url(shown), admit_url(committed)),
                    (Ok(next), Ok(current)) if next.origin() == current.origin()
                )
        }
        _ => false,
    }
}
