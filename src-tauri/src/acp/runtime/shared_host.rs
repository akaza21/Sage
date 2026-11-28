//! One harness process shared by several chat leases. The router keeps each
//! lease's adapter single-thread and unchanged: it gives each lease only the
//! frames of the threads it owns, maps each lease's JSON-RPC ids into one
//! host-wide id space and back, and fans process-wide notifications out to
//! every lease. What is harness-specific (where a frame names its thread)
//! comes from a `ScopePolicy` in the harness binding.
//!
//! Every server request the process sends gets exactly one answer: from the
//! lease that owns its thread, or a refusal in `Routed::replies` when no lease
//! can take it (unowned, over a bound, or its lease released).

use serde_json::{Value, json};

mod unowned;
use unowned::Unowned;

/// Chats one process serves, so a crash reaches at most this many.
pub(crate) const MAX_LEASES: usize = 4;
/// Root thread plus the child threads a chat's subagents open.
const MAX_THREADS_PER_LEASE: usize = 16;
/// Requests one lease may have waiting on the process at once.
const MAX_PENDING_PER_LEASE: usize = 64;
/// Server requests (approvals, input) one lease may have unanswered.
const MAX_SERVER_REQUESTS_PER_LEASE: usize = 64;
/// Requests the host itself may have waiting on the process.
const MAX_HOST_PENDING: usize = 16;
/// JSON-RPC error a server request gets when no lease can answer it.
const NO_OWNER_CODE: i64 = -32001;

/// Where a harness's frames name their thread.
pub(crate) trait ScopePolicy {
    /// The thread a notification or server request belongs to; `None` when it
    /// names none.
    fn thread_of<'a>(&self, frame: &'a Value) -> Option<&'a str>;
    /// Whether a successful response to `method` opens a thread for the lease
    /// that asked.
    fn opens_thread(&self, method: &str) -> bool;
    /// The thread such a response opened.
    fn thread_in_result<'a>(&self, result: &'a Value) -> Option<&'a str>;
    /// A child thread that a frame of an owned thread announces.
    fn child_announced<'a>(&self, frame: &'a Value) -> Option<&'a str>;
    /// The thread a notification reports as unloaded.
    fn thread_closed<'a>(&self, frame: &'a Value) -> Option<&'a str>;
    /// The handshake request the host made once for every lease.
    fn is_handshake_request(&self, method: &str) -> bool;
    /// The handshake notification the host already sent.
    fn is_handshake_notification(&self, method: &str) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LeaseSlot(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RouterError {
    #[error("the shared host has no free lease")]
    Full,
    #[error("the lease is not admitted")]
    UnknownLease,
    #[error("the frame is not a JSON-RPC message")]
    Malformed,
    #[error("the lease owns too many threads")]
    ThreadsFull,
    #[error("another lease owns the thread")]
    ThreadOwned,
    #[error("too many requests are waiting")]
    PendingFull,
    #[error("the lease answered a request it was never given")]
    ForeignAnswer,
    #[error("the host handshake has not completed")]
    HandshakePending,
}

/// What a lease's outbound frame becomes.
#[derive(Debug, PartialEq)]
pub(crate) enum Outbound {
    /// Write this frame to the process.
    Write(Value),
    /// Give this reply straight back to the lease; nothing is written.
    Answer(Value),
    /// The host already did this; nothing is written.
    Drop,
}

/// What one inbound frame becomes.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Routed {
    pub(crate) deliveries: Vec<(LeaseSlot, Value)>,
    /// Refusals the host writes back to the process itself.
    pub(crate) replies: Vec<Value>,
    /// The answer to a request the host itself sent.
    pub(crate) host: Option<Value>,
    /// A lease whose thread bookkeeping failed; the frame was still delivered.
    pub(crate) fault: Option<(LeaseSlot, RouterError)>,
}

/// A request waiting on the process: its host-wide id, the lease's own id and
/// its method.
struct Pending {
    host_id: u64,
    own_id: Value,
    method: String,
}

#[derive(Default)]
struct Lease {
    threads: Vec<String>,
    requests: Vec<Pending>,
    server_requests: Vec<Value>,
}

pub(crate) struct HostRouter<P> {
    policy: P,
    leases: [Option<Lease>; MAX_LEASES],
    next_host_id: u64,
    host_pending: Vec<u64>,
    handshake: Option<Value>,
    unowned: Unowned,
}

fn refusal(id: Value) -> Value {
    json!({"id": id, "error": {"code": NO_OWNER_CODE, "message": "no chat can answer this request"}})
}

impl<P: ScopePolicy> HostRouter<P> {
    pub(crate) fn new(policy: P) -> Self {
        Self {
            policy,
            leases: Default::default(),
            next_host_id: 1,
            host_pending: Vec::new(),
            handshake: None,
            unowned: Unowned::default(),
        }
    }

