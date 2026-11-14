pub(crate) mod commands;
mod gc;
mod normalize;
mod sent;
mod store;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod work;

pub(crate) use gc::reconcile_image_artifacts;
pub(crate) use normalize::{
    Bounds, PREVIEW as PREVIEW_IMAGE_BOUNDS, PROMPT as PROMPT_IMAGE_BOUNDS, byte_size,
    normalize as normalize_image,
};
pub(crate) use sent::retire_sent_images;
pub(crate) use store::verify_image_reference;
#[cfg(test)]
pub(crate) use test_support::{
    ingest_test_image, insert_test_image, noise_png, png_dimensions, screenshot_png,
};
pub(crate) use work::image_work_permit;

/// What an image prompt to a harness that echoes its images sends.
pub(crate) const ECHOED_PROMPT_IMAGE_BOUNDS: Bounds = Bounds {
    bytes: sage_core::limits::ECHOED_PROMPT_IMAGE_TOTAL_BYTES,
    ..PROMPT_IMAGE_BOUNDS
};
