pub(crate) mod commands;
mod gc;
mod normalize;
mod sent;
mod store;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub(crate) use gc::reconcile_image_artifacts;
pub(crate) use normalize::{
    PREVIEW as PREVIEW_IMAGE_BOUNDS, PROMPT as PROMPT_IMAGE_BOUNDS, byte_size,
    normalize as normalize_image,
};
pub(crate) use sent::retire_sent_images;
pub(crate) use store::verify_image_reference;
#[cfg(test)]
pub(crate) use test_support::{
    ingest_test_image, insert_test_image, noise_png, png_dimensions, screenshot_png,
};
