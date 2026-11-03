use super::*;

impl PiRuntime {
    pub(super) fn ingest_frame(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        let optional_id = self
            .thinking_request_command
            .as_deref()
            .filter(|id| self.translator.sole_thinking_levels_request_id() == Some(*id));
        let record = crate::acp::bindings::pi::decode::decode_record_with_thinking_request(
            frame.as_bytes(),
            optional_id,
        );
        let thinking_reply = matches!(&record, Ok(crate::acp::bindings::pi::decode::Record::Response(response))
            if Some(&response.id) == self.thinking_request_command.as_ref() && response.command == "get_available_thinking_levels");
        let outputs = match record.and_then(|record| self.ingest_record(record)) {
            Ok(outputs) => outputs,
            Err(error) => {
                if matches!(error, AdapterError::ConfigMutationRejected) {
                    return Err(support::map_pi(error));
                }
                if self.refresh_thinking_after_model {
                    self.current_thinking = None;
                    self.available_thinking.clear();
                }
                if thinking_reply {
                    let stale = self.thinking_request_session.as_ref()
                        != self.external_session.as_ref()
                        || self.thinking_request_owner.as_ref()
                            != Some(&(self.session_id.clone(), self.runtime_epoch));
                    self.clear_thinking_request();
                    self.available_thinking.clear();
                    return AdapterBatch::new(
                        vec![if stale {
                            crate::acp::bindings::adapter::AdapterEvent::IgnoredLate
                        } else if self.active_turn.is_none() {
                            crate::acp::bindings::adapter::AdapterEvent::IgnoredDuplicate
                        } else {
                            support::unsupported(
                                "pi_thinking_levels",
                                "Pi thinking levels reply was not usable",
                            )
                        }],
                        Vec::new(),
                    );
                }
                if self.staged_initialize.take().is_some() {
                    self.poisoned = true;
                }
                return Err(support::map_pi(error));
            }
        };
        if thinking_reply
            && outputs.iter().any(|output| {
                matches!(output, crate::acp::bindings::pi::AdapterOutput::LateIgnored)
            })
        {
            self.clear_thinking_request();
        }
        let hidden_models = self.staged_initialize.is_some()
            && outputs
                .iter()
                .any(|value| matches!(value, crate::acp::bindings::pi::AdapterOutput::Models(_)));
        if hidden_models {
            self.staged_initialize = None;
            self.poisoned = true;
            return Err(AdapterRuntimeError::Frame);
        }
        let advance = self.staged_initialize.is_some()
            && outputs
                .iter()
                .any(|value| matches!(value, crate::acp::bindings::pi::AdapterOutput::State(_)));
        let failed = outputs
            .iter()
            .any(|value| matches!(value, crate::acp::bindings::pi::AdapterOutput::Error { .. }));
        if failed && self.refresh_thinking_after_model {
            self.current_thinking = None;
            self.available_thinking.clear();
        }
        if thinking_reply && failed {
            self.clear_thinking_request();
            self.available_thinking.clear();
            if self.active_turn.is_none() {
                return AdapterBatch::new(
                    vec![crate::acp::bindings::adapter::AdapterEvent::IgnoredDuplicate],
                    Vec::new(),
                );
            }
        }
        if failed && self.staged_initialize.is_some() {
            self.staged_initialize = None;
        }
        let staged = if advance {
            let outbound = self
                .staged_initialize
                .take()
                .ok_or(AdapterRuntimeError::Operation)?;
            Some(self.reserve(outbound, RequestKind::InitializeSecond)?)
        } else {
            None
        };
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        self.translate_outputs(outputs, staged, request_digest, resources)
    }
}