    /// Records the result of the one handshake the host made with the
    /// process. Each lease's own handshake request is answered with it and its
    /// parameters are not sent, so the host's handshake must ask for whatever
    /// every lease's adapter relies on.
    pub(crate) fn set_handshake(&mut self, result: Value) {
        self.handshake = Some(result);
    }

    /// A host-wide id for a request the host itself sends; its answer comes
    /// back in `Routed::host`.
    pub(crate) fn host_request_id(&mut self) -> Result<u64, RouterError> {
        if self.host_pending.len() >= MAX_HOST_PENDING {
            return Err(RouterError::PendingFull);
        }
        let id = self.take_host_id();
        self.host_pending.push(id);
        Ok(id)
    }

    pub(crate) fn admit(&mut self) -> Result<LeaseSlot, RouterError> {
        let index = self
            .leases
            .iter()
            .position(Option::is_none)
            .ok_or(RouterError::Full)?;
        self.leases[index] = Some(Lease::default());
        Ok(LeaseSlot(index as u8))
    }

    pub(crate) fn leases(&self) -> usize {
        self.leases.iter().flatten().count()
    }

    /// Frees the slot. Late replies to its requests and frames of its threads
    /// are dropped from now on. Returns the threads it owned and the refusals
    /// the host must write for the server requests it left unanswered.
    pub(crate) fn release(
        &mut self,
        slot: LeaseSlot,
    ) -> Result<(Vec<String>, Vec<Value>), RouterError> {
        let lease = self
            .leases
            .get_mut(usize::from(slot.0))
            .and_then(Option::take)
            .ok_or(RouterError::UnknownLease)?;
        for thread in &lease.threads {
            self.unowned.retire(thread);
        }
        let refusals = lease.server_requests.into_iter().map(refusal).collect();
        Ok((lease.threads, refusals))
    }

    pub(crate) fn owner(&self, thread: &str) -> Option<LeaseSlot> {
        self.leases.iter().enumerate().find_map(|(index, lease)| {
            lease
                .as_ref()
                .filter(|lease| lease.threads.iter().any(|owned| owned == thread))
                .map(|_| LeaseSlot(index as u8))
        })
    }

    /// Gives `thread` to `slot` and returns what arrived for it before. Used
    /// when a thread opens and when a lease resumes a still-loaded thread.
    pub(crate) fn bind_thread(
        &mut self,
        slot: LeaseSlot,
        thread: &str,
    ) -> Result<Routed, RouterError> {
        if self.owner(thread).is_some_and(|owner| owner != slot) {
            return Err(RouterError::ThreadOwned);
        }
        let lease = self.lease_mut(slot)?;
        if !lease.threads.iter().any(|owned| owned == thread) {
            if lease.threads.len() >= MAX_THREADS_PER_LEASE {
                return Err(RouterError::ThreadsFull);
            }
            lease.threads.push(thread.to_owned());
        }
        let mut routed = Routed::default();
        for frame in self.unowned.claim(thread) {
            self.deliver(&mut routed, slot, frame);
        }
        Ok(routed)
    }

    pub(crate) fn outbound(
        &mut self,
        slot: LeaseSlot,
        mut frame: Value,
    ) -> Result<Outbound, RouterError> {
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let id = frame.get("id").cloned();
        self.lease_mut(slot)?;
        match (method, id) {
            (Some(method), Some(id)) if self.policy.is_handshake_request(&method) => {
                let result = self
                    .handshake
                    .clone()
                    .ok_or(RouterError::HandshakePending)?;
                Ok(Outbound::Answer(json!({"id": id, "result": result})))
            }
            (Some(method), Some(own_id)) => {
                if self.lease_mut(slot)?.requests.len() >= MAX_PENDING_PER_LEASE {
                    return Err(RouterError::PendingFull);
                }
                let host_id = self.take_host_id();
                let pending = Pending {
                    host_id,
                    own_id,
                    method,
                };
                self.lease_mut(slot)?.requests.push(pending);
                frame["id"] = json!(host_id);
                Ok(Outbound::Write(frame))
            }
            (None, Some(id)) => {
                let lease = self.lease_mut(slot)?;
                let index = lease
                    .server_requests
                    .iter()
                    .position(|owned| *owned == id)
                    .ok_or(RouterError::ForeignAnswer)?;
                lease.server_requests.swap_remove(index);
                Ok(Outbound::Write(frame))
            }
            (Some(method), None) if self.policy.is_handshake_notification(&method) => {
                Ok(Outbound::Drop)
            }
            (Some(_), None) => Ok(Outbound::Write(frame)),
            (None, None) => Err(RouterError::Malformed),
        }
    }

