//! Ownership of an empty placeholder a silent save reserved. Dropping a
//! [`Reservation`] anywhere (a canceled download, a tab closed while
//! planning, a dispatch to a closed webview that never runs) queues its
//! release on the download worker; [`Reservation::keep`] is for a download
//! that finished writing there. Each placeholder is also recorded in the
//! settings store, and [`sweep_on_startup`] releases what a crash left.

use std::path::{Path, PathBuf};

use super::{folder, worker};
use crate::settings_store::SettingsStore;

#[derive(Debug)]
pub(crate) struct Reservation {
    path: Option<PathBuf>,
    store: SettingsStore,
}

impl Reservation {
    /// Takes ownership of a placeholder [`folder::unique_destination`]
    /// created. Runs on the worker: it writes the store.
    pub(crate) fn adopt(store: SettingsStore, path: PathBuf) -> Self {
        let _ = store.record_download_reservation(&path);
        Self {
            path: Some(path),
            store,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        self.path.as_deref().expect("a live reservation has a path")
    }

    /// The download wrote its file here: keep it and drop the record.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn keep(mut self) {
        if let Some(path) = self.path.take() {
            let store = self.store.clone();
            worker::submit(move || {
                let _ = store.forget_download_reservation(&path);
            });
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let store = self.store.clone();
            // If the queue is full the record stays, and the next start
            // releases the placeholder.
            worker::submit(move || release(&store, &path));
        }
    }
}

fn release(store: &SettingsStore, path: &Path) {
    folder::release_reservation(path);
    let _ = store.forget_download_reservation(path);
}

/// Releases placeholders recorded by an earlier run that ended before their
/// downloads settled. Only empty files are removed.
pub(crate) fn sweep_on_startup(store: &SettingsStore) {
    let store = store.clone();
    worker::submit(move || {
        for path in store.take_download_reservations().unwrap_or_default() {
            folder::release_reservation(&path);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholder(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "sage-download-reservation-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = folder::unique_destination(&directory, "file.bin", |_| false).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        path
    }

    #[test]
    fn dropping_a_reservation_releases_its_placeholder_and_record() {
        let store = SettingsStore::in_memory().unwrap();
        let path = placeholder("drop");
        let reservation = Reservation::adopt(store.clone(), path.clone());
        assert_eq!(reservation.path(), path);
        // Dropped unrun, as a closure sent to a closed webview would be.
        let closure = move || drop(reservation);
        drop(closure);
        worker::flush();
        assert!(!path.exists());
        assert!(store.take_download_reservations().unwrap().is_empty());
    }

    #[test]
    fn a_kept_reservation_leaves_the_downloaded_file_and_drops_the_record() {
        let store = SettingsStore::in_memory().unwrap();
        let path = placeholder("keep");
        let reservation = Reservation::adopt(store.clone(), path.clone());
        std::fs::write(&path, b"").unwrap();
        reservation.keep();
        worker::flush();
        // Even an empty download is kept once it finished there.
        assert!(path.exists());
        assert!(store.take_download_reservations().unwrap().is_empty());
    }

    #[test]
    fn the_startup_sweep_releases_what_a_crash_left_but_never_a_written_file() {
        let store = SettingsStore::in_memory().unwrap();
        let left = placeholder("crash-left");
        let written = placeholder("crash-written");
        // A crash: the reservations were recorded and never dropped.
        std::mem::forget(Reservation::adopt(store.clone(), left.clone()));
        std::mem::forget(Reservation::adopt(store.clone(), written.clone()));
        std::fs::write(&written, b"partial").unwrap();
        sweep_on_startup(&store);
        worker::flush();
        assert!(!left.exists());
        assert_eq!(std::fs::read(&written).unwrap(), b"partial");
        assert!(store.take_download_reservations().unwrap().is_empty());
    }
}
