//! A fork before a user prompt: fork through its native turn, then cut only
//! the new thread. No Sage Chat is installed until both replies succeed.
use super::*;
use crate::acp::goal::NativeRpcReply;
use sage_acp_protocol::{intent::ExternalId, session_control::ConversationTarget};
use serde_json::Value;

pub(super) enum ForkCut {
    Forking { turn: String },
    Cutting { request: u64, fork: ExternalId },
}

impl CodexRuntime {
    pub(super) fn fork_before(
        &mut self,
        target: &ConversationTarget,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.fork_cut.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        if self.inner.fork_ledger_full() {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let turn = self.inner.native_target(target).map_err(map_adapter)?;
        let egress = self.request(|adapter, id| adapter.fork_at(id, Some(&turn)))?;
        self.fork_cut = Some(ForkCut::Forking { turn });
        Ok(egress)
    }

    pub(super) fn rewind_ingress(
        &mut self,
        value: &Value,
    ) -> Option<Result<AdapterBatch, AdapterRuntimeError>> {
        let id = value.get("id")?.as_u64()?;
        if value.get("method").is_some() {
            return None;
        }
        match self.fork_cut.as_ref()? {
            ForkCut::Forking { .. } if self.inner.fork_pending() == Some(id) => {
                Some(self.fork_cut_started(value, id))
            }
            ForkCut::Cutting { request, .. } if *request == id => {
                Some(self.fork_cut_finished(value, id))
            }
            _ => None,
        }
    }

    fn fork_cut_started(
        &mut self,
        value: &Value,
        id: u64,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let Some(ForkCut::Forking { turn }) = self.fork_cut.take() else {
            return Err(AdapterRuntimeError::Operation);
        };
        let reply = super::super::state::fork_reply(value, self.inner.bound_thread_id());
        self.inner.complete_fork(id).map_err(map_adapter)?;
        let NativeRpcReply::Forked(fork) = reply else {
            self.native_rpc_reply = Some(reply);
            return one(AdapterEvent::Absorbed);
        };
        self.inner.remember_fork(fork.clone());
        let egress = self.request(|adapter, id| adapter.cut_fork(id, &fork, &turn))?;
        let request = serde_json::from_slice::<Value>(egress.frame().payload())
            .map_err(|_| AdapterRuntimeError::Frame)?["id"]
            .as_u64()
            .ok_or(AdapterRuntimeError::Frame)?;
        self.fork_cut = Some(ForkCut::Cutting { request, fork });
        AdapterBatch::new(vec![AdapterEvent::Absorbed], vec![egress])
    }

    fn fork_cut_finished(
        &mut self,
        value: &Value,
        id: u64,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let Some(ForkCut::Cutting { fork, .. }) = self.fork_cut.take() else {
            return Err(AdapterRuntimeError::Operation);
        };
        self.inner.complete_fork_cut(id).map_err(map_adapter)?;
        self.native_rpc_reply = Some(
            if value.get("result").is_some() && value.get("error").is_none() {
                NativeRpcReply::Forked(fork)
            } else {
                self.inner.defer_fork_release(fork.as_str(), true);
                NativeRpcReply::Rejected
            },
        );
        one(AdapterEvent::Absorbed)
    }
}
