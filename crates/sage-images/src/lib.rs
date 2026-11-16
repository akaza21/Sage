//! Images attached to a Chat: the typed attachment reference, admission of
//! the images a harness sends, and the image artifact store beside the
//! database, with normalisation, chip thumbnails, sent-image retirement and
//! startup reconciliation.
//!
//! Invariant: no Tauri, no `AppState` and no `#[tauri::command]`. The upload
//! commands live in the app (`image_artifacts_commands`), which owns the
//! active Project and calls the functions here with explicit arguments.

pub mod admit;
pub mod attachment;
mod files;
mod gc;
mod normalize;
mod sent;
mod store;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod thumbnail;
mod work;

pub use files::remove_image_file;
pub use gc::reconcile_image_artifacts;
pub use normalize::{
    Bounds, PREVIEW as PREVIEW_IMAGE_BOUNDS, PROMPT as PROMPT_IMAGE_BOUNDS, byte_size,
    normalize as normalize_image,
};
pub use sent::retire_sent_images;
#[cfg(feature = "test-support")]
pub use store::{full_path, thumb_path};
pub use store::{
    ingest_image, normalize_upload, read_image_thumbnail, remove_pending_image,
    verify_image_reference,
};
#[cfg(any(test, feature = "test-support"))]
pub use test_support::{
    ingest_test_image, insert_test_image, noise_png, png_dimensions, screenshot_png,
};
#[cfg(feature = "test-support")]
pub use work::image_work_available;
pub use work::image_work_permit;

pub const ECHOED_PROMPT_IMAGE_BOUNDS: Bounds = Bounds {
    bytes: sage_core::limits::ECHOED_PROMPT_IMAGE_TOTAL_BYTES,
    ..PROMPT_IMAGE_BOUNDS
};
