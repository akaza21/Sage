//! Ownership of a silent save's placeholder while its download runs.
//!
//! - Dropping a [`Reservation`] anywhere (a canceled or failed download, a
//!   tab closed while planning, a hand-back to a closed webview that never
//!   runs) queues a delete of the placeholder through its own handle on the
//!   download worker, only if it is still empty.
//! - [`Reservation::keep`] is for a download that finished there: the
//!   handle is closed and the file left alone.
//!
//! Nothing is recorded and nothing is swept after a crash; see
//! [`super::placeholder`].

use std::path::Path;

use super::{placeholder::Placeholder, worker};

#[derive(Debug)]
pub(crate) struct Reservation(Option<Placeholder>);

impl Reservation {
    pub(crate) fn adopt(placeholder: Placeholder) -> Self {
        Self(Some(placeholder))
    }

    pub(crate) fn path(&self) -> &Path {
        &self
            .0
            .as_ref()
            .expect("a live reservation has a placeholder")
            .path
    }

    /// The download finished writing here: close the handle, keep the file.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn keep(mut self) {
        self.0.take();
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(placeholder) = self.0.take() {
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
    fn dropping_a_reservation_deletes_its_empty_placeholder() {
        let scratch = Scratch::new("drop");
        let reservation =
            Reservation::adopt(unique_destination(&scratch.0, "file.bin", |_| false).unwrap());
        let path = reservation.path().to_owned();
        assert!(path.exists());
        // Dropped unrun, as a closure sent to a closed webview would be.
        let closure = move || drop(reservation);
        drop(closure);
        worker::flush();
        assert!(!path.exists());
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
