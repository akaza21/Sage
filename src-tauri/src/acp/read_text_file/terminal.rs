use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use tokio::sync::oneshot;

use crate::acp::{
    resource_mapper::NativeResourceAdmission,
    runtime::read_text_file::{ReadTextRange, ReadTextRequestId},
    transport::P02Cancellation,
};
use sage_acp_protocol::{
    contract::{BoundedVec, SafeText},
    intent::{NormalizedPayload, ToolCategory},
};
use sage_domain::resource::ResourceRef;
use sage_workspace::Workspace;

use super::{
    DiskAllowedState, ReadTextClassification, ReadTextFlushState, ReadTextNextAction, ReadTextOwner,
};

pub(super) const MAX_PENDING_READS: usize = 16;
pub(super) type ReadTextResult<T> = Result<T, Box<ReadTextFailure>>;
const READ_LABEL: &str = "fs/read_text_file";

pub(super) struct ClassificationRequest<'a> {
    pub(super) owner: &'a ReadTextOwner,
    pub(super) resource: &'a ResourceRef,
    pub(super) token: &'a str,
    pub(super) expected_revision: Option<u64>,
    pub(super) classification: ReadTextClassification,
    pub(super) editor_revision: Option<u64>,
    pub(super) durable_revision: Option<u64>,
}

