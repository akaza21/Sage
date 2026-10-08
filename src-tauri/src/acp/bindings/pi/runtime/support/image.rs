use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterRuntimeError},
    intent::ImageMediaType,
    lifecycle::ItemLifecycle,
};

use super::{PiRuntime, unsupported};

impl PiRuntime {
    pub(super) fn image_event(
        &mut self,
        media_type: String,
        data: String,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let Some(media_type) = ImageMediaType::parse(&media_type) else {
            return Ok(unsupported("pi_image", "Image media type is not admitted"));
        };
        let payload = match crate::acp::image_admit::admit_image_parts(media_type, &data) {
            Ok(payload) => payload,
            Err(reason) => return Ok(unsupported("pi_image", reason.unsupported_summary())),
        };
        let id = self.next_local_item("image")?;
        self.item(id, ItemLifecycle::Completed, None, payload)
    }
}
