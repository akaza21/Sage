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
        let reservation = reservation.completed();
        let publish_app = app.clone();
        let publish_tab = tab_id.to_owned();
        if !worker::submit(move || {
            let claim_app = publish_app.clone();
            let claim_tab = publish_tab.clone();
            let result_app = publish_app.clone();
            let result_tab = publish_tab.clone();
            let recovery = reservation.writer_path().to_owned();
            if !reservation.publish_if(move || {
                let claimed = claim_publication(&claim_app.state::<BrowserState>().downloads, id);
                if claimed {
                    emit_views(&claim_app, &claim_tab);
                }
                claimed
            }, move |result| {
                match result {
                    Ok(Some(path)) => record(&result_app, &result_tab, id, "completed", Some(path)),
                    Ok(None) => record(&result_app, &result_tab, id, "canceled", None),
                    Err(error) => {
                        let message = format!("{error}. Downloaded bytes may be recovered from {} or may already be saved in the destination folder.", recovery.display());
                        record_error(&result_app, &result_tab, id, message);
                    }
                }
                release_active(&result_app, id);
            }) {
                record_error(&publish_app, &publish_tab, id, "Download destination I/O executor unavailable".into());
                release_active(&publish_app, id);
            }
        }) {
            ACTIVE.with_borrow_mut(|active| active.remove(&id));
            record(app, tab_id, id, "error", None);
        }
    } else {
        ACTIVE.with_borrow_mut(|active| active.remove(&id));
        record(app, tab_id, id, terminal, saved);
    }
}

fn release_active(app: &AppHandle, id: u64) {
    let _ = app.run_on_main_thread(move || {
        ACTIVE.with_borrow_mut(|active| active.remove(&id));
    });
}

fn record_error(app: &AppHandle, tab_id: &str, id: u64, message: String) {
    if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock()
        && registry.settle(id, "error")
        && let Some(entry) = registry.entries.get_mut(&id)
    {
        entry.error = Some(message.chars().take(2048).collect());
    }
    emit_views(app, tab_id);
}

#[cfg(test)]
fn publish_if_downloading(
    downloads: &Mutex<DownloadRegistry>,
    id: u64,
    publish: impl FnOnce() -> std::io::Result<PathBuf>,
) -> std::io::Result<Option<PathBuf>> {
    if !claim_publication(downloads, id) {
        return Ok(None);
    }
    publish().map(Some)
}

fn claim_publication(downloads: &Mutex<DownloadRegistry>, id: u64) -> bool {
    // Claim publication atomically with respect to Cancel/tab-close. The lock
    // must be released before filesystem I/O, which can stall on network folders.
    downloads.lock().is_ok_and(|mut registry| {
        let Some(entry) = registry.entries.get_mut(&id) else {
            return false;
        };
        if entry.state != "downloading" {
            return false;
        }
        entry.state = "publishing";
        true
    })
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
    fn cancel_or_tab_close_while_queued_on_destination_writes_no_final_file() {
        use crate::browser::downloads::io;
        use std::{
            sync::{Arc, mpsc},
            time::Duration,
        };

        for close_tab in [false, true] {
            let folder = std::env::temp_dir()
                .join(format!("sage-destination-cancel-{}", uuid::Uuid::new_v4()));
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
            let downloads = Arc::new(Mutex::new(registry));
            let (release, wait) = mpsc::sync_channel::<()>(0);
            let (started, start) = mpsc::sync_channel(1);
            io::acquire(&folder).unwrap().cleanup(move |_| {
                started.send(()).unwrap();
                let _ = wait.recv();
            });
            start.recv_timeout(Duration::from_secs(1)).unwrap();
            let claim_registry = Arc::clone(&downloads);
            let (done, completion) = mpsc::sync_channel(1);
            assert!(reservation.publish_if(
                move || claim_publication(&claim_registry, id),
                move |result| {
                    done.send(result).ok();
                }
            ));
            if close_tab {
                assert_eq!(
                    downloads.lock().unwrap().cancel_tab("browser-one"),
                    vec![id]
                );
            } else {
                assert!(downloads.lock().unwrap().cancel("browser-one", id));
            }
            drop(release);
            assert_eq!(
                completion
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap()
                    .unwrap(),
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
