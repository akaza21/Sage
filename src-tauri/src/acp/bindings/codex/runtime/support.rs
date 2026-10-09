use super::super::{request::OutgoingRequest, state::CodexAdapter};
use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterEgress, AdapterEvent, AdapterIdentity, AdapterModel,
        AdapterRuntimeError, EgressToken, ResourceMapper,
    },
    intent::ExternalId,
    limits::LIMITS,
    transport::OutboundFrame,
};

impl super::CodexRuntime {
    fn next_id(&mut self) -> Result<u64, AdapterRuntimeError> {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .filter(|id| *id <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Operation)?;
        Ok(self.next_request_id)
    }

    pub(super) fn request(
        &mut self,
        build: impl FnOnce(&mut CodexAdapter, u64) -> Result<OutgoingRequest, AdapterError>,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.request_with_control(build, false)
    }

    pub(super) fn request_with_control(
        &mut self,
        build: impl FnOnce(&mut CodexAdapter, u64) -> Result<OutgoingRequest, AdapterError>,
        control: bool,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let id = self.next_id()?;
        let request = build(&mut self.inner, id).map_err(map_adapter)?;
        match OutboundFrame::new(request.bytes) {
            Ok(frame) => Ok(AdapterEgress::new(
                frame,
                if control {
                    EgressToken::cancel_request(id)
                } else {
                    EgressToken::request(id)
                },
            )),
            Err(_) => {
                self.inner.reject_before_write(id).map_err(map_adapter)?;
                Err(AdapterRuntimeError::Frame)
            }
        }
    }
}

use super::super::{
    incoming::{IngressError, ModelDescriptor},
    state::{AdapterError, ScopedExternalId},
};

pub(super) struct MapperBridge<'a>(pub(super) &'a dyn ResourceMapper);

impl super::super::ResourceMapper for MapperBridge<'_> {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        self.0.map_existing(absolute_path)
    }

    fn map_missing_display(
        &self,
        absolute_path: &str,
    ) -> Option<crate::acp::intent::SafeResourceRef> {
        self.0.map_missing_display(absolute_path)
    }
}

pub(super) fn identity(value: ScopedExternalId) -> AdapterIdentity {
    AdapterIdentity {
        binding_digest: value.binding_digest,
        runtime_epoch: value.runtime_epoch,
        session_id: value.session_id,
        external_id: value.external_id,
        turn_id: value.turn_id,
        turn_client_message_id: value.turn_client_message_id,
    }
}

pub(super) fn denial_request_id(bytes: &[u8]) -> Result<ExternalId, AdapterRuntimeError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
    let id = match value.get("id") {
        Some(serde_json::Value::String(id)) => id.clone(),
        Some(serde_json::Value::Number(id)) => id.to_string(),
        _ => return Err(AdapterRuntimeError::Frame),
    };
    ExternalId::new("externalRequestId", id).map_err(|_| AdapterRuntimeError::Frame)
}

pub(super) fn one(event: AdapterEvent) -> Result<AdapterBatch, AdapterRuntimeError> {
    AdapterBatch::new(vec![event], Vec::new())
}

pub(super) fn model_values(
    models: Vec<ModelDescriptor>,
) -> Result<Vec<AdapterModel>, AdapterRuntimeError> {
    if models.len() > LIMITS.wire.models_or_config_entries as usize {
        return Err(AdapterRuntimeError::BatchCount);
    }
    let bytes = models
        .iter()
        .try_fold(0_usize, |total, model| {
            total
                .checked_add(model.id.as_str().len())?
                .checked_add(model.name.as_str().len())
        })
        .ok_or(AdapterRuntimeError::BatchBytes)?;
    if bytes > LIMITS.wire.models_or_config_bytes as usize {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(models.len())
        .map_err(|_| AdapterRuntimeError::BatchBytes)?;
    output.extend(models.into_iter().map(|model| AdapterModel {
        id: model.id,
        name: model.name,
        is_default: model.is_default,
    }));
    Ok(output)
}

pub(super) fn config_for_model(
    model: &ModelDescriptor,
    current: &str,
) -> Option<crate::acp::session_config::SessionConfig> {
    if model.supported_efforts.is_empty() {
        return None;
    }
    let options: Vec<_> = model
        .supported_efforts
        .iter()
        .map(|effort| {
            let name = effort.id.as_str();
            let mut chars = name.chars();
            let label = chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default();
            serde_json::json!({
                "value":name,
                "name":label,
                "description":effort.description
            })
        })
        .collect();
    crate::acp::session_config::SessionConfig::from_wire(
        &serde_json::json!({
            "modes":null,
            "configOptions":[{
                "id":"effort",
                "name":"Thinking level",
                "category":"effort",
                "type":"select",
            "currentValue":current,
                "options":options
            }]
        }),
        true,
    )
    .ok()
}

pub(super) fn map_adapter(error: AdapterError) -> AdapterRuntimeError {
    match error {
        AdapterError::Ingress(IngressError::FrameBytes | IngressError::Malformed) => {
            AdapterRuntimeError::Frame
        }
        AdapterError::PermissionTeardownRequired
        | AdapterError::Ingress(
            IngressError::PermissionTeardownRequired | IngressError::UnsupportedServerRequest,
        ) => AdapterRuntimeError::MandatoryDenyTeardown,
        _ => AdapterRuntimeError::Operation,
    }
}
