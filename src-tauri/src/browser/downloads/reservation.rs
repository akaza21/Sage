//! Owns a silent download's public name claim and private writer destination.
//! Only completed bytes are published to a public name, atomically without
//! replacement. Cancellation releases the empty claim by handle and removes
//! only the private staging file. Crash leftovers are never swept.

use std::path::{Path, PathBuf};

#[cfg(any(windows, test))]
use super::io;
#[cfg(windows)]
use super::io::Deadline;
use super::{io::Destination, placeholder::Placeholder};

#[derive(Debug)]
pub(crate) struct Reservation {
    path: PathBuf,
    #[cfg_attr(not(windows), allow(dead_code))]
    name: String,
    #[cfg(windows)]
    staging: Option<super::staging::Staging>,
    #[cfg(windows)]
    preserve_staging: bool,
    placeholder: Option<Placeholder>,
    destination: Option<Destination>,
}

impl Reservation {
    #[cfg(test)]
    pub(crate) fn adopt(placeholder: Placeholder, name: &str) -> Self {
        let destination = io::acquire(placeholder.path.parent().unwrap()).unwrap();
        Self::on_destination(placeholder, name, destination)
    }

    pub(crate) fn on_destination(
        placeholder: Placeholder,
        name: &str,
        destination: Destination,
    ) -> Self {
        Self {
            path: placeholder.path.clone(),
            name: name.to_owned(),
            #[cfg(windows)]
            staging: None,
            #[cfg(windows)]
            preserve_staging: false,
            placeholder: Some(placeholder),
            destination: Some(destination),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Allocate the private destination on its I/O lane before resuming
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

    /// Once native writing completed, dispatch failure must keep the bytes too.
    #[cfg(windows)]
    pub(crate) fn completed(mut self) -> Self {
        self.preserve_staging = true;
        self
    }

    /// Run only after WebView2 has closed the writer, on its destination lane.
    /// Taking staging before I/O preserves completed bytes on every failure,
    /// including when the deadline expires inside an uninterruptible OS call.
    #[cfg(windows)]
    pub(crate) fn publish_until(mut self, deadline: Deadline) -> std::io::Result<PathBuf> {
        let staging = self
            .staging
            .take()
            .ok_or_else(|| std::io::Error::other("download was not staged"))?;
        deadline.check()?;
        // Retire only our empty placeholder by identity. Whether its name is
        // absent is irrelevant: the no-replace rename makes the final claim.
        let _ = self.placeholder.take().and_then(Placeholder::retire);
        deadline.check()?;
        let folder = self
            .path
            .parent()
            .and_then(|path| super::folder::admit_folder_before(path, deadline))
            .ok_or_else(|| std::io::Error::other("download folder unavailable"))?;
        deadline.check()?;
        // Report the same canonical, long folder spelling as Settings, even
        // when the reservation arrived through an 8.3 alias.
        let path = staging.publish(&folder, &self.name, deadline)?;
        staging.cleanup(deadline);
        deadline.check()?;
        Ok(path)
    }

    #[cfg(all(windows, test))]
    pub(crate) fn publish(self) -> std::io::Result<PathBuf> {
        self.publish_until(io::test_deadline())
    }

    #[cfg(all(windows, test))]
    pub(crate) fn publish_off_thread(
        self,
        completed: impl FnOnce(std::io::Result<PathBuf>) + Send + 'static,
    ) -> bool {
        self.publish_if(
            || true,
            move |result| {
                completed(result.and_then(|path| {
                    path.ok_or_else(|| std::io::Error::other("download canceled"))
                }))
            },
        )
    }

    #[cfg(windows)]
    pub(crate) fn publish_if(
        mut self,
        claim: impl FnOnce() -> bool + Send + 'static,
        completed: impl FnOnce(std::io::Result<Option<PathBuf>>) + Send + 'static,
    ) -> bool {
        self.preserve_staging = true;
        let destination = self
            .destination
            .as_ref()
            .expect("reservation owns its destination lane")
            .operation();
        destination.run(
            move |_, deadline| {
                if let Err(error) = deadline.check() {
                    self.staging.take();
                    return Err(error);
                }
                if !claim() {
                    // A timeout callback can settle the row while claim checks it.
                    // Preserve completed bytes in that race as well as in I/O.
                    self.preserve_staging = deadline.check().is_err();
                    drop(self);
                    return Ok(None);
                }
                self.publish_until(deadline).map(Some)
            },
            completed,
            |_| {},
        )
    }

    #[cfg(test)]
    pub(crate) fn keep(mut self) {
        if let Some(placeholder) = self.placeholder.take() {
            self.destination
                .take()
                .unwrap()
                .cleanup(move |_| drop(placeholder));
        }
    }

    /// Dispose a late plan on its destination lane, retaining admission while
    /// the OS closes or deletes its handles. No shared coordinator is involved.
    #[cfg(windows)]
    pub(crate) fn discard(mut self) {
        let deadline = io::cleanup_deadline();
        if let Some(placeholder) = self.placeholder.take() {
            placeholder.delete_if_empty();
        }
        if let Some(staging) = self.staging.take() {
            staging.cleanup(deadline);
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let placeholder = self.placeholder.take();
        #[cfg(windows)]
        let staging = self.staging.take();
        #[cfg(windows)]
        let staging = if self.preserve_staging { None } else { staging };
        #[cfg(windows)]
        if placeholder.is_none() && staging.is_none() {
            return;
        }
        #[cfg(not(windows))]
        if placeholder.is_none() {
            return;
        }
        // Admission reserved a disposal slot before creating any handles.
        self.destination.take().unwrap().cleanup(move |deadline| {
            if let Some(placeholder) = placeholder
                && deadline.check().is_ok()
            {
                placeholder.delete_if_empty();
            }
            #[cfg(windows)]
            if let Some(staging) = staging {
                staging.cleanup(deadline);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    #[cfg(windows)]
    use crate::browser::downloads::folder::admit_folder;
    use crate::browser::downloads::folder::unique_destination;
    use crate::browser::downloads::worker;

    struct Scratch(PathBuf);

    #[cfg(windows)]
    #[test]
    fn a_completed_download_keeps_its_bytes_if_publication_cannot_be_dispatched() {
        let scratch = Scratch::new("completed-undispatched");
        let mut reservation = Reservation::adopt(
            unique_destination(&scratch.0, "report.txt", |_| false).unwrap(),
            "report.txt",
        );
        assert!(reservation.handoff());
        let recovery = reservation.writer_path().to_owned();
        std::fs::write(&recovery, b"completed bytes").unwrap();
        let reservation = reservation.completed();
        let queued_publication = move || reservation.publish();
        // A rejected coordination closure must retain completed native bytes.
        drop(queued_publication);
        worker::flush();
        assert_eq!(std::fs::read(recovery).unwrap(), b"completed bytes");
        assert!(!scratch.0.join("report.txt").exists());
    }

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
        let folder = admit_folder(&scratch.0).unwrap();
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
            assert_eq!(reservation.publish().unwrap(), folder.join(name));
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
    fn alternate_folder_paths_publish_long_destinations_without_overwriting() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

        let scratch = Scratch::new("short-folder");
        let folder = admit_folder(&scratch.0).unwrap();
        let wide = folder
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: the input is NUL-terminated and live through both calls;
        // the second call has the buffer size requested by the first.
        let size = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
        assert_ne!(size, 0, "the real temp directory must resolve");
        let mut buffer = vec![0; size as usize];
        let length = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), size) };
        assert!(length > 0 && length < size);
        let short = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
        // Volumes without 8.3 names return the long path unchanged. Exercise
        // the OS result either way, plus a spelling that always needs repair.
        let dotted = folder
            .parent()
            .unwrap()
            .join(".")
            .join(folder.file_name().unwrap());

        std::fs::write(folder.join("report.txt"), b"original").unwrap();
        for (input, name) in [
            (&dotted, "report (2).txt"),
            (&short, "report (4).txt"),
            (&folder, "report (6).txt"),
        ] {
            let mut reservation = Reservation::adopt(
                unique_destination(input, "report.txt", |_| false).unwrap(),
                "report.txt",
            );
            assert!(reservation.handoff());
            std::fs::write(reservation.path(), b"another application's data").unwrap();
            std::fs::write(reservation.writer_path(), b"downloaded bytes").unwrap();
            let published = reservation.publish().unwrap();
            // Path equality ignores `.` segments; require the exact spelling.
            assert_eq!(published.as_os_str(), folder.join(name).as_os_str());
            assert_eq!(std::fs::read(published).unwrap(), b"downloaded bytes");
        }
        assert_eq!(
            std::fs::read(folder.join("report.txt")).unwrap(),
            b"original"
        );
        for name in ["report (1).txt", "report (3).txt", "report (5).txt"] {
            assert_eq!(
                std::fs::read(folder.join(name)).unwrap(),
                b"another application's data"
            );
        }
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 7);
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
        assert_eq!(
            first.publish().unwrap(),
            admit_folder(&scratch.0).unwrap().join("report.txt")
        );
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
        assert_eq!(
            published,
            admit_folder(&scratch.0).unwrap().join("report (1).txt")
        );
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
        assert_eq!(
            published,
            admit_folder(&scratch.0).unwrap().join("report (2).txt")
        );
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
