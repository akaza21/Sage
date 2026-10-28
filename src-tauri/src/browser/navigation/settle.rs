//! Settling a tab when its native navigation completes. See `outcome` for
//! how a completion is classified.

use super::outcome::{Completion, Outcome, Settled, classify};
use crate::browser::BrowserTabs;

/// What the tab showed before a navigation started, so a navigation that
/// never commits (a download, a cancel) can put it back.
pub(in crate::browser) struct PageRestore {
    title: Option<String>,
    favicon: Option<String>,
    error: Option<&'static str>,
}

impl<T> BrowserTabs<T> {
    /// Called when a navigation starts on a settled tab: remembers the page
    /// chrome the start is about to clear. A navigation that supersedes one
    /// still loading keeps the first record.
    pub(in crate::browser) fn remember_page(&mut self, label: &str) {
        if let Some(tab) = self.entries.get_mut(label)
            && !tab.loading
        {
            tab.restore = Some(PageRestore {
                title: tab.title.clone(),
                favicon: tab.favicon.clone(),
                error: tab.last_error,
            });
        }
    }

    /// `DownloadStarting` fired for this tab: the navigation now loading, if
    /// any, became a download (allowed or denied) and will not commit.
    pub(in crate::browser) fn note_download(&mut self, label: &str) {
        if let Some(tab) = self.entries.get_mut(label)
            && tab.loading
        {
            tab.download_navigation = tab.navigation_id;
        }
    }

    pub(in crate::browser) fn settle_navigation(
        &mut self,
        label: &str,
        navigation_id: u64,
        completion: Completion<'_>,
    ) -> Option<Settled> {
        let tab = self.entries.get(label)?;
        if tab.navigation_id != Some(navigation_id) || !tab.loading {
            return None;
        }
        let became_download = tab.download_navigation == Some(navigation_id);
        let previous = tab.committed_url.clone();
        let settled = match classify(completion, previous.as_deref(), became_download) {
            Outcome::Committed(url) => self
                .finish_navigation(label, navigation_id, Some(url))
                .then_some(Settled::Committed),
            Outcome::Failed(reason) => self
                .fail_navigation(label, navigation_id, reason)
                .then_some(Settled::Failed),
            Outcome::Abandoned => {
                self.abandon_navigation(label, navigation_id);
                Some(Settled::Abandoned)
            }
        };
        if let Some(tab) = self.entries.get_mut(label)
            && !tab.loading
        {
            tab.restore = None;
            tab.download_navigation = None;
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
    /// with its URL, history cursor and chrome unchanged.
    fn abandon_navigation(&mut self, label: &str, navigation_id: u64) {
        if let Some(tab) = self.entries.get_mut(label) {
            tab.history.abandon(navigation_id);
            tab.fallback_url = None;
            tab.loading = false;
            if let Some(restore) = tab.restore.take() {
                tab.title = restore.title;
                tab.favicon = restore.favicon;
                tab.last_error = restore.error;
            }
            tab.revision = tab.revision.wrapping_add(1);
        }
    }
}
