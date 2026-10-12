use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterRuntimeError},
    image_admit::{ImageAdmitError, admit_image_parts},
    intent::ImageMediaType,
    lifecycle::ItemLifecycle,
};

use super::{PiRuntime, identity::unsupported_payload};

impl PiRuntime {
    pub(super) fn image_event(
        &mut self,
        media_type: String,
        data: String,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let admitted = ImageMediaType::parse(&media_type)
            .ok_or(ImageAdmitError::UnsupportedMediaType)
            .and_then(|media_type| admit_image_parts(media_type, &data));
        let (lifecycle, payload) = match admitted {
            Ok(payload) => (ItemLifecycle::Completed, payload),
            Err(reason) => (
                ItemLifecycle::Unsupported,
                unsupported_payload("pi_image", reason.unsupported_summary()),
            ),
        };
        let id = self.next_local_item("image")?;
        self.item(id, lifecycle, None, payload)
    }
}
