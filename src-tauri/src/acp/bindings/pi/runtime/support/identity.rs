use crate::acp::bindings::adapter::{
    AdapterEvent, AdapterIdentity, AdapterModel, AdapterRuntimeError, ResourceMapper,
};
use sage_acp_protocol::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{
        ExternalId, NormalizedPayload, SafeResourceLocation, ToolCategory,
        session_event::SessionScopedEvent,
    },
    lifecycle::{ItemLifecycle, ToolState},
};
use sage_core::limits::COMMAND_RESOURCE_LIMIT;
use sage_core::limits::LIMITS;

use super::super::PiRuntime;
use super::stream::append_snapshot;
use crate::acp::bindings::pi::ModelSummary;

impl PiRuntime {
    pub(super) fn identity(
        &self,
        external_id: ExternalId,
        turn_id: Option<ExternalId>,
        client: Option<RequiredText<512>>,
    ) -> AdapterIdentity {
        AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id,
            turn_id,
            turn_client_message_id: client,
        }
    }

    /// A session-scoped fact; dropped before Pi's session is ready.
    pub(super) fn session_event(&self, event: SessionScopedEvent) -> AdapterEvent {
        match self.external_session.clone() {
            Some(session) => AdapterEvent::Session {
                id: self.identity(session, None, None),
                event,
            },
            None => AdapterEvent::IgnoredLate,
        }
    }

    pub(super) fn current_context_window(&self) -> Option<u32> {
        let current = self.current_model.as_deref()?;
        self.available_models
            .iter()
            .find(|model| model_key(model).is_ok_and(|key| key == current))?
            .context_window
    }

    pub(super) fn turn_identity(&self) -> Result<AdapterIdentity, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        Ok(self.identity(
            turn.external_id.clone(),
            Some(turn.external_id.clone()),
            Some(turn.client_message_id.clone()),
        ))
    }

    pub(super) fn item(
        &self,
        id: ExternalId,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        Ok(AdapterEvent::Item {
            id: self.identity(
                id,
                Some(turn.external_id.clone()),
                Some(turn.client_message_id.clone()),
            ),
            lifecycle,
            tool_state,
            payload,
        })
    }

    pub(super) fn require_turn(&self, turn_key: &str) -> Result<(), AdapterRuntimeError> {
        match self.active_turn.as_ref() {
            Some(turn) if turn.external_id.as_str() == turn_key => Ok(()),
            _ => Err(AdapterRuntimeError::Operation),
        }
    }

    pub(super) fn next_local_item(
        &mut self,
        kind: &str,
    ) -> Result<ExternalId, AdapterRuntimeError> {
        self.active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        self.next_item = self
            .next_item
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Operation)?;
        external(
            "piItemId",
            format!("pi-{kind}-{}-{}", self.runtime_epoch.get(), self.next_item),
        )
    }

    pub(super) fn start_text(
        &mut self,
        value: Option<String>,
    ) -> Result<ExternalId, AdapterRuntimeError> {
        let id = match value {
            Some(value) => external("piItemId", value)?,
            None => self.next_local_item("text")?,
        };
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        if turn.text_id.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        turn.text_id = Some(id.clone());
        turn.text_snapshot = Some(String::new());
        turn.text_redacted_tail = false;
        Ok(id)
    }

    pub(super) fn append_text(
        &mut self,
        delta: String,
        redacted_tail: bool,
    ) -> Result<(ExternalId, String), AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        let id = turn.text_id.clone().ok_or(AdapterRuntimeError::Operation)?;
        let snapshot = append_snapshot(
            &mut turn.text_snapshot,
            &mut turn.text_redacted_tail,
            &delta,
            redacted_tail,
        )
        .inspect_err(|_| {
            self.poisoned = true;
        })?;
        Ok((id, snapshot))
    }

    pub(super) fn complete_text(
        &mut self,
        value: Option<String>,
    ) -> Result<(ExternalId, String), AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        let active = turn.text_id.clone().ok_or(AdapterRuntimeError::Operation)?;
        if let Some(value) = value
            && external("piItemId", value)? != active
        {
            return Err(AdapterRuntimeError::Operation);
        }
        let snapshot = turn
            .text_snapshot
            .take()
            .ok_or(AdapterRuntimeError::Operation)?;
        turn.text_id = None;
        Ok((active, snapshot))
    }

    pub(super) fn append_thought(
        &mut self,
        delta: String,
        redacted_tail: bool,
    ) -> Result<(ExternalId, String), AdapterRuntimeError> {
        if self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .thought_id
            .is_none()
        {
            let id = self.next_local_item("thought")?;
            let turn = self
                .active_turn
                .as_mut()
                .ok_or(AdapterRuntimeError::Operation)?;
            turn.thought_id = Some(id);
            turn.thought_snapshot = Some(String::new());
            turn.thought_redacted_tail = false;
        }
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        let id = turn
            .thought_id
            .clone()
            .ok_or(AdapterRuntimeError::Operation)?;
        let snapshot = append_snapshot(
            &mut turn.thought_snapshot,
            &mut turn.thought_redacted_tail,
            &delta,
            redacted_tail,
        )
        .inspect_err(|_| {
            self.poisoned = true;
        })?;
        Ok((id, snapshot))
    }

    pub(super) fn is_edit_tool(&self, external_id: &str) -> bool {
        self.active_turn
            .as_ref()
            .and_then(|turn| turn.tools.get(external_id))
            .is_some_and(|payload| {
                matches!(
                    payload,
                    NormalizedPayload::ToolCall {
                        category: ToolCategory::Edit,
                        ..
                    }
                )
            })
    }

    pub(super) fn update_tool(
        &mut self,
        external_id: &str,
        output: Option<String>,
        terminal: bool,
    ) -> Result<NormalizedPayload, AdapterRuntimeError> {
        let mut payload = self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .tools
            .get(external_id)
            .cloned()
            .ok_or(AdapterRuntimeError::Operation)?;
        if let NormalizedPayload::ToolCall { output_summary, .. } = &mut payload {
            *output_summary = output
                .map(|output| SafeText::new("toolOutput", output))
                .transpose()
                .map_err(|_| AdapterRuntimeError::Frame)?;
        }
        self.cache_tool(external_id.to_owned(), payload.clone(), terminal)?;
        Ok(payload)
    }
}

