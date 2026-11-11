use super::*;
use crate::acp::bindings::adapter::ResourceMapper;
use crate::acp::intent::ExternalId;
use crate::acp::transport::{Frame, request_payload_digest};

impl CodexRuntime {
    pub(super) fn ingest_frame(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.poisoned {
            return Err(AdapterRuntimeError::Operation);
        }
        if let Some(pending) = self.inner.fork_pending() {
            let value: serde_json::Value =
                serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
            if value.get("id").and_then(serde_json::Value::as_u64) == Some(pending)
                && value.get("method").is_none()
            {
                let reply = super::super::state::fork_reply(&value, self.inner.bound_thread_id());
                self.inner.complete_fork(pending).map_err(map_adapter)?;
                if let crate::acp::goal::NativeRpcReply::Forked(thread) = &reply {
                    self.inner.remember_fork(thread.clone());
                }
                self.native_rpc_reply = Some(reply);
                return one(AdapterEvent::IgnoredLate);
            }
        }
        if let Some((pending, _)) = self.inner.fork_release_pending() {
            let value: serde_json::Value =
                serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
            if value.get("id").and_then(serde_json::Value::as_u64) == Some(pending)
                && value.get("method").is_none()
            {
                let released = value.get("result").is_some() && value.get("error").is_none();
                let background = self
                    .inner
                    .complete_fork_release(pending, released)
                    .map_err(map_adapter)?;
                // A deferred release has no waiter; it never answers an RPC.
                if !background {
                    self.native_rpc_reply = Some(if released {
                        crate::acp::goal::NativeRpcReply::Released
                    } else {
                        crate::acp::goal::NativeRpcReply::Rejected
                    });
                }
                return one(AdapterEvent::IgnoredLate);
            }
        }
        if self.inner.child_read_pending() || self.inner.is_child_read_tombstone_id_active() {
            let value: serde_json::Value =
                serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
            if let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) {
                if self.inner.is_child_read_tombstone(id)
                    && value.get("method").is_none()
                    && (value.get("result").is_some() || value.get("error").is_some())
                {
                    return one(AdapterEvent::IgnoredLate);
                }
                if let Some((pending, child, parent)) = self.inner.child_read_scope()
                    && id == pending
                {
                    use super::super::child_transcript::{
                        ChildTranscriptError, normalize_read_response,
                    };
                    use crate::acp::goal::NativeRpcReply;
                    let reply = match normalize_read_response(
                        frame.as_bytes(),
                        id,
                        child,
                        parent,
                        resources,
                    ) {
                        Ok(snapshot) => NativeRpcReply::ChildTranscript(snapshot),
                        Err(ChildTranscriptError::Capacity) => NativeRpcReply::TooLarge,
                        Err(ChildTranscriptError::Rejected) => NativeRpcReply::Rejected,
                        Err(ChildTranscriptError::Malformed) => {
                            return Err(AdapterRuntimeError::Frame);
                        }
                    };
                    self.inner.complete_child_read(id).map_err(map_adapter)?;
                    self.native_rpc_reply = Some(reply);
                    return one(AdapterEvent::IgnoredLate);
                }
            }
        }
        let disposition = ingest_disposition(
            &mut self.inner,
            frame,
            resources,
            &self.declared_test_commands,
        )?;
        self.record_browser_failure_diagnostic(&disposition, frame.as_bytes());
        let request_digest =
            request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?;
        let batch = self.translate(disposition, request_digest)?;
        if !self.inner.take_deferred_interrupt() {
            return Ok(batch);
        }
        let (events, mut egress) = batch.into_parts();
        egress.push(self.request_with_control(CodexAdapter::cancel, true)?);
        AdapterBatch::new(events, egress)
    }

    /// Runs `frames`, in order, through the protocol ingestion `ingest_frame`
    /// applies, against one copy of the adapter state, and reports whether
    /// each would be read as a duplicate terminal that leaves the state as it
    /// was. Only a repeated `turn/completed` counts: a repeated item terminal
    /// also reads as DuplicateTerminal, but it is not a Turn boundary. A
    /// DuplicateTerminal outcome alone is not enough: a late reply can read
    /// as one while it settles a pending RPC. While a native RPC reply
    /// is pending, `ingest_frame` may consume a frame before that ingestion,
    /// so the probe answers false. Diagnostics stay silent: the trial is not
    /// the read, and the real read reports them.
    pub(super) fn ingests_as_duplicate_terminals(
        &self,
        frames: &[Frame],
        resources: &dyn ResourceMapper,
    ) -> bool {
        if self.poisoned
            || self.inner.fork_pending().is_some()
            || self.inner.fork_release_pending().is_some()
            || self.inner.child_read_pending()
            || self.inner.is_child_read_tombstone_id_active()
        {
            return false;
        }
        let mut probe = self.inner.clone();
        super::super::incoming::trial_read(|| {
            frames.iter().all(|frame| {
                is_turn_terminal(frame)
                    && matches!(
                        ingest_disposition(
                            &mut probe,
                            frame,
                            resources,
                            &self.declared_test_commands
                        ),
                        Ok(IngressDisposition::DuplicateTerminal)
                    )
                    && request_payload_digest(frame).is_ok()
                    && probe == self.inner
            })
        })
    }
}

