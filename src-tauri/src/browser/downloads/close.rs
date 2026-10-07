use std::sync::Mutex;

use super::DownloadRegistry;

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
            (entry.tab_id == tab_id && super::is_active(entry.state)).then_some(*id)
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
            registry.settle(id, "error");
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
