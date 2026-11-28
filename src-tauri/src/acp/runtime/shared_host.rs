//! One harness process shared by several chat leases. The router keeps each
//! lease's adapter single-thread and unchanged: it gives each lease only the
//! frames of the threads it owns, maps each lease's JSON-RPC ids into one
//! host-wide id space and back, and fans process-wide notifications out to
//! every lease. What is harness-specific (where a frame names its thread)
//! comes from a `ScopePolicy` in the harness binding.

use serde_json::{Value, json};

/// Chats one process serves, so a crash reaches at most this many.
pub(crate) const MAX_LEASES: usize = 4;
/// Root thread plus the child threads a chat's subagents open.
const MAX_THREADS_PER_LEASE: usize = 16;
/// Requests one lease may have waiting on the process at once.
const MAX_PENDING_PER_LEASE: usize = 64;
/// Server requests (approvals, input) one lease may have unanswered.
const MAX_SERVER_REQUESTS_PER_LEASE: usize = 64;
/// JSON-RPC error a server request gets when no lease owns its thread.
const NO_OWNER_CODE: i64 = -32001;

/// Where a harness's frames name their thread.
pub(crate) trait ScopePolicy {
    /// The thread a notification or server request belongs to; `None` when it
    /// is process-wide.
    fn thread_of<'a>(&self, frame: &'a Value) -> Option<&'a str>;
    /// The thread a successful response opens for the lease that asked.
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
    #[error("the lease has too many requests waiting")]
    PendingFull,
    #[error("the lease answered a request it was never given")]
    ForeignAnswer,
    #[error("another lease owns the thread")]
    ThreadOwned,
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
    /// A reply the host writes back itself.
    pub(crate) reply: Option<Value>,
}

#[derive(Default)]
struct Lease {
    threads: Vec<String>,
    /// Host-wide id and the lease's own id of each request waiting.
    requests: Vec<(u64, Value)>,
    server_requests: Vec<Value>,
}

pub(crate) struct HostRouter<P> {
    policy: P,
    leases: [Option<Lease>; MAX_LEASES],
    next_host_id: u64,
    handshake: Option<Value>,
}

impl<P: ScopePolicy> HostRouter<P> {
    pub(crate) fn new(policy: P) -> Self {
        Self {
            policy,
            leases: Default::default(),
            next_host_id: 1,
            handshake: None,
        }
    }

    /// Records the result of the handshake the host made with the process;
    /// each lease's own handshake request is answered with it.
    pub(crate) fn set_handshake(&mut self, result: Value) {
        self.handshake = Some(result);
    }