    pub(crate) fn inbound(&mut self, frame: Value) -> Result<Routed, RouterError> {
        let has_method = frame.get("method").is_some_and(Value::is_string);
        let mut routed = Routed::default();
        match (has_method, frame.get("id").cloned()) {
            (false, Some(id)) => self.response(&mut routed, frame, &id),
            (true, _) => self.dispatch(&mut routed, frame),
            (false, None) => return Err(RouterError::Malformed),
        }
        Ok(routed)
    }

    fn response(&mut self, routed: &mut Routed, mut frame: Value, id: &Value) {
        let Some(host_id) = id.as_u64() else {
            return;
        };
        if let Some(at) = self.host_pending.iter().position(|owned| *owned == host_id) {
            self.host_pending.swap_remove(at);
            routed.host = Some(frame);
            return;
        }
        let found = self
            .leases
            .iter_mut()
            .enumerate()
            .find_map(|(index, lease)| {
                let lease = lease.as_mut()?;
                let at = lease
                    .requests
                    .iter()
                    .position(|pending| pending.host_id == host_id)?;
                Some((LeaseSlot(index as u8), lease.requests.swap_remove(at)))
            });
        let Some((slot, pending)) = found else {
            return;
        };
        let opened = frame
            .get("result")
            .filter(|_| self.policy.opens_thread(&pending.method))
            .and_then(|result| self.policy.thread_in_result(result))
            .map(str::to_owned);
        frame["id"] = pending.own_id;
        routed.deliveries.push((slot, frame));
        if let Some(thread) = opened {
            self.bind_into(routed, slot, &thread);
        }
    }

    /// A notification or server request.
    fn dispatch(&mut self, routed: &mut Routed, frame: Value) {
        let Some(thread) = self.policy.thread_of(&frame).map(str::to_owned) else {
            self.unscoped(routed, frame);
            return;
        };
        let Some(slot) = self.owner(&thread) else {
            let closed = self.policy.thread_closed(&frame) == Some(thread.as_str());
            routed
                .replies
                .extend(self.unowned.hold(&thread, frame, closed));
            return;
        };
        let child = self.policy.child_announced(&frame).map(str::to_owned);
        let closed = self.policy.thread_closed(&frame) == Some(thread.as_str());
        self.deliver(routed, slot, frame);
        if let Some(child) = child {
            self.bind_into(routed, slot, &child);
        }
        if closed && let Some(lease) = self.leases[usize::from(slot.0)].as_mut() {
            lease.threads.retain(|owned| *owned != thread);
        }
    }

    fn unscoped(&mut self, routed: &mut Routed, frame: Value) {
        let Some(id) = frame.get("id").cloned() else {
            for slot in self.slots() {
                routed.deliveries.push((slot, frame.clone()));
            }
            return;
        };
        // A server request that names no thread could belong to any chat;
        // the per-chat adapters refuse these too.
        routed.replies.push(refusal(id));
    }

    /// Drops frames held for unbound threads since before the previous sweep.
    /// The host calls this on a short timer; returns the refusals to write.
    pub(crate) fn sweep_unowned(&mut self) -> Vec<Value> {
        self.unowned.sweep()
    }

    /// Hands `frame` to `slot`, recording a server request it must answer, or
    /// refuses it when the lease has too many open.
    fn deliver(&mut self, routed: &mut Routed, slot: LeaseSlot, frame: Value) {
        let Some(lease) = self.leases[usize::from(slot.0)].as_mut() else {
            return;
        };
        if let Some(id) = frame.get("id").filter(|_| frame.get("method").is_some()) {
            if lease.server_requests.len() >= MAX_SERVER_REQUESTS_PER_LEASE {
                routed.replies.push(refusal(id.clone()));
                return;
            }
            lease.server_requests.push(id.clone());
        }
        routed.deliveries.push((slot, frame));
    }

    fn bind_into(&mut self, routed: &mut Routed, slot: LeaseSlot, thread: &str) {
        match self.bind_thread(slot, thread) {
            Ok(held) => {
                routed.deliveries.extend(held.deliveries);
                routed.replies.extend(held.replies);
            }
            Err(error) => routed.fault = Some((slot, error)),
        }
    }

    fn take_host_id(&mut self) -> u64 {
        let id = self.next_host_id;
        self.next_host_id += 1;
        id
    }

    fn slots(&self) -> Vec<LeaseSlot> {
        self.leases
            .iter()
            .enumerate()
            .filter(|(_, lease)| lease.is_some())
            .map(|(index, _)| LeaseSlot(index as u8))
            .collect()
    }

    fn lease_mut(&mut self, slot: LeaseSlot) -> Result<&mut Lease, RouterError> {
        self.leases
            .get_mut(usize::from(slot.0))
            .and_then(Option::as_mut)
            .ok_or(RouterError::UnknownLease)
    }
}
