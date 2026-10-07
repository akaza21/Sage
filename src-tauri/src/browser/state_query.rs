use super::BrowserState;
use std::collections::HashSet;
use tauri::State;

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
        let url = tabs.openable_url(label)?;
        let revision = tabs
            .snapshot(label)
            .ok_or("browser tab unavailable")?
            .revision;
        Ok((url, revision))
    }
}
