//! Bounded permission-option parsing for Claude ingress.

use super::super::*;

impl ClaudeRuntime {
    pub(in crate::acp::bindings::claude::runtime) fn permission_request(
        &mut self,
        value: Value,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let (request_id, translator_id, wire_id) = permission_identity(value.get("id"))?;
        let mut published = self.drained_deferred()?;
        if let Some(record) = self.denials.get(request_id.as_str()) {
            if record.state == DenialState::Written && record.reply == PermissionReply::Deny {
                let egress = vec![denial(&request_id, &record.wire_id, true)?];
                published.push(SharedEvent::IgnoredDuplicate);
                return AdapterBatch::new(published, egress)
                    .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
            }
            published.push(SharedEvent::IgnoredDuplicate);
            return AdapterBatch::new(published, Vec::new());
        }
        let allow_once_option_id = permission_allow_option(&value, "allow_once")?;
        let allow_always_option_id = permission_allow_option(&value, "allow_always")?;
        let events = self
            .active
            .as_mut()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?
            .translator
            .translate_permission_request_value(value)
            .map_err(map_translation)?;
        let active = self
            .active
            .as_ref()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        let mut request_event = None;
        for event in events {
            if let AdapterEvent::Item {
                external_key,
                lifecycle,
                tool_state,
                payload,
            } = event
            {
                let mut payload = payload;
                if let crate::acp::intent::NormalizedPayload::PermissionRequest {
                    offered_option_kinds,
                    ..
                } = &mut payload
                    && allow_once_option_id.is_some()
                {
                    let mut kinds = offered_option_kinds
                        .as_ref()
                        .map_or_else(Vec::new, |existing| existing.as_slice().to_vec());
                    if !kinds.contains(&crate::acp::intent::PermissionOptionKind::AllowOnce)
                        && kinds.len() < 8
                    {
                        kinds.push(crate::acp::intent::PermissionOptionKind::AllowOnce);
                        *offered_option_kinds = Some(
                            crate::acp::contract::BoundedVec::new("permissionOptionKinds", kinds)
                                .map_err(|_| AdapterRuntimeError::Frame)?,
                        );
                    }
                }
                if matches!(
                    payload,
                    crate::acp::intent::NormalizedPayload::PermissionRequest { .. }
                ) {
                    request_event = Some(SharedEvent::PermissionRequest {
                        turn_id: self.turn_identity(active)?,
                        item_id: self.item_identity(active, request_id.clone())?,
                        external_request_id: request_id.clone(),
                        payload,
                        request_payload_digest: request_digest,
                    });
                } else {
                    published.push(SharedEvent::Item {
                        id: self.item_identity(active, external_key)?,
                        lifecycle,
                        tool_state,
                        payload,
                    });
                }
            }
        }
        let request_event = request_event.ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        published.push(request_event);
        self.reserve_denial(
            request_id.as_str(),
            translator_id,
            wire_id,
            allow_once_option_id,
            allow_always_option_id,
        )?;
        AdapterBatch::new(published, Vec::new())
    }

    /// The harness withdrew a request the user has not answered: its card
    /// is cancelled, so a turn may end without it.
    pub(crate) fn withdraw_permission(
        &mut self,
        wire: &Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let (request_id, _, _) = permission_identity(Some(wire))?;
        let Some(record) = self
            .denials
            .get_mut(request_id.as_str())
            .filter(|record| record.state == DenialState::Pending)
        else {
            return AdapterBatch::new(vec![SharedEvent::Absorbed], Vec::new());
        };
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        let event = active
            .translator
            .permission_cancel_committed(&record.translator_id)
            .map_err(map_translation)?;
        record.reply = PermissionReply::Cancelled;
        record.state = DenialState::Written;
        if matches!(event, AdapterEvent::Item { .. }) {
            self.deferred.push(event);
        }
        let mut published = self.drained_deferred()?;
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        published.push(SharedEvent::PermissionResolved {
            turn_id: self.turn_identity(active)?,
            external_request_id: request_id,
            state: crate::acp::lifecycle::ServerRequestState::Cancelled,
        });
        AdapterBatch::new(published, Vec::new())
    }

    fn drained_deferred(&mut self) -> Result<Vec<SharedEvent>, AdapterRuntimeError> {
        let deferred = std::mem::take(&mut self.deferred);
        let mut published = Vec::with_capacity(deferred.len().saturating_add(2));
        for event in deferred {
            let AdapterEvent::Item {
                external_key,
                lifecycle,
                tool_state,
                payload,
            } = event
            else {
                continue;
            };
            let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
            published.push(SharedEvent::Item {
                id: self.item_identity(active, external_key)?,
                lifecycle,
                tool_state,
                payload,
            });
        }
        Ok(published)
    }

    pub(in crate::acp::bindings::claude::runtime) fn reserve_denial(
        &mut self,
        request_id: &str,
        translator_id: String,
        wire_id: Value,
        allow_once_option_id: Option<String>,
        allow_always_option_id: Option<String>,
    ) -> Result<(), AdapterRuntimeError> {
        let limit = LIMITS.queues.server_requests_per_session as usize;
        if self.denials.len() >= limit {
            let oldest = self
                .denials
                .iter()
                .filter(|(_, record)| record.state == DenialState::Written)
                .min_by_key(|(_, record)| record.sequence)
                .map(|(id, _)| id.clone())
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            self.denials.remove(&oldest);
        }
        let sequence = self
            .next_denial_sequence
            .checked_add(1)
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        self.denials.insert(
            request_id.to_owned(),
            DenialRecord {
                state: DenialState::Pending,
                sequence,
                translator_id,
                wire_id,
                allow_once_option_id,
                allow_always_option_id,
                reply: PermissionReply::Deny,
            },
        );
        self.next_denial_sequence = sequence;
        Ok(())
    }
}

pub(super) fn permission_allow_option(
    value: &Value,
    wanted: &str,
) -> Result<Option<String>, AdapterRuntimeError> {
    let options = value
        .get("params")
        .and_then(Value::as_object)
        .and_then(|params| params.get("options"))
        .and_then(Value::as_array)
        .ok_or(AdapterRuntimeError::Frame)?;
    Ok(options.iter().take(8).find_map(|option| {
        let option_id = option.get("optionId")?.as_str().map(str::to_owned)?;
        if option_id.is_empty() || option_id.len() > 64 {
            return None;
        }
        let kind = option
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if kind == wanted {
            return Some(option_id);
        }
        let name = option
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.len() > 64 {
            return None;
        }
        (kind.is_empty()
            && match wanted {
                "allow_once" => {
                    matches!(name.to_ascii_lowercase().as_str(), "allow" | "allow once")
                }
                "allow_always" => name.eq_ignore_ascii_case("allow always"),
                _ => false,
            })
        .then_some(option_id)
    }))
}
