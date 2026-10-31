//! What a tab commits: the URL a navigation or same-document change lands
//! on, and the title its page reports. Each new committed URL is also a
//! visit in the global browsing history, and each new title names the
//! latest visit to that URL; both are queued without blocking the UI
//! thread (see `browsing_history`).

use super::{BrowserTabs, admit_url};

impl<T> BrowserTabs<T> {
    pub(super) fn finish_navigation(
        &mut self,
        label: &str,
        navigation_id: u64,
        url: Option<&str>,
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
            // entering the address already shown is not a new visit.
            if let Some(url) = url
                && previous.as_deref() != Some(url)
            {
                self.browsing_history.record_visit(label, url);
            }
            tab.loading = false;
        }
        tab.revision = tab.revision.wrapping_add(1);
        tab.document_revision = tab.document_revision.wrapping_add(1);
        true
    }

    pub(super) fn set_document_title(&mut self, label: &str, url: &str, raw: &str) -> bool {
        let Some(tab) = self.entries.get_mut(label) else {
            return false;
        };
        if tab.loading || tab.last_error.is_some() || tab.committed_url.as_deref() != Some(url) {
            return false;
        }
        let title = raw
            .chars()
            .take(256)
            .filter(|character| {
                !matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
            .map(|character| if character.is_control() { ' ' } else { character })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(80)
            .collect::<String>();
        let title = if title.is_empty() { None } else { Some(title) };
        if tab.title == title {
            return false;
        }
        if let Some(title) = &title {
            self.browsing_history.record_title(label, title);
        }
        tab.title = title;
        tab.revision = tab.revision.wrapping_add(1);
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
            tab.committed_url = Some(url.to_owned());
            self.browsing_history
                .record_same_document(label, url, tab.title.as_deref());
            tab.loading = false;
        } else {
            tab.loading = true;
        }
        tab.revision = tab.revision.wrapping_add(1);
        tab.document_revision = tab.document_revision.wrapping_add(1);
        true
    }
}

#[cfg(test)]
#[path = "commit_tests.rs"]
mod tests;
