use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterRuntimeError},
    lifecycle::ItemLifecycle,
};

use super::{PiRuntime, identity::unsupported_payload};

impl PiRuntime {
    pub(super) fn image_event(&mut self) -> Result<AdapterEvent, AdapterRuntimeError> {
        let payload = unsupported_payload(
            "pi_image",
            "Image output is unavailable: shared durable image admission is missing",
        );
        let id = self.next_local_item("image")?;
        self.item(id, ItemLifecycle::Unsupported, None, payload)
    }
}
