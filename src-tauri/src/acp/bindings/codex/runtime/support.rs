use super::super::{
    request::{OutgoingRequest, RequestKind},
    state::CodexAdapter,
};
use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterEgress, AdapterEvent, AdapterIdentity, AdapterModel,
        AdapterRuntimeError, EgressToken, ResourceMapper,
    },
    intent::ExternalId,
    transport::OutboundFrame,
};
use sage_core::limits::LIMITS;

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
        let frame = if request.kind == RequestKind::TurnStart {
            OutboundFrame::new_prompt(request.bytes)
        } else {
            OutboundFrame::new(request.bytes)
        };
        match frame {
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

    fn read_diff_context(&self, resource: &crate::acp::intent::SafeResourceRef) -> Option<String> {
        self.0.read_diff_context(resource)
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
    models: &[ModelDescriptor],
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
    output.extend(models.iter().map(|model| AdapterModel {
        id: model.id.clone(),
        name: model.name.clone(),
        is_default: model.is_default,
    }));
    Ok(output)
}

pub(super) fn effort_option(model: &ModelDescriptor, current: &str) -> Option<serde_json::Value> {
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
    let default = model.default_effort.as_ref().filter(|default| {
        model
            .supported_efforts
            .iter()
            .any(|effort| effort.id == **default)
    });
    let mut option = serde_json::json!({
        "id":"effort", "name":"Thinking level", "category":"effort", "type":"select",
        "currentValue":current, "options":options
    });
    if let Some(default) = default {
        option["defaultValue"] = serde_json::json!(default);
    }
    Some(option)
}

/// A model Codex did not list (one its picker hides) is still the thread's,
/// so it is offered first, named by its id.
pub(super) fn model_option(
    catalog: &[ModelDescriptor],
    current: &crate::acp::intent::ModelId,
) -> serde_json::Value {
    let choice = |id: &str, name: &str| serde_json::json!({"value":id,"name":name});
    let unlisted = (!catalog.iter().any(|model| model.id == *current))
        .then(|| choice(current.as_str(), current.as_str()));
    let options: Vec<_> = unlisted
        .into_iter()
        .chain(
            catalog
                .iter()
                .map(|model| choice(model.id.as_str(), model.name.as_str())),
        )
        .collect();
    serde_json::json!({
        "id":"model", "name":"Model", "category":"model", "type":"select",
        "currentValue":current.as_str(), "options":options
    })
}

/// `base` with each of `options`, in order, that still fits beside the ones
/// before it, so an option over the config's bounds drops only itself.
pub(super) fn thread_config(
    base: crate::acp::session_config::SessionConfig,
    options: impl IntoIterator<Item = serde_json::Value>,
) -> crate::acp::session_config::SessionConfig {
    let Ok(mut wire) = serde_json::to_value(&base) else {
        return base;
    };
    let mut config = base;
    let mut kept = Vec::new();
    for option in options {
        kept.push(option);
        wire["configOptions"] = serde_json::Value::from(kept.clone());
        match serde_json::from_value::<crate::acp::session_config::SessionConfig>(wire.clone())
            .ok()
            .filter(|candidate| candidate.validate().is_ok())
        {
            Some(candidate) => config = candidate,
            None => {
                kept.pop();
            }
        }
    }
    config
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