    /// The next host-wide id, for a request the host itself sends.
    pub(crate) fn host_request_id(&mut self) -> u64 {
        let id = self.next_host_id;
        self.next_host_id += 1;
        id
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
    /// are dropped from now on. Returns the threads it owned.
    pub(crate) fn release(&mut self, slot: LeaseSlot) -> Result<Vec<String>, RouterError> {
        let lease = self
            .leases
            .get_mut(usize::from(slot.0))
            .and_then(Option::take)
            .ok_or(RouterError::UnknownLease)?;
        Ok(lease.threads)
    }

    pub(crate) fn owner(&self, thread: &str) -> Option<LeaseSlot> {
        self.leases.iter().enumerate().find_map(|(index, lease)| {
            lease
                .as_ref()
                .filter(|lease| lease.threads.iter().any(|owned| owned == thread))
                .map(|_| LeaseSlot(index as u8))
        })
    }

    /// Gives `thread` to `slot`, as when the lease resumes a thread that is
    /// still loaded in the process.
    pub(crate) fn bind_thread(&mut self, slot: LeaseSlot, thread: &str) -> Result<(), RouterError> {
        if self.owner(thread).is_some_and(|owner| owner != slot) {
            return Err(RouterError::ThreadOwned);
        }
        let lease = self.lease_mut(slot)?;
        if lease.threads.iter().any(|owned| owned == thread) {
            return Ok(());
        }
        if lease.threads.len() >= MAX_THREADS_PER_LEASE {
            return Err(RouterError::ThreadsFull);
        }
        lease.threads.push(thread.to_owned());
        Ok(())
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
        match (method, id) {
            (Some(method), Some(id)) if self.policy.is_handshake_request(&method) => {
                self.lease_mut(slot)?;
                let result = self
                    .handshake
                    .clone()
                    .ok_or(RouterError::HandshakePending)?;
                Ok(Outbound::Answer(json!({"id": id, "result": result})))
            }
            (Some(_), Some(id)) => {
                let host_id = self.next_host_id;
                let lease = self.lease_mut(slot)?;
                if lease.requests.len() >= MAX_PENDING_PER_LEASE {
                    return Err(RouterError::PendingFull);
                }
                lease.requests.push((host_id, id));
                self.next_host_id += 1;
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
                self.lease_mut(slot)?;
                Ok(Outbound::Drop)
            }
            (Some(_), None) => {
                self.lease_mut(slot)?;
                Ok(Outbound::Write(frame))
            }
            (None, None) => Err(RouterError::Malformed),
        }
    }

    pub(crate) fn inbound(&mut self, frame: Value) -> Result<Routed, RouterError> {
        let has_method = frame.get("method").is_some_and(Value::is_string);
        match (has_method, frame.get("id").cloned()) {
            (false, Some(id)) => self.response(frame, &id),
            (true, Some(id)) => self.server_request(frame, id),
            (true, None) => self.notification(frame),
            (false, None) => Err(RouterError::Malformed),
        }
    }

    fn response(&mut self, mut frame: Value, id: &Value) -> Result<Routed, RouterError> {
        let Some(host_id) = id.as_u64() else {
            return Ok(Routed::default());
        };
        let found = self
            .leases
            .iter_mut()
            .enumerate()
            .find_map(|(index, lease)| {
                let lease = lease.as_mut()?;
                let at = lease
                    .requests
                    .iter()
                    .position(|(owned, _)| *owned == host_id)?;
                Some((index, lease.requests.swap_remove(at).1))
            });
        let Some((index, own_id)) = found else {
            return Ok(Routed::default());
        };
        let slot = LeaseSlot(index as u8);
        if let Some(thread) = frame
            .get("result")
            .and_then(|result| self.policy.thread_in_result(result))
            .map(str::to_owned)
        {
            self.bind_thread(slot, &thread)?;
        }
        frame["id"] = own_id;
        Ok(Routed {
            deliveries: vec![(slot, frame)],
            reply: None,
        })
    }

    fn server_request(&mut self, frame: Value, id: Value) -> Result<Routed, RouterError> {
        let slot = match self.policy.thread_of(&frame) {
            Some(thread) => self.owner(thread),
            None => self.first_lease(),
        };
        let Some(slot) = slot else {
            let message = "no chat owns this thread";
            let reply = json!({"id": id, "error": {"code": NO_OWNER_CODE, "message": message}});
            return Ok(Routed {
                deliveries: Vec::new(),
                reply: Some(reply),
            });
        };
        let lease = self.lease_mut(slot)?;
        if lease.server_requests.len() >= MAX_SERVER_REQUESTS_PER_LEASE {
            return Err(RouterError::PendingFull);
        }
        lease.server_requests.push(id);
        Ok(Routed {
            deliveries: vec![(slot, frame)],
            reply: None,
        })
    }

    fn notification(&mut self, frame: Value) -> Result<Routed, RouterError> {
        let Some(thread) = self.policy.thread_of(&frame).map(str::to_owned) else {
            let deliveries = self
                .slots()
                .into_iter()
                .map(|slot| (slot, frame.clone()))
                .collect();
            return Ok(Routed {
                deliveries,
                reply: None,
            });
        };
        let Some(slot) = self.owner(&thread) else {
            return Ok(Routed::default());
        };
        if let Some(child) = self.policy.child_announced(&frame).map(str::to_owned) {
            self.bind_thread(slot, &child)?;
        }
        if self.policy.thread_closed(&frame) == Some(thread.as_str()) {
            let lease = self.lease_mut(slot)?;
            lease.threads.retain(|owned| *owned != thread);
        }
        Ok(Routed {
            deliveries: vec![(slot, frame)],
            reply: None,
        })
    }

    fn slots(&self) -> Vec<LeaseSlot> {
        self.leases
            .iter()
            .enumerate()
            .filter(|(_, lease)| lease.is_some())
            .map(|(index, _)| LeaseSlot(index as u8))
            .collect()
    }

    fn first_lease(&self) -> Option<LeaseSlot> {
        self.slots().into_iter().next()
    }

    fn lease_mut(&mut self, slot: LeaseSlot) -> Result<&mut Lease, RouterError> {
        self.leases
            .get_mut(usize::from(slot.0))
            .and_then(Option::as_mut)
            .ok_or(RouterError::UnknownLease)
    }
}
