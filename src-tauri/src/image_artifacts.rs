mod gc;
mod store;
#[cfg(test)]
mod test_support;

pub(crate) use gc::reconcile_image_artifacts;
pub(crate) use store::verify_image_reference;
#[cfg(test)]
pub(crate) use test_support::insert_test_image;
