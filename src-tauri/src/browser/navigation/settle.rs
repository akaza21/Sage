use std::collections::VecDeque;

use super::outcome::{Commit, Completion, Outcome, Settled, classify, failure_reason};
use crate::browser::{BrowserTabs, admit_url};

/// Trace entries hold ids, flags and statuses only; no URLs.
const MAX_TRACE: usize = 16;

struct PageRestore {
    title: Option<String>,
    favicon: Option<String>,
    error: Option<&'static str>,
}

#[derive(Default)]
pub(in crate::browser) struct NavigationEvidence {
    restore: Option<PageRestore>,
    commit: Option<Commit>,
    /// A navigation this one superseded (or a Back/Forward attempt this
    /// retry follows) already committed: the page changed since the last
    /// settle, so this navigation can never be abandoned.
    page_changed: Option<Commit>,
    /// The admitted URL on screen while the tab reports a failure; kept
    /// across settles so a later abandon compares against it.
    shown_failure: Option<String>,
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
    /// A navigation that supersedes one still loading keeps the first restore
    /// record.
    pub(in crate::browser) fn begin_evidence(&mut self, label: &str, id: u64) {
        if let Some(tab) = self.entries.get_mut(label) {
            let page_changed = if tab.loading {
                tab.evidence.commit.or(tab.evidence.page_changed)
            } else {
                None
            };
            let restore = if tab.loading {
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
                .commit_navigation(
                    label,
                    navigation_id,
                    Some(url),
                    unexplained_failure.is_none(),
                )
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

    /// A same-document change the page made while the navigation was pending,
    /// which `source_changed` deliberately ignored, is adopted as one now.
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
            crate::browser::commit::record_same_document(
                &self.browsing_history,
                label,
                tab,
                source,
            );
            tab.committed_url = Some(source.to_owned());
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
