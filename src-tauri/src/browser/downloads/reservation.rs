//! Owns a silent download's public name claim and private writer destination.
//! Only completed bytes are published to a public name, atomically without
//! replacement. Cancellation releases the empty claim by handle and removes
//! only the private staging file. Crash leftovers are never swept.

use std::path::{Path, PathBuf};

use super::{placeholder::Placeholder, worker};

#[derive(Debug)]
pub(crate) struct Reservation {
    path: PathBuf,
    #[cfg_attr(not(windows), allow(dead_code))]
    name: String,
    #[cfg(windows)]
    staging: Option<super::staging::Staging>,
    placeholder: Option<Placeholder>,
}

impl Reservation {
    pub(crate) fn adopt(placeholder: Placeholder, name: &str) -> Self {
        Self {
            path: placeholder.path.clone(),
            name: name.to_owned(),
            #[cfg(windows)]
            staging: None,
            placeholder: Some(placeholder),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Allocate the private destination on the worker before resuming
    /// WebView2. The public placeholder remains a claim, never a writer path.
    #[cfg(windows)]
    pub(crate) fn handoff(&mut self) -> bool {
        if self.staging.is_some() {
            return false;
        }
        self.staging = self
            .path
            .parent()
            .and_then(|folder| super::staging::Staging::create(folder).ok());
        self.staging.is_some()
    }

    #[cfg(windows)]
    pub(crate) fn writer_path(&self) -> &Path {
        &self
            .staging
            .as_ref()
            .expect("silent download must stage before handoff")
            .path
    }

    /// Run only after WebView2 has closed the writer, on the download worker.
    #[cfg(windows)]
    pub(crate) fn publish(mut self) -> std::io::Result<PathBuf> {
        // Retire only our empty placeholder by identity. Whether its name is
        // absent is irrelevant: the no-replace rename makes the final claim.
        let _ = self.placeholder.take().and_then(Placeholder::retire);
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| std::io::Error::other("download was not staged"))?;
        let folder = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("download folder missing"))?;
        let path = staging.publish(folder, &self.name)?;
        if let Some(staging) = self.staging.take() {
            staging.cleanup();
        }
        self.keep();
        Ok(path)
    }

