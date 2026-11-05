use std::sync::Mutex;

use super::{DownloadRegistry, DownloadView, is_cancelable};

pub(super) fn cancel_after_native(
    downloads: &Mutex<DownloadRegistry>,
    tab_id: &str,
    id: u64,
    native: impl FnOnce() -> Result<(), String>,
) -> Result<Vec<DownloadView>, String> {
    let cancelable = downloads
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .get(&id)
        .is_some_and(|entry| entry.tab_id == tab_id && is_cancelable(entry.state));
    if !cancelable {
        return Err("browser download unavailable".into());
    }
    native()?;
    let mut registry = downloads.lock().map_err(|error| error.to_string())?;
    if !registry.cancel(tab_id, id) {
        return Err("browser download unavailable".into());
    }
    Ok(registry.views(tab_id))
}

pub(crate) fn cancel_tab_after_native(
    downloads: &Mutex<DownloadRegistry>,
    tab_id: &str,
    native: impl FnOnce(Vec<u64>) -> Result<(), String>,
) -> Result<(), String> {
    let ids = downloads
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .iter()
        .filter_map(|(id, entry)| {
            (entry.tab_id == tab_id && is_cancelable(entry.state)).then_some(*id)
        })
        .collect::<Vec<_>>();
    let result = if ids.is_empty() {
        Ok(())
    } else {
        native(ids.clone())
    };
    let mut registry = downloads.lock().map_err(|error| error.to_string())?;
    if result.is_ok() {
        registry.cancel_tab(tab_id);
    } else {
        for id in ids {
            // A queued worker can claim publication while native close runs.
            // Its filesystem result now owns the final outcome.
            if registry
                .entries
                .get(&id)
                .is_some_and(|entry| is_cancelable(entry.state))
            {
                registry.settle(id, "error");
            }
        }
        registry.forget_tab(tab_id);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::downloads::NavigationProvenance;

    #[test]
    fn failed_native_tab_close_records_error_not_cancellation() {
        let mut registry = DownloadRegistry::default();
        registry.set_provenance("browser-1", NavigationProvenance::User);
        let id = registry
            .begin("browser-1", "https://example.com/file", "file.txt")
            .unwrap();
        let downloads = Mutex::new(registry);
        let result = cancel_tab_after_native(&downloads, "browser-1", |ids| {
            assert_eq!(ids, vec![id]);
            Err("native dispatch failed".into())
        });
        assert_eq!(result, Err("native dispatch failed".into()));
        assert_eq!(
            downloads.lock().unwrap().views("browser-1")[0].state,
            "error"
        );
    }
}
