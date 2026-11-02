//! Ownership of a silent save's name and its temporary placeholder.
//!
//! - Before handoff, dropping a [`Reservation`] (a canceled or failed start, a
//!   tab closed while planning, a hand-back to a closed webview that never
//!   runs) queues a delete of the placeholder through its own handle on the
//!   download worker, only if it is still empty (Windows; elsewhere the
//!   placeholder is left, see [`super::placeholder`]).
//! - Handoff retires the empty placeholder on the worker before WebView2
//!   resumes. The registry retains the name until settlement; afterward
//!   there is no Sage file handle or cleanup of the native destination.
//! - [`Reservation::keep`] is for a download that finished there.
//!
//! Nothing is recorded and nothing is swept after a crash; see
//! [`super::placeholder`].

use std::path::{Path, PathBuf};

use super::{placeholder::Placeholder, worker};

#[derive(Debug)]
pub(crate) struct Reservation {
    path: PathBuf,
    placeholder: Option<Placeholder>,
}

impl Reservation {
    pub(crate) fn adopt(placeholder: Placeholder) -> Self {
        Self {
            path: placeholder.path.clone(),
            placeholder: Some(placeholder),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Called on the worker after the registry claims this path, before
    /// WebView2 resumes. Afterward only the registry owns the name; Sage
    /// never deletes anything the native download may create there.
    #[cfg(windows)]
    pub(crate) fn handoff(&mut self) -> bool {
        self.placeholder
            .take()
            .and_then(Placeholder::handoff)
            .is_some()
    }

    /// The download finished writing here: close the handle, keep the file.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn keep(mut self) {
        self.placeholder.take();
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(placeholder) = self.placeholder.take() {
            // If the queue is full the empty placeholder is left in place,
            // the safe direction.
            worker::submit(move || {
                placeholder.delete_if_empty();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::browser::downloads::folder::unique_destination;

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
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn dropping_a_reservation_deletes_its_empty_placeholder_where_a_handle_can() {
        let scratch = Scratch::new("drop");
        let reservation =
            Reservation::adopt(unique_destination(&scratch.0, "file.bin", |_| false).unwrap());
        let path = reservation.path().to_owned();
        assert!(path.exists());
        // Dropped unrun, as a closure sent to a closed webview would be.
        let closure = move || drop(reservation);
        drop(closure);
        worker::flush();
        assert_eq!(path.exists(), !cfg!(windows));
    }

    #[cfg(windows)]
    #[test]
    fn silent_downloads_allow_exclusive_writers_and_preserve_existing_files() {
        use std::{fs::OpenOptions, io::Write, os::windows::fs::OpenOptionsExt};

        use crate::browser::downloads::folder::{Plan, plan};
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

        let scratch = Scratch::new("silent-writer");
        for (suggested, name, content) in [
            ("report-a.txt", "report-a.txt", b"alpha\n".as_slice()),
            ("report-b.txt", "report-b.txt", b"bravo\n".as_slice()),
            (
                "report-a.txt",
                "report-a (1).txt",
                b"duplicate\n".as_slice(),
            ),
        ] {
            let Plan::Silent(placeholder) = plan(Some(&scratch.0), suggested, None, |_| false)
            else {
                panic!("an available remembered folder should save silently");
            };
            let mut reservation = Reservation::adopt(placeholder);
            assert_eq!(reservation.path().file_name().unwrap(), name);
            assert!(reservation.handoff());
            // Native download writers need not share write or delete access.
            let mut writer = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .share_mode(FILE_SHARE_READ)
                .open(reservation.path())
                .expect("the download writer must be able to open its destination");
            writer.write_all(content).unwrap();
            drop(writer);
            reservation.keep();
            assert_eq!(std::fs::read(scratch.0.join(name)).unwrap(), content);
        }
        assert_eq!(
            std::fs::read(scratch.0.join("report-a.txt")).unwrap(),
            b"alpha\n"
        );
        assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 3);
    }

    #[cfg(windows)]
    #[test]
    fn a_claimed_name_stays_reserved_after_its_placeholder_is_handed_off() {
        use crate::browser::downloads::{DownloadRegistry, NavigationProvenance};

        let scratch = Scratch::new("claimed-handoff");
        let mut registry = DownloadRegistry::default();
        registry.set_provenance("browser-one", NavigationProvenance::User);
        let id = registry
            .begin("browser-one", "https://example.com/report", "report.txt")
            .unwrap();
        let mut first =
            Reservation::adopt(unique_destination(&scratch.0, "report.txt", |_| false).unwrap());
        assert!(registry.record_destination(id, first.path().to_owned()));
        assert!(first.handoff());
        assert!(!first.path().exists());
        let claimed = registry.claimed();
        let second = Reservation::adopt(
            unique_destination(&scratch.0, "report.txt", |path| claimed.contains(path)).unwrap(),
        );
        assert_eq!(second.path().file_name().unwrap(), "report (1).txt");
        assert!(registry.destination_chosen(id));
        std::fs::write(first.path(), b"first download").unwrap();
        drop(first);
        drop(second);
        worker::flush();
        assert_eq!(
            std::fs::read(scratch.0.join("report.txt")).unwrap(),
            b"first download"
        );
    }

    #[test]
    fn a_kept_empty_download_stays() {
        let scratch = Scratch::new("keep");
        let reservation =
            Reservation::adopt(unique_destination(&scratch.0, "file.bin", |_| false).unwrap());
        let path = reservation.path().to_owned();
        reservation.keep();
        worker::flush();
        assert!(path.exists());
    }

    #[test]
    fn a_dropped_reservation_never_deletes_a_file_that_took_its_name() {
        let scratch = Scratch::new("replaced");
        let reservation =
            Reservation::adopt(unique_destination(&scratch.0, "file.bin", |_| false).unwrap());
        let path = reservation.path().to_owned();
        let staged = scratch.0.join("staged.bin");
        std::fs::write(&staged, b"").unwrap();
        std::fs::rename(&staged, &path).unwrap();
        drop(reservation);
        worker::flush();
        assert!(path.exists());
    }
}
