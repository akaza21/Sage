//! Frames for threads no lease owns. A thread's first frames can arrive
//! before the response that opens it, or before its parent announces it, so
//! they are held briefly and handed over when a lease binds the thread.
//! Frames of a thread whose lease was released are not held: the process may
//! keep such a thread loaded for a while and anything it sends is nobody's.

use std::collections::VecDeque;

use serde_json::Value;

use super::refusal;

/// Frames held across all unbound threads.
const MAX_HELD: usize = 32;
/// Serialized bytes held across all unbound threads.
const MAX_HELD_BYTES: usize = 256 * 1024;
/// Released threads remembered so their late frames are not held.
const MAX_RETIRED: usize = 64;

struct Held {
    thread: String,
    frame: Value,
    bytes: usize,
    generation: u64,
}

#[derive(Default)]
pub(super) struct Unowned {
    held: VecDeque<Held>,
    held_bytes: usize,
    retired: VecDeque<String>,
    generation: u64,
}

fn frame_bytes(frame: &Value) -> usize {
    serde_json::to_vec(frame).map_or(usize::MAX, |bytes| bytes.len())
}

fn server_request_id(frame: &Value) -> Option<Value> {
    frame.get("method")?;
    frame.get("id").cloned()
}

fn refusals(dropped: impl IntoIterator<Item = Held>) -> Vec<Value> {
    dropped
        .into_iter()
        .filter_map(|held| server_request_id(&held.frame).map(refusal))
        .collect()
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
        if closed {
            self.retired.retain(|retired| retired != thread);
            return refusals(self.take(|held| held.thread == thread));
        }
        let bytes = frame_bytes(&frame);
        if bytes > MAX_HELD_BYTES || self.retired.iter().any(|retired| retired == thread) {
            return server_request_id(&frame).map(refusal).into_iter().collect();
        }
        self.held_bytes += bytes;
        self.held.push_back(Held {
            thread: thread.to_owned(),
            frame,
            bytes,
            generation: self.generation,
        });
        let mut evicted = Vec::new();
        while self.held.len() > MAX_HELD || self.held_bytes > MAX_HELD_BYTES {
            let Some(oldest) = self.held.pop_front() else {
                break;
            };
            self.held_bytes -= oldest.bytes;
            evicted.push(oldest);
        }
        refusals(evicted)
    }

    /// Drops what was held since before the previous sweep, so nothing waits
    /// longer than two sweep periods. Returns the refusals owed.
    pub(super) fn sweep(&mut self) -> Vec<Value> {
        let generation = self.generation;
        self.generation += 1;
        refusals(self.take(|held| held.generation < generation))
    }

    /// What was held for `thread`, in arrival order, now that a lease owns it.
    pub(super) fn claim(&mut self, thread: &str) -> Vec<Value> {
        self.retired.retain(|retired| retired != thread);
        self.take(|held| held.thread == thread)
            .into_iter()
            .map(|held| held.frame)
            .collect()
    }

    fn take(&mut self, mut picked: impl FnMut(&Held) -> bool) -> VecDeque<Held> {
        let (taken, kept): (VecDeque<Held>, VecDeque<Held>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|held| picked(held));
        self.held = kept;
        self.held_bytes = self.held.iter().map(|held| held.bytes).sum();
        taken
    }
}
