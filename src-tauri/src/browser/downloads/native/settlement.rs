//! Publish silent downloads off the UI thread before announcing completion.

use std::{path::PathBuf, sync::Mutex};

use tauri::{AppHandle, Manager};

use super::super::{DownloadRegistry, worker};
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
            match publish_if_downloading(&state.downloads, id, || {
                emit_views(&publish_app, &publish_tab);
                reservation.publish()
            }) {
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
    publish: impl FnOnce() -> std::io::Result<PathBuf>,
) -> std::io::Result<Option<PathBuf>> {
    // Claim publication atomically with respect to Cancel/tab-close. The lock
    // must be released before filesystem I/O, which can stall on network folders.
    let claimed = downloads.lock().is_ok_and(|mut registry| {
        let Some(entry) = registry.entries.get_mut(&id) else {
            return false;
        };
        if entry.state != "downloading" {
            return false;
        }
        entry.state = "publishing";
        true
    });
    if !claimed {
        return Ok(None);
    }
    publish().map(Some)
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
    use crate::browser::downloads::{
        NavigationProvenance, folder::unique_destination, reservation::Reservation,
    };

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
                publish_if_downloading(&downloads, id, || reservation.publish()).unwrap(),
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

    #[test]
    fn cancellation_after_publication_starts_preserves_the_real_outcome() {
        use crate::browser::downloads::{cancel_after_native, cancel_tab_after_native};

        for close_tab in [false, true] {
            for fail_publish in [false, true] {
                let folder = std::env::temp_dir()
                    .join(format!("sage-publish-started-{}", uuid::Uuid::new_v4()));
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
                let downloads = Mutex::new(registry);
                let mut cancellation = None;
                let published = publish_if_downloading(&downloads, id, || {
                    // The publish step controls this interleaving: the worker has
                    // passed its check, but no publication I/O has happened yet.
                    cancellation = Some(if close_tab {
                        cancel_tab_after_native(&downloads, "browser-one", |_| Ok(()))
                    } else {
                        cancel_after_native(&downloads, "browser-one", id, || Ok(())).map(|_| ())
                    });
                    if fail_publish {
                        return Err(std::io::Error::other("publication failed"));
                    }
                    reservation.publish()
                });
                let terminal = if fail_publish { "error" } else { "completed" };
                assert_eq!(published.is_err(), fail_publish);
                assert!(downloads.lock().unwrap().settle(id, terminal));
                assert_eq!(
                    downloads.lock().unwrap().views("browser-one")[0].state,
                    terminal
                );
                if close_tab {
                    assert!(cancellation.unwrap().is_ok());
                } else {
                    assert!(cancellation.unwrap().is_err());
                }
                worker::flush();
                if fail_publish {
                    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
                } else {
                    let path = published.unwrap().unwrap();
                    assert_eq!(
                        path,
                        crate::browser::downloads::folder::admit_folder(&folder)
                            .unwrap()
                            .join("report.txt")
                    );
                    assert_eq!(std::fs::read(&path).unwrap(), b"downloaded bytes");
                    std::fs::remove_file(path).unwrap();
                }
                std::fs::remove_dir(folder).unwrap();
            }
        }
    }

    #[test]
    fn failed_native_close_during_publication_cannot_overwrite_completion() {
        use crate::browser::downloads::cancel_tab_after_native;

        let folder = std::env::temp_dir().join(format!(
            "sage-publish-close-failed-{}",
            uuid::Uuid::new_v4()
        ));
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
        let downloads = Mutex::new(registry);
        let mut published = None;
        let close = cancel_tab_after_native(&downloads, "browser-one", |_| {
            // Native close has taken its cancelable snapshot. The worker now
            // publishes, but record() has not yet announced its completion.
            published = publish_if_downloading(&downloads, id, || reservation.publish()).unwrap();
            Err("native dispatch failed".into())
        });
        assert_eq!(close, Err("native dispatch failed".into()));
        assert!(downloads.lock().unwrap().settle(id, "completed"));
        assert_eq!(
            downloads.lock().unwrap().views("browser-one")[0].state,
            "completed"
        );
        let path = published.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"downloaded bytes");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
