//! Frames for threads no lease owns. A thread's first frames can arrive
//! before the response that opens it, or before its parent announces it, so
//! they are held briefly and handed over when a lease binds the thread.
//! Frames of a thread whose lease was released are not held: Codex keeps such
//! a thread loaded for about a minute and anything it sends is nobody's.

use std::collections::VecDeque;

use serde_json::Value;

use super::refusal;

/// Frames held across all unbound threads.
const MAX_HELD: usize = 32;
/// Released threads remembered so their late frames are not held.
const MAX_RETIRED: usize = 64;

#[derive(Default)]
pub(super) struct Unowned {
    held: VecDeque<(String, Value, u64)>,
    retired: VecDeque<String>,
    generation: u64,
}

fn server_request_id(frame: &Value) -> Option<Value> {
    frame.get("method")?;
    frame.get("id").cloned()
}

impl Unowned {
    pub(super) fn retire(&mut self, thread: &str) {
        if self.retired.iter().any(|retired| retired == thread) {
            return;
        }
        if self.retired.len() >= MAX_RETIRED {
            self.retired.pop_front();
        }
        self.retired.push_back(thread.to_owned());
    }

    /// Holds `frame` for `thread`, or drops it. Returns the refusals owed for
    /// any server request dropped on the way.
    pub(super) fn hold(&mut self, thread: &str, frame: Value, closed: bool) -> Vec<Value> {
        let mut refusals = Vec::new();
        if closed {
            self.retired.retain(|retired| retired != thread);
            for (_, frame, _) in self.take(thread) {
                refusals.extend(server_request_id(&frame).map(refusal));
            }
            return refusals;
        }
        if self.retired.iter().any(|retired| retired == thread) {
            refusals.extend(server_request_id(&frame).map(refusal));
            return refusals;
        }
        self.held
            .push_back((thread.to_owned(), frame, self.generation));
        if self.held.len() > MAX_HELD
            && let Some((_, oldest, _)) = self.held.pop_front()
        {
            refusals.extend(server_request_id(&oldest).map(refusal));
        }
        refusals
    }

    /// Drops what was held since before the previous sweep, so nothing waits
    /// longer than two sweep periods. Returns the refusals owed.
    pub(super) fn sweep(&mut self) -> Vec<Value> {
        let generation = self.generation;
        let (stale, kept) = std::mem::take(&mut self.held)
            .into_iter()
            .partition::<VecDeque<_>, _>(|(_, _, held)| *held < generation);
        self.held = kept;
        self.generation += 1;
        stale
            .into_iter()
            .filter_map(|(_, frame, _)| server_request_id(&frame).map(refusal))
            .collect()
    }

    /// What was held for `thread`, in arrival order, now that a lease owns it.
    pub(super) fn claim(&mut self, thread: &str) -> Vec<Value> {
        self.retired.retain(|retired| retired != thread);
        self.take(thread)
            .into_iter()
            .map(|(_, frame, _)| frame)
            .collect()
    }

    fn take(&mut self, thread: &str) -> Vec<(String, Value, u64)> {
        let (taken, kept) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|(owner, _, _)| owner == thread);
        self.held = kept;
        taken
    }
}
