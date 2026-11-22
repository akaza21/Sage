use serde_json::Value;

use super::{ClaudeSdkRuntime, Inbound, sdk_capabilities, sdk_frame};
use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressToken, ResourceMapper,
    },
    transport::request_value_digest,
};

impl ClaudeSdkRuntime {
    pub(super) fn ingest_sdk(
        &mut self,
        sdk: Value,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.started.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let mut inbound = self
            .bridge()?
            .inbound(sdk)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let mut batch = AdapterBatchBuilder::new();
        if inbound.started && self.session.harness_turn_admitted() {
            let id = self.session.harness_session_identity()?;
            self.started = Some(inbound);
            batch.push_event(AdapterEvent::TurnStarted { id })?;
            return Ok(batch.finish());
        }
        if inbound.started {
            inbound = self
                .bridge()?
                .decline()
                .map_err(|_| AdapterRuntimeError::Frame)?;
        }
        let mut observed = self.translate(&mut batch, inbound, resources)?;
        if let Some(ended) = self.bridge()?.settled_turn_end() {
            observed |= self.translate(&mut batch, ended, resources)?;
        }
        if !observed {
            batch.push_event(AdapterEvent::Absorbed)?;
        }
        Ok(batch.finish())
    }

    /// The end of a turn that only waited on the answers just delivered.
    pub(super) fn ingest_delivered(
        &mut self,
        resources: &dyn ResourceMapper,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        let Some(ended) = self.bridge()?.settled_turn_end() else {
            return Ok(None);
        };
        let mut batch = AdapterBatchBuilder::new();
        if !self.translate(&mut batch, ended, resources)? {
            batch.push_event(AdapterEvent::Absorbed)?;
        }
        Ok(Some(batch.finish()))
    }

    /// Adds what `inbound` reports to `batch`; returns whether it reported
    /// any event.
    pub(super) fn translate(
        &mut self,
        batch: &mut AdapterBatchBuilder,
        inbound: Inbound,
        resources: &dyn ResourceMapper,
    ) -> Result<bool, AdapterRuntimeError> {
        let mut observed = false;
        for wire in inbound.withdrawn {
            let (events, _) = self.session.withdraw_permission(&wire.into())?.into_parts();
            for event in events
                .into_iter()
                .filter(|event| !matches!(event, AdapterEvent::Absorbed))
            {
                observed = true;
                batch.push_event(event)?;
            }
        }
        for event in self.steering.answers(inbound.steers) {
            observed = true;
            batch.push_event(event)?;
        }
        self.answered(inbound.native, inbound.usage);
        if let Some(questions) = inbound.questions {
            self.session.offer_user_input(questions);
        }
        for event in inbound.session {
            if let Some(event) = self.session.session_event(event) {
                observed = true;
                batch.push_event(event)?;
            }
        }
        for child in inbound.children {
            for event in self
                .session
                .child_event(&mut self.children, child, resources)?
            {
                observed = true;
                batch.push_event(event)?;
            }
        }
        for acp in inbound.acp {
            let digest = request_value_digest(&acp);
            let (events, egress) = self
                .session
                .ingest_value(acp, digest, resources)?
                .into_parts();
            for event in events {
                observed = true;
                batch.push_event(match event {
                    AdapterEvent::Initialized { capabilities } => AdapterEvent::Initialized {
                        capabilities: {
                            let mut capabilities = sdk_capabilities(capabilities);
                            capabilities.goal = self.bridge()?.goal_capabilities();
                            capabilities
                        },
                    },
                    event => event,
                })?;
            }
            for egress in egress {
                batch.push_egress(self.outbound(egress)?)?;
            }
        }
        for reply in inbound.replies {
            batch.push_egress(settling(&reply)?)?;
        }
        Ok(observed)
    }
}

/// Sage's answers to Claude Code's requests, and the interrupt of a run it
/// refused, start nothing, so they are written between turns too.
pub(super) fn settling(sdk: &Value) -> Result<AdapterEgress, AdapterRuntimeError> {
    Ok(AdapterEgress::new(sdk_frame(sdk)?, EgressToken::settling()))
}
