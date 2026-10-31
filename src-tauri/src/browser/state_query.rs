use super::{BrowserState, BrowserTabs};
use std::collections::HashSet;
use tauri::State;

#[cfg(all(test, windows, feature = "acp-fixture", debug_assertions))]
pub(crate) mod bootstrap_fixture;

#[tauri::command]
pub(crate) async fn browser_live_tabs(
    state: State<'_, BrowserState>,
) -> Result<Vec<String>, String> {
    let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    Ok(tabs.entries.keys().cloned().collect())
}

#[tauri::command]
pub(crate) async fn browser_history(
    state: State<'_, BrowserState>,
    tab_id: String,
) -> Result<Vec<String>, String> {
    let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    let tab = tabs.entries.get(&tab_id).ok_or("browser tab unavailable")?;
    Ok(tab.history.recent().map(str::to_owned).collect())
}

#[tauri::command]
pub(crate) async fn browser_suggested_sites(
    state: State<'_, BrowserState>,
) -> Result<Vec<String>, String> {
    let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    let mut seen = HashSet::new();
    let mut sites = Vec::new();
    for tab in tabs.entries.values() {
        for url in tab.history.recent() {
            let Ok(parsed) = url::Url::parse(url) else {
                continue;
            };
            let origin = parsed.origin().ascii_serialization();
            if seen.insert(origin) {
                sites.push(url.to_owned());
                if sites.len() == 4 {
                    return Ok(sites);
                }
            }
        }
    }
    Ok(sites)
}

impl BrowserState {
    pub(crate) fn settled_tab(&self, label: &str) -> Result<(String, u64), String> {
        let tabs = self.tabs.lock().map_err(|error| error.to_string())?;
        tabs.settled_tab(label)
    }
}

impl<T> BrowserTabs<T> {
    fn settled_tab(&self, label: &str) -> Result<(String, u64), String> {
        let url = self.openable_url(label)?;
        let revision = self
            .entries
            .get(label)
            .ok_or("browser tab unavailable")?
            .document_revision;
        Ok((url, revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_attachment_revision_survives_late_title_and_favicon_updates() {
        let mut tabs = BrowserTabs::<usize>::new();
        tabs.insert("browser-1".into(), 1, false);
        assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
        assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
        let attached = tabs.settled_tab("browser-1").unwrap();
        let projected = tabs.snapshot("browser-1").unwrap().revision;
        assert!(tabs.set_document_title("browser-1", &attached.0, "Late title"));
        assert_eq!(tabs.settled_tab("browser-1").unwrap(), attached);
        let mut png = vec![0_u8; 45];
        png[..16].copy_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
        png[16..20].copy_from_slice(&1_u32.to_be_bytes());
        png[20..24].copy_from_slice(&1_u32.to_be_bytes());
        png[33..].copy_from_slice(b"\0\0\0\0IEND\xaeB`\x82");
        assert!(tabs.set_favicon_png("browser-1", &attached.0, 1, Some(&png)));
        assert_eq!(tabs.settled_tab("browser-1").unwrap(), attached);
        assert!(tabs.snapshot("browser-1").unwrap().revision > projected);
        assert!(tabs.set_zoom_percent("browser-1", 125));
        assert_eq!(tabs.settled_tab("browser-1").unwrap(), attached);
        assert!(tabs.source_changed("browser-1", "https://example.com/#next"));
        assert_ne!(tabs.settled_tab("browser-1").unwrap().1, attached.1);
        assert!(tabs.start_navigation("browser-1", 2, &attached.0));
        assert!(tabs.settled_tab("browser-1").is_err());
        assert!(tabs.finish_navigation("browser-1", 2, Some(&attached.0)));
        assert_ne!(tabs.settled_tab("browser-1").unwrap().1, attached.1);
    }
}
