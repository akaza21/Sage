//! Ownership of an empty placeholder a silent save reserved.
//!
//! - Dropping a [`Reservation`] anywhere (a canceled download, a tab closed
//!   while planning, a hand-back to a closed webview that never runs) queues
//!   its release on the download worker.
//! - [`Reservation::keep`] is for a download that finished there; it drops
//!   the record at once, so a finished file can never be swept later.
//! - Each placeholder is recorded with its file identity before anything
//!   else can happen to it, and a record is deleted only once its clean-up
//!   is confirmed: removed, already gone, or no longer Sage's file. A file
//!   whose ownership is uncertain is always kept. [`sweep_on_startup`]
//!   retries what a crash or an unreachable folder left.

use std::path::Path;

use super::{
    folder::Reserved,
    identity::{FileIdentity, Release, release_if_ours},
    worker,
};
use crate::settings_store::{DownloadReservationRecord, SettingsStore};

#[derive(Debug)]
pub(crate) struct Reservation {
    reserved: Option<Reserved>,
    store: SettingsStore,
}

impl Reservation {
    /// Takes ownership of a placeholder. Runs on the worker: it writes the
    /// store.
    pub(crate) fn adopt(store: SettingsStore, reserved: Reserved) -> Self {
        let _ = store.record_download_reservation(&record(&reserved.path, reserved.identity));
        Self {
            reserved: Some(reserved),
            store,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self
            .reserved
            .as_ref()
            .expect("a live reservation has a placeholder")
            .path
    }

    /// The download finished writing here: keep the file. The record is
    /// deleted now, not queued, so no later sweep can mistake a finished
    /// (even empty) download for an unused placeholder. It is one local
    /// SQLite delete and touches no download folder.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn keep(mut self) {
        if let Some(reserved) = self.reserved.take() {
            let _ = self.store.forget_download_reservation(&reserved.path);
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(reserved) = self.reserved.take() {
            let store = self.store.clone();
            // If the queue is full the record stays, and the next start
            // retries.
            worker::submit(move || release(&store, &reserved.path, reserved.identity));
        }
    }
}

fn record(path: &Path, identity: FileIdentity) -> DownloadReservationRecord {
    DownloadReservationRecord {
        path: path.to_owned(),
        volume: identity.volume,
        file_index: identity.index,
    }
}

/// Releases one placeholder; the record goes only when the outcome is known.
fn release(store: &SettingsStore, path: &Path, identity: FileIdentity) {
    match release_if_ours(path, identity) {
        Release::Done | Release::NotOurs => {
            let _ = store.forget_download_reservation(path);
        }
        Release::Unknown => {}
    }
}

/// Releases placeholders an earlier run left (a crash, an unreachable
/// folder). Each record is kept until its own clean-up is confirmed.
pub(crate) fn sweep_on_startup(store: &SettingsStore) {
    let store = store.clone();
    worker::submit(move || {
        for entry in store.download_reservations().unwrap_or_default() {
            let identity = FileIdentity {
                volume: entry.volume,
                index: entry.file_index,
            };
            release(&store, &entry.path, identity);
        }
    });
}

#[cfg(test)]
#[path = "reservation_tests.rs"]
mod tests;
