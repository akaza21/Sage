//! Settling a tab when its native navigation completes. See `outcome` for
//! how a completion is classified.

use super::outcome::{Commit, Completion, Outcome, Settled, classify};
use crate::browser::{BrowserTabs, admit_url};

/// Redirect hops remembered per navigation, for download correlation.
const MAX_NAVIGATION_URIS: usize = 16;

/// What the tab showed before a navigation started, so a navigation that
/// never commits (a download, a cancel) can put it back.
struct PageRestore {
    title: Option<String>,
    favicon: Option<String>,
    error: Option<&'static str>,
}

/// Evidence gathered while one navigation loads.
#[derive(Default)]
pub(in crate::browser) struct NavigationEvidence {
    restore: Option<PageRestore>,
    /// The navigation's URI and its redirect hops (bounded).
    uris: Vec<String>,
    /// `DownloadStarting` named one of `uris` during this navigation.
    became_download: bool,
    /// `ContentLoading` for this navigation id: a document committed.
    commit: Option<Commit>,
}

impl<T> BrowserTabs<T> {
    /// Called by `start_navigation` before it clears the page chrome. A
    /// navigation that supersedes one still loading keeps the first restore
    /// record; everything else is per navigation.
    pub(in crate::browser) fn begin_evidence(&mut self, label: &str, _id: u64, uri: &str) {
        if let Some(tab) = self.entries.get_mut(label) {
            let restore = if tab.loading {
                tab.evidence.restore.take()
            } else {
                Some(PageRestore {
                    title: tab.title.clone(),
                    favicon: tab.favicon.clone(),
                    error: tab.last_error,
                })
            };
            tab.evidence = NavigationEvidence {
                restore,
                uris: vec![uri.to_owned()],
                ..NavigationEvidence::default()
            };
        }
    }

    /// `NavigationStarting` again for the loading id: a redirect hop.
    pub(in crate::browser) fn note_redirect(&mut self, label: &str, id: u64, uri: &str) {
        if let Some(tab) = self.entries.get_mut(label)
            && tab.loading
            && tab.navigation_id == Some(id)
            && tab.evidence.uris.len() < MAX_NAVIGATION_URIS
            && !tab.evidence.uris.iter().any(|known| known == uri)
        {
            tab.evidence.uris.push(uri.to_owned());
        }
    }

    /// `ContentLoading` for `id`: a new document (or error page) committed.
    pub(in crate::browser) fn note_content_loading(&mut self, label: &str, id: u64, error: bool) {
        if let Some(tab) = self.entries.get_mut(label)
            && tab.loading
            && tab.navigation_id == Some(id)
        {
            tab.evidence.commit = Some(if error {
                Commit::ErrorPage
            } else {
                Commit::Document
            });
        }
    }

    /// `DownloadStarting` for `uri` (allowed or denied). Only a download of
    /// the loading navigation's own URI or redirect hop counts: `<a
    /// download>`, blob and subframe downloads are not this navigation.
    pub(in crate::browser) fn note_download(&mut self, label: &str, uri: &str) {
        if let Some(tab) = self.entries.get_mut(label)
            && tab.loading
            && tab
                .evidence
                .uris
                .iter()
                .any(|known| without_fragment(known) == without_fragment(uri))
        {
            tab.evidence.became_download = true;
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
        let outcome = classify(
            completion,
            tab.evidence.commit,
            tab.evidence.became_download,
        );
        let settled = match outcome {
            Outcome::Committed(url) => self
                .finish_navigation(label, navigation_id, Some(url))
                .then_some(Settled::Committed),
            Outcome::Failed(reason) => self
                .fail_navigation(label, navigation_id, reason)
                .then_some(Settled::Failed),
            Outcome::Abandoned => {
                self.abandon_navigation(label, navigation_id, completion.source);
                Some(Settled::Abandoned)
            }
        };
        if let Some(tab) = self.entries.get_mut(label)
            && !tab.loading
        {
            tab.evidence = NavigationEvidence::default();
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
        }
        tab.revision = tab.revision.wrapping_add(1);
    }
}

/// Fragments never reach the network, so a download URI may lack one.
fn without_fragment(uri: &str) -> &str {
    uri.split_once('#').map_or(uri, |(before, _)| before)
}