pub(super) struct PendingRead {
    pub(super) owner: ReadTextOwner,
    pub(super) resource: ResourceRef,
    pub(super) expected_editor_revision: Option<u64>,
    pub(super) workspace: Workspace,
    pub(super) admission: Option<NativeResourceAdmission>,
    pub(super) range: ReadTextRange,
    pub(super) deadline: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AttemptPhase {
    Waiting,
    Reading,
    Leased,
    Delivering,
    Revoked,
}

#[allow(dead_code, reason = "P-02 Decision B delivery state")]
enum Terminal {
    Failure(Box<ReadTextFailure>),
    Success(Box<ReadTextCompletion>),
}
#[derive(Clone, Copy)]
pub(super) struct Classification {
    pub(super) state: ReadTextClassification,
    pub(super) editor_revision: Option<u64>,
}
struct Attempt {
    pending: PendingRead,
    classification: Option<Classification>,
    phase: AttemptPhase,
    terminal: Option<Terminal>,
    pending_terminal: Option<Terminal>,
    cancellation: P02Cancellation,
    notify: Option<oneshot::Sender<()>>,
}

#[allow(dead_code, reason = "P-02 adapter receipt handoff")]
#[derive(Debug)]
pub(crate) struct TerminalReceipt {
    pub(crate) owner: ReadTextOwner,
    pub(crate) token: String,
    pub(crate) resource: ResourceRef,
    pub(crate) range: ReadTextRange,
    pub(crate) payload: NormalizedPayload,
}

mod sink;
pub(crate) use sink::ReadTextReceiptSink;
pub(crate) struct ReadState {
    inner: Mutex<ReadStateInner>,
    changed: Condvar,
}

pub(crate) struct ReadTextSettlementHandle {
    state: Arc<ReadState>,
    tokens: BTreeSet<String>,
}
impl ReadTextSettlementHandle {
    pub(crate) fn wait_until(&self, deadline: Instant) -> bool {
        let mut inner = state_machine::lock(&self.state);
        loop {
            if !inner.attempts.iter().any(|(token, attempt)| {
                self.tokens.contains(token) && attempt.phase == AttemptPhase::Delivering
            }) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let timeout = deadline.saturating_duration_since(now);
            let (next, _) = self
                .state
                .changed
                .wait_timeout(inner, timeout)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner = next;
        }
    }
}
struct ReadStateInner {
    attempts: BTreeMap<String, Attempt>,
    reservations: BTreeMap<String, String>,
    receipts: Vec<TerminalReceipt>,
}
impl ReadState {
    pub(super) fn new() -> Self {
        Self {
            inner: Mutex::new(ReadStateInner {
                attempts: BTreeMap::new(),
                reservations: BTreeMap::new(),
                receipts: Vec::with_capacity(MAX_PENDING_READS),
            }),
            changed: Condvar::new(),
        }
    }
}

#[allow(dead_code, reason = "P-02 Decision B receiver handoff")]
pub(crate) struct ReadTextResponseReceiver {
    state: Arc<ReadState>,
    token: String,
    deadline: Instant,
    receiver: Option<oneshot::Receiver<()>>,
}
#[allow(dead_code, reason = "P-02 Decision B receiver handoff")]
impl ReadTextResponseReceiver {
    pub(super) fn new(
        state: Arc<ReadState>,
        token: String,
        deadline: Instant,
        receiver: oneshot::Receiver<()>,
    ) -> Self {
        Self {
            state,
            token,
            deadline,
            receiver: Some(receiver),
        }
    }
    pub(crate) fn blocking_receive(mut self) -> ReadTextResult<ReadTextCompletionLease> {
        let mut receiver = self.receiver.take().expect("receiver consumed once");
        loop {
            match receiver.try_recv() {
                Ok(()) => return claim_receiver(&self.state, &self.token),
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
            if Instant::now() >= self.deadline {
                expire_all(&self.state);
            }
            let inner = state_machine::lock(&self.state);
            if Instant::now() >= self.deadline {
                continue;
            }
            let timeout = self.deadline.saturating_duration_since(Instant::now());
            let _ = self
                .state
                .changed
                .wait_timeout(inner, timeout)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}
impl Drop for ReadTextResponseReceiver {
    fn drop(&mut self) {
        if self.receiver.take().is_some() {
            receiver_dropped(&self.state, &self.token);
        }
    }
}

#[allow(dead_code, reason = "P-02 Decision B begin handoff")]
pub(crate) struct ReadTextBegin {
    pub(crate) event: super::ReadTextFlushEvent,
    pub(crate) receiver: ReadTextResponseReceiver,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadTextFailureKind {
    Busy,
    Unavailable,
    Timeout,
    Cancelled,
    Forbidden,
    Corrupt,
}
#[allow(dead_code, reason = "P-02 typed terminal failure")]
#[derive(Debug, Clone)]
pub(crate) struct ReadTextFailure {
    pub(crate) kind: ReadTextFailureKind,
    pub(crate) receipt: NormalizedPayload,
}
impl ReadTextFailure {
    pub(super) fn terminal(kind: ReadTextFailureKind) -> Box<Self> {
        Box::new(Self {
            kind,
            receipt: receipt(
                None,
                ReadTextRange {
                    line: None,
                    limit: None,
                },
                "none",
                outcome(kind),
                0,
            ),
        })
    }
    pub(super) fn for_resource(
        kind: ReadTextFailureKind,
        resource: &ResourceRef,
        range: ReadTextRange,
    ) -> Self {
        Self {
            kind,
            receipt: receipt(Some(resource), range, "none", outcome(kind), 0),
        }
    }
}
#[allow(dead_code, reason = "P-02 sealed completion handoff")]
#[derive(Debug, Clone)]
pub(crate) struct ReadTextCompletion {
    pub(crate) content: String,
    pub(crate) receipt: NormalizedPayload,
}
#[allow(dead_code, reason = "P-02 Decision B adapter lease")]
pub(crate) struct ReadTextCompletionLease {
    state: Arc<ReadState>,
    token: String,
    deadline: Instant,
    completion: Option<ReadTextCompletion>,
}

#[allow(dead_code, reason = "P-02 Decision B typed egress receipt")]
#[derive(Debug)]
pub(crate) enum ReadTextDelivery {
    Complete(TerminalReceipt),
    ZeroByteFailure(TerminalReceipt),
    EffectUnknown(TerminalReceipt),
}
#[allow(dead_code, reason = "P-02 Decision B typed egress receipt")]
impl ReadTextDelivery {
    pub(crate) fn receipt(&self) -> &TerminalReceipt {
        match self {
            Self::Complete(value) | Self::ZeroByteFailure(value) | Self::EffectUnknown(value) => {
                value
            }
        }
    }
}

mod delivery;
mod state_machine;
pub(super) use state_machine::{
    acknowledge_start, begin_delivery, cancel_exact, claim_receiver, classify_attempt,
    drain_receipts, expire_all, expire_at, finish_read, insert_attempt, lease_dropped,
    receiver_dropped, terminalize_all,
};
mod receipt;
pub(super) use receipt::{
    delivery_unknown, outcome, owner_session_key, receipt, settlement_handle,
};

#[cfg(test)]
mod repair_tests;
