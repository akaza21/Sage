use super::*;
use std::path::PathBuf;

use crate::browser::downloads::folder::unique_destination;

fn recorded_paths(store: &SettingsStore) -> Vec<PathBuf> {
    store
        .download_reservations()
        .unwrap()
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sage-download-reservation-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn reserve(&self) -> Reserved {
        unique_destination(&self.0, "file.bin", |_| false).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dropping_a_reservation_releases_its_placeholder_and_record() {
    let store = SettingsStore::in_memory().unwrap();
    let scratch = Scratch::new("drop");
    let reserved = scratch.reserve();
    let path = reserved.path.clone();
    let reservation = Reservation::adopt(store.clone(), reserved);
    assert_eq!(reservation.path(), path);
    assert_eq!(recorded_paths(&store), vec![path.clone()]);
    // Dropped unrun, as a closure sent to a closed webview would be.
    let closure = move || drop(reservation);
    drop(closure);
    worker::flush();
    assert!(!path.exists());
    assert!(recorded_paths(&store).is_empty());
}

#[test]
fn a_kept_empty_download_is_never_swept_later() {
    let store = SettingsStore::in_memory().unwrap();
    let scratch = Scratch::new("keep");
    let reserved = scratch.reserve();
    let path = reserved.path.clone();
    Reservation::adopt(store.clone(), reserved).keep();
    // The record is gone at once, before any queued work runs.
    assert!(recorded_paths(&store).is_empty());
    sweep_on_startup(&store);
    worker::flush();
    assert!(path.exists());
}

#[test]
fn the_startup_sweep_releases_what_a_crash_left() {
    let store = SettingsStore::in_memory().unwrap();
    let scratch = Scratch::new("crash-left");
    let reserved = scratch.reserve();
    let path = reserved.path.clone();
    std::mem::forget(Reservation::adopt(store.clone(), reserved));
    sweep_on_startup(&store);
    worker::flush();
    assert!(!path.exists());
    assert!(recorded_paths(&store).is_empty());
}

#[test]
fn the_startup_sweep_keeps_a_users_empty_file_that_replaced_the_placeholder() {
    let store = SettingsStore::in_memory().unwrap();
    let scratch = Scratch::new("crash-replaced");
    let reserved = scratch.reserve();
    let path = reserved.path.clone();
    std::mem::forget(Reservation::adopt(store.clone(), reserved));
    // After the crash the user moves it away and makes their own empty file
    // at the same path (the old one stays alive, so its identity is unique).
    std::fs::rename(&path, scratch.0.join("old.bin")).unwrap();
    std::fs::write(&path, b"").unwrap();
    sweep_on_startup(&store);
    worker::flush();
    assert!(path.exists());
    // Not Sage's any more, so the record goes too.
    assert!(recorded_paths(&store).is_empty());
}

#[test]
fn the_startup_sweep_keeps_a_partly_written_download() {
    let store = SettingsStore::in_memory().unwrap();
    let scratch = Scratch::new("crash-written");
    let reserved = scratch.reserve();
    let path = reserved.path.clone();
    std::mem::forget(Reservation::adopt(store.clone(), reserved));
    std::fs::write(&path, b"partial").unwrap();
    sweep_on_startup(&store);
    worker::flush();
    assert_eq!(std::fs::read(&path).unwrap(), b"partial");
    assert!(recorded_paths(&store).is_empty());
}

#[cfg(unix)]
#[test]
fn an_unreachable_placeholder_keeps_its_record_for_the_next_start() {
    let store = SettingsStore::in_memory().unwrap();
    let scratch = Scratch::new("unreachable");
    let reserved = scratch.reserve();
    // A record whose path cannot be examined (through a regular file) stands
    // in for an offline share: the clean-up outcome is unknown.
    let blocked = reserved.path.join("child");
    store
        .record_download_reservation(&record(&blocked, reserved.identity))
        .unwrap();
    sweep_on_startup(&store);
    worker::flush();
    assert_eq!(recorded_paths(&store), vec![blocked]);
}