pub(super) fn model_event(
    models: Vec<ModelSummary>,
    current: Option<&str>,
) -> Result<AdapterEvent, AdapterRuntimeError> {
    if models.len() > LIMITS.wire.models_or_config_entries as usize {
        return Err(AdapterRuntimeError::BatchCount);
    }
    let mut bytes = 0_usize;
    for (index, model) in models.iter().enumerate() {
        let id = model_key(model)?;
        let name = if model.name.len() <= 512 {
            &model.name
        } else {
            &model.id
        };
        bytes = bytes
            .checked_add(id.len())
            .and_then(|value| value.checked_add(name.len()))
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if bytes > LIMITS.wire.models_or_config_bytes as usize
            || models[..index]
                .iter()
                .any(|other| model_key(other).ok().as_deref() == Some(&id))
        {
            return Err(AdapterRuntimeError::BatchBytes);
        }
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(models.len())
        .map_err(|_| AdapterRuntimeError::BatchBytes)?;
    for model in models {
        let id = model_key(&model)?;
        let is_default = current == Some(id.as_str());
        let name = if model.name.len() <= 512 {
            model.name
        } else {
            model.id.clone()
        };
        output.push(AdapterModel {
            id: RequiredText::new("modelId", id).map_err(|_| AdapterRuntimeError::Frame)?,
            name: RequiredText::new("modelName", name).map_err(|_| AdapterRuntimeError::Frame)?,
            is_default,
        });
    }
    Ok(AdapterEvent::Models {
        models: output,
        next_cursor: None,
    })
}

pub(super) fn external(
    field: &'static str,
    value: String,
) -> Result<ExternalId, AdapterRuntimeError> {
    ExternalId::new(field, value).map_err(|_| AdapterRuntimeError::Frame)
}

pub(super) fn text_payload(value: impl Into<String>) -> NormalizedPayload {
    NormalizedPayload::Text {
        text: SafeText::new("text", value).expect("Pi translator enforces text bound"),
    }
}

pub(super) fn tool_start_payload(
    name: String,
    path: Option<&str>,
    resources: &dyn ResourceMapper,
) -> Result<NormalizedPayload, AdapterRuntimeError> {
    let category = match name.as_str() {
        "read" => ToolCategory::Read,
        "edit" | "write" => ToolCategory::Edit,
        "bash" | "powershell" => ToolCategory::Execute,
        "grep" | "find" | "ls" => ToolCategory::Search,
        _ => ToolCategory::Other,
    };
    let mapped = path.map(|path| resources.map_existing(path));
    let relative = mapped
        .as_ref()
        .and_then(|mapped| mapped.as_ref().ok())
        .map(|resource| resource.relative_path.as_str());
    let label = tool_label(&name, relative);
    let locations = match mapped {
        Some(mapped) => {
            let location = match mapped {
                Ok(resource) => SafeResourceLocation::Resource { resource },
                Err(reason) => SafeResourceLocation::unavailable(reason),
            };
            BoundedVec::<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>::new(
                "locations",
                vec![location],
            )
            .map_err(|_| AdapterRuntimeError::Frame)?
        }
        None => Default::default(),
    };
    Ok(NormalizedPayload::ToolCall {
        execution: Default::default(),
        category,
        label: SafeText::new("toolLabel", label).map_err(|_| AdapterRuntimeError::Frame)?,
        input_summary: None,
        output_summary: None,
        locations,
        command_activity: Default::default(),
    })
}

fn tool_label(name: &str, path: Option<&str>) -> String {
    let (verb, target) = match name {
        "read" => ("Read", "a file"),
        "edit" => ("Edited", "a file"),
        "write" => ("Wrote", "a file"),
        "grep" | "find" => ("Searched", "files"),
        "ls" => ("Listed", "a folder"),
        "bash" | "powershell" => return "Shell command".into(),
        _ => return name.into(),
    };
    format!("{verb} {}", path.unwrap_or(target))
}

pub(in crate::acp::bindings::pi::runtime) fn unsupported(
    kind: impl Into<String>,
    summary: impl Into<String>,
) -> AdapterEvent {
    AdapterEvent::Unsupported(unsupported_payload(kind, summary))
}

pub(super) fn unsupported_payload(
    kind: impl Into<String>,
    summary: impl Into<String>,
) -> NormalizedPayload {
    NormalizedPayload::Unsupported {
        external_kind: SafeText::new("externalKind", kind).expect("Pi kind is bounded"),
        summary: SafeText::new("unsupportedSummary", summary).expect("fixed bounded summary"),
    }
}

pub(super) fn model_key(model: &ModelSummary) -> Result<String, AdapterRuntimeError> {
    let key = if model.provider.is_empty() {
        model.id.clone()
    } else {
        format!("{}:{}", model.provider, model.id)
    };
    if key.is_empty() || key.len() > 512 {
        return Err(AdapterRuntimeError::Frame);
    }
    Ok(key)
}
