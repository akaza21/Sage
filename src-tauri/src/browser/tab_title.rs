use super::BrowserTabs;

impl<T> BrowserTabs<T> {
    /// Records the committed document's title, stripped of bidi controls and
    /// bounded to 80 characters. Returns whether the visible title changed.
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
        tab.title = title;
        tab.revision = tab.revision.wrapping_add(1);
        true
    }
}
