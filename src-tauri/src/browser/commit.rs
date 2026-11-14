use super::{BrowserTab, BrowserTabs, admit_url, browsing_history::BrowsingHistory};

impl<T> BrowserTabs<T> {
    pub(super) fn finish_navigation(
        &mut self,
        label: &str,
        navigation_id: u64,
        url: Option<&str>,
    ) -> bool {
        self.commit_navigation(label, navigation_id, url, true)
    }

    /// `visit` is unset for a load that failed yet moved the screen: it commits
    /// its URL for the tab but is not a page the user visited.
    pub(super) fn commit_navigation(
        &mut self,
        label: &str,
        navigation_id: u64,
        url: Option<&str>,
        visit: bool,
    ) -> bool {
        let Some(tab) = self.entries.get_mut(label) else {
            return false;
        };
        if tab.navigation_id != Some(navigation_id) || !tab.loading {
            return false;
        }
        if url.is_some_and(|url| admit_url(url).is_err()) {
            return false;
        }
        tab.fallback_url = tab.history.finish(navigation_id, url);
        if tab.history.take_failure() {
            tab.last_error = Some("Back or Forward could not reach the saved page");
            tab.loading = false;
        } else if tab.fallback_url.is_none() {
            let previous = std::mem::replace(&mut tab.committed_url, url.map(str::to_owned));
            // A reload, a redirect back to the page already shown, or
            // entering the address already shown is not a new visit,
            // unless that page's own visit was never recorded (it failed
            // to load, say, or a full queue dropped it).
            if let Some(url) = url
                && visit
                && (previous.as_deref() != Some(url) || tab.visited_url.as_deref() != Some(url))
            {
                let queued = self.browsing_history.record_visit(label, url, None);
                tab.visited_url = queued.then(|| url.to_owned());
            }
            tab.loading = false;
        }
        tab.revision = tab.revision.wrapping_add(1);
        tab.document_revision = tab.document_revision.wrapping_add(1);
        true
    }

    pub(super) fn source_changed(&mut self, label: &str, url: &str) -> bool {
        if admit_url(url).is_err() {
            return false;
        }
        let Some(tab) = self.entries.get_mut(label) else {
            return false;
        };
        if (tab.loading && !tab.history.pending_target_is(url))
            || tab.last_error.is_some()
            || tab.committed_url.is_none()
            || tab.committed_url.as_deref() == Some(url)
        {
            return false;
        }
        tab.fallback_url = tab.history.same_document(url);
        if tab.history.take_failure() {
            tab.last_error = Some("Back or Forward could not reach the saved page");
            tab.loading = false;
        } else if tab.fallback_url.is_none() {
            record_same_document(&self.browsing_history, label, tab, url);
            tab.committed_url = Some(url.to_owned());
            tab.loading = false;
        } else {
            tab.loading = true;
        }
        tab.revision = tab.revision.wrapping_add(1);
        tab.document_revision = tab.document_revision.wrapping_add(1);
        true
    }
}

pub(super) fn record_same_document<T>(
    history: &BrowsingHistory,
    label: &str,
    tab: &mut BrowserTab<T>,
    url: &str,
) {
    let title = tab.title.as_deref();
    let queued = if tab.visited_url.is_some() && tab.visited_url == tab.committed_url {
        history.record_same_document(label, url, title)
    } else {
        history.record_visit(label, url, title)
    };
    tab.visited_url = queued.then(|| url.to_owned());
}

#[cfg(test)]
#[path = "commit_tests.rs"]
mod tests;