    /// The download finished writing here: close the handle, keep the file.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn keep(mut self) {
        self.placeholder.take();
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let placeholder = self.placeholder.take();
        #[cfg(windows)]
        let staging = self.staging.take();
        #[cfg(windows)]
        if placeholder.is_none() && staging.is_none() {
            return;
        }
        #[cfg(not(windows))]
        if placeholder.is_none() {
            return;
        }
        // Queue saturation leaves private/empty leftovers rather than doing
        // potentially blocking cleanup on the WebView2 thread.
        worker::submit(move || {
            if let Some(placeholder) = placeholder {
                placeholder.delete_if_empty();
            }
            #[cfg(windows)]
            if let Some(staging) = staging {
                staging.cleanup();
            }
        });
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
        let reservation = Reservation::adopt(
            unique_destination(&scratch.0, "file.bin", |_| false).unwrap(),
            "file.bin",
        );
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
            let mut reservation = Reservation::adopt(placeholder, suggested);
            assert_eq!(reservation.path().file_name().unwrap(), name);
            assert!(reservation.handoff());
            // Native download writers need not share write or delete access.
            let mut writer = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .share_mode(FILE_SHARE_READ)
                .open(reservation.writer_path())
                .expect("the download writer must be able to open its destination");
            writer.write_all(content).unwrap();
            drop(writer);
            assert_eq!(reservation.publish().unwrap(), scratch.0.join(name));
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
        let mut first = Reservation::adopt(
            unique_destination(&scratch.0, "report.txt", |_| false).unwrap(),
            "report.txt",
        );
        assert!(registry.record_destination(id, first.path().to_owned()));
        assert!(first.handoff());
        assert!(first.path().exists());
        let claimed = registry.claimed();
        let second = Reservation::adopt(
            unique_destination(&scratch.0, "report.txt", |path| claimed.contains(path)).unwrap(),
            "report.txt",
        );
        assert_eq!(second.path().file_name().unwrap(), "report (1).txt");
        assert!(registry.destination_chosen(id));
        std::fs::write(first.writer_path(), b"first download").unwrap();
        assert_eq!(first.publish().unwrap(), scratch.0.join("report.txt"));
        drop(second);
        worker::flush();
        assert_eq!(
            std::fs::read(scratch.0.join("report.txt")).unwrap(),
            b"first download"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_third_party_file_created_mid_handoff_is_not_overwritten() {
        use std::{fs::OpenOptions, io::Write, os::windows::fs::OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

        let scratch = Scratch::new("handoff-race");
        let final_path = scratch.0.join("report.txt");
        let mut reservation = Reservation::adopt(
            unique_destination(&scratch.0, "report.txt", |_| false).unwrap(),
            "report.txt",
        );
        assert!(reservation.handoff());
        // Another process takes the public name while the UI resume is queued.
        std::fs::write(&final_path, b"another application's data").unwrap();
        // Model WebView2: CREATE_ALWAYS/truncate, not create_new. It may also
        // keep exclusive write/delete sharing until completion.
        let mut writer = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(FILE_SHARE_READ)
            .open(reservation.writer_path())
            .unwrap();
        writer.write_all(b"downloaded bytes").unwrap();
        drop(writer);
        let published = reservation.publish().unwrap();
        assert_eq!(
            std::fs::read(&final_path).unwrap(),
            b"another application's data"
        );
        assert_eq!(published, scratch.0.join("report (1).txt"));
        assert_eq!(std::fs::read(&published).unwrap(), b"downloaded bytes");
        assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 2);
    }

    #[test]
    fn a_kept_empty_download_stays() {
        let scratch = Scratch::new("keep");
        let reservation = Reservation::adopt(
            unique_destination(&scratch.0, "file.bin", |_| false).unwrap(),
            "file.bin",
        );
        let path = reservation.path().to_owned();
        reservation.keep();
        worker::flush();
        assert!(path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_late_collision_advances_the_original_name_suffix() {
        let scratch = Scratch::new("late-suffix");
        std::fs::write(scratch.0.join("report.txt"), b"original").unwrap();
        let mut reservation = Reservation::adopt(
            unique_destination(&scratch.0, "report.txt", |_| false).unwrap(),
            "report.txt",
        );
        assert_eq!(reservation.path(), scratch.0.join("report (1).txt"));
        assert!(reservation.handoff());
        std::fs::write(reservation.path(), b"third party").unwrap();
        std::fs::write(reservation.writer_path(), b"download").unwrap();
        let published = reservation.publish().unwrap();
        assert_eq!(published, scratch.0.join("report (2).txt"));
        assert_eq!(
            std::fs::read(scratch.0.join("report.txt")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(scratch.0.join("report (1).txt")).unwrap(),
            b"third party"
        );
        assert_eq!(std::fs::read(published).unwrap(), b"download");
    }

    #[cfg(windows)]
    #[test]
    fn canceling_a_staged_download_removes_only_its_private_bytes_and_claim() {
        let scratch = Scratch::new("cancel-staging");
        let other = scratch.0.join("existing.txt");
        std::fs::write(&other, b"keep me").unwrap();
        let mut reservation = Reservation::adopt(
            unique_destination(&scratch.0, "report.txt", |_| false).unwrap(),
            "report.txt",
        );
        assert!(reservation.handoff());
        std::fs::write(reservation.writer_path(), b"partial download").unwrap();
        drop(reservation);
        worker::flush();
        assert_eq!(std::fs::read(&other).unwrap(), b"keep me");
        assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 1);
    }

    #[test]
    fn a_dropped_reservation_never_deletes_a_file_that_took_its_name() {
        let scratch = Scratch::new("replaced");
        let reservation = Reservation::adopt(
            unique_destination(&scratch.0, "file.bin", |_| false).unwrap(),
            "file.bin",
        );
        let path = reservation.path().to_owned();
        let staged = scratch.0.join("staged.bin");
        std::fs::write(&staged, b"").unwrap();
        std::fs::rename(&staged, &path).unwrap();
        drop(reservation);
        worker::flush();
        assert!(path.exists());
    }
}
