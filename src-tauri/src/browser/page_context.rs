#[cfg(windows)]
use tauri::Manager;
use tauri::State;

use super::BrowserState;

#[tauri::command]
pub(crate) async fn browser_page_context(
    state: State<'_, BrowserState>,
    tab_id: String,
    expected_revision: String,
) -> Result<serde_json::Value, String> {
    if !valid_page_context_tab_id(&tab_id) {
        return Err("invalid browser tab id".into());
    }
    #[cfg(windows)]
    {
        let webview = state
            .tabs
            .lock()
            .map_err(|_| "browser unavailable")?
            .handle(&tab_id)
            .cloned()
            .ok_or("browser tab unavailable")?;
        let result = tauri::async_runtime::spawn_blocking(move || {
            let browser_state = webview.app_handle().state::<BrowserState>();
            super::devtools::capture_page_context(&browser_state, &tab_id, &expected_revision)
        })
        .await
        .map_err(|_| "browser page capture unavailable")??;
        serde_json::to_value(result).map_err(|_| "browser page capture unavailable".into())
    }
    #[cfg(not(windows))]
    {
        let _ = (state, tab_id, expected_revision);
        Err("browser page capture is only available on Windows".into())
    }
}

pub(super) fn valid_page_context_tab_id(tab_id: &str) -> bool {
    tab_id.len() <= 64
        && tab_id
            .strip_prefix("browser-")
            .and_then(|digits| digits.parse::<u64>().ok())
            .is_some_and(|number| number > 0 && tab_id == format!("browser-{number}"))
}

#[cfg(test)]
mod tests {
    use super::valid_page_context_tab_id;

    #[test]
    fn rejects_noncanonical_or_oversized_tab_ids() {
        assert!(valid_page_context_tab_id("browser-1"));
        assert!(!valid_page_context_tab_id("browser-0"));
        assert!(!valid_page_context_tab_id("browser-01"));
        assert!(!valid_page_context_tab_id("browser-1/../secret"));
        assert!(!valid_page_context_tab_id(&format!(
            "browser-{}",
            "1".repeat(57)
        )));
    }
}
