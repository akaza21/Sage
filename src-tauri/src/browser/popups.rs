use url::Url;

use super::{BrowserTabs, MAX_BROWSER_TABS, admit_url};

pub(super) fn popup_target<T>(
    tabs: &BrowserTabs<T>,
    opener: &str,
    requested: &Url,
) -> Option<String> {
    if !tabs.entries.contains_key(opener) || tabs.slot_count() >= MAX_BROWSER_TABS {
        return None;
    }
    admit_url(requested.as_str())
        .ok()
        .map(|url| url.to_string())
}
