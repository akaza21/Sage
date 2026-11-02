//! Publish silent downloads off the UI thread before announcing completion.

use std::{path::PathBuf, sync::Mutex};

use tauri::{AppHandle, Manager};

use super::super::{DownloadRegistry, reservation::Reservation, worker};
use super::{ACTIVE, BrowserState, RESERVED, emit_views};

pub(super) fn finish(
    app: &AppHandle,
    tab_id: &str,
    id: u64,
    terminal: &'static str,
    saved: Option<PathBuf>,
) {
    let reservation = RESERVED.with_borrow_mut(|reserved| reserved.remove(&id));
    // The native path is private for silent saves; never show it in a row.
    let saved = if reservation.is_some() { None } else { saved };
    if terminal == "completed"
        && let Some(reservation) = reservation
    {
        let publish_app = app.clone();
        let publish_tab = tab_id.to_owned();
        if !worker::submit(move || {
            let state = publish_app.state::<BrowserState>();
            match publish_if_downloading(&state.downloads, id, reservation) {
                Ok(Some(path)) => record(&publish_app, &publish_tab, id, "completed", Some(path)),
                Ok(None) => record(&publish_app, &publish_tab, id, "canceled", None),
                Err(_) => record(&publish_app, &publish_tab, id, "error", None),
            }
            let _ = publish_app.run_on_main_thread(move || {
                ACTIVE.with_borrow_mut(|active| active.remove(&id));
            });
        }) {
            ACTIVE.with_borrow_mut(|active| active.remove(&id));
            record(app, tab_id, id, "error", None);
        }
    } else {
        ACTIVE.with_borrow_mut(|active| active.remove(&id));
        record(app, tab_id, id, terminal, saved);
    }
}

fn publish_if_downloading(
    downloads: &Mutex<DownloadRegistry>,
    id: u64,
    reservation: Reservation,
) -> std::io::Result<Option<PathBuf>> {
    // Cancel/tab-close may have settled the row while this job waited behind
    // a slow filesystem operation. Never publish a canceled queued download.
    let downloading = downloads.lock().is_ok_and(|registry| {
        registry
            .entries
            .get(&id)
            .is_some_and(|entry| entry.state == "downloading")
    });
    if !downloading {
        return Ok(None);
    }
    reservation.publish().map(Some)
}

fn record(app: &AppHandle, tab_id: &str, id: u64, terminal: &'static str, saved: Option<PathBuf>) {
    if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock()
        && registry.settle(id, terminal)
        && let Some(path) = saved
    {
        registry.saved_as(id, &path);
        if terminal == "completed" {
            registry.destinations.insert(id, path);
        }
    }
    emit_views(app, tab_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::downloads::{NavigationProvenance, folder::unique_destination};

    #[test]
    fn cancel_or_tab_close_while_publication_is_queued_writes_no_final_file() {
        for close_tab in [false, true] {
            let folder =
                std::env::temp_dir().join(format!("sage-publish-cancel-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&folder).unwrap();
            let mut registry = DownloadRegistry::default();
            registry.set_provenance("browser-one", NavigationProvenance::User);
            let id = registry
                .begin("browser-one", "https://example.com/report", "report.txt")
                .unwrap();
            let mut reservation = Reservation::adopt(
                unique_destination(&folder, "report.txt", |_| false).unwrap(),
                "report.txt",
            );
            assert!(registry.record_destination(id, reservation.path().to_owned()));
            assert!(registry.destination_chosen(id));
            assert!(reservation.handoff());
            std::fs::write(reservation.writer_path(), b"downloaded bytes").unwrap();
            // Simulate the worker being occupied until after Cancel/tab close.
            if close_tab {
                assert_eq!(registry.cancel_tab("browser-one"), vec![id]);
            } else {
                assert!(registry.cancel("browser-one", id));
            }
            let downloads = Mutex::new(registry);
            assert_eq!(
                publish_if_downloading(&downloads, id, reservation).unwrap(),
                None
            );
            worker::flush();
            assert_eq!(
                downloads.lock().unwrap().views("browser-one")[0].state,
                "canceled"
            );
            assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
            std::fs::remove_dir(folder).unwrap();
        }
    }
}