/// Whether `frame` is a Turn terminal notification. Checked before the trial
/// read, so no other frame is mapped or diffed by the probe.
fn is_turn_terminal(frame: &Frame) -> bool {
    serde_json::from_slice::<serde_json::Value>(frame.as_bytes())
        .ok()
        .and_then(|value| value.get("method")?.as_str().map(|m| m == "turn/completed"))
        .unwrap_or(false)
}

fn ingest_disposition(
    inner: &mut CodexAdapter,
    frame: &Frame,
    resources: &dyn ResourceMapper,
    declarations: &DeclaredTestCommands,
) -> Result<IngressDisposition, AdapterRuntimeError> {
    inner
        .ingest_with_declarations(frame.as_bytes(), &MapperBridge(resources), declarations)
        .map_err(map_adapter)
}

impl CodexRuntime {
    /// `thread/fork`; a full fork ledger is a capacity refusal (BatchCount)
    /// the service turns into a reason the user can act on.
    /// Undo removes the latest Turn from Codex's history; files stay.
    pub(super) fn control_request(
        &mut self,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let crate::acp::session_control::SessionControl::UndoTurn { turn_id } = control else {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        };
        self.native_rpc_reply = None;
        self.request(|adapter, id| adapter.revert(id, turn_id.as_str()))
    }

    pub(super) fn fork_request(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.native_rpc_reply = None;
        if self.inner.fork_ledger_full() {
            return Err(AdapterRuntimeError::BatchCount);
        }
        self.request(CodexAdapter::fork)
    }

    /// Unloads a fork thread from this app-server: `thread/unsubscribe` once
    /// its fork Chat is committed (it resumes in its own runtime), and
    /// `thread/delete` once refused, which also removes the orphan rollout.
    pub(super) fn release_fork_request(
        &mut self,
        fork: &ExternalId,
        committed: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.native_rpc_reply = None;
        // A deferred release holds the release slot: this one is wanted for
        // the next idle moment and the caller is answered now.
        if self.inner.queue_fork_release(fork.as_str(), !committed) {
            self.native_rpc_reply = Some(crate::acp::goal::NativeRpcReply::Released);
            return Ok(None);
        }
        self.request(|adapter, id| adapter.release_fork(id, fork.as_str(), !committed))
            .map(Some)
    }

    /// A Turn is running: the release waits for the next idle moment, and
    /// the caller's RPC is answered now.
    pub(super) fn defer_fork_release_request(
        &mut self,
        fork: &ExternalId,
        committed: bool,
    ) -> bool {
        let deferred = self.inner.defer_fork_release(fork.as_str(), !committed);
        if deferred {
            self.native_rpc_reply = Some(crate::acp::goal::NativeRpcReply::Released);
        }
        deferred
    }

    pub(super) fn deferred_fork_release_request(
        &mut self,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        if !self.inner.has_deferred_fork_release() {
            return Ok(None);
        }
        self.request(CodexAdapter::next_deferred_fork_release)
            .map(Some)
    }
}
