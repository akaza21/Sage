use std::collections::VecDeque;

use crate::acp::limits::LIMITS;

use super::OutboundFrame;

/// Hard cap for one reserved control-lane message.
const CONTROL_MESSAGE_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueError {
    CountCapacity,
    ByteCapacity,
    ControlItemTooLarge,
    SequenceCapacity,
}

pub(crate) struct BoundedQueue<T> {
    entries: VecDeque<(T, usize)>,
    count_limit: usize,
    byte_limit: usize,
    retained_bytes: usize,
}

impl<T> BoundedQueue<T> {
    pub(crate) fn new(count_limit: usize, byte_limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            count_limit,
            byte_limit,
            retained_bytes: 0,
        }
    }

    pub(crate) fn try_push_with(
        &mut self,
        bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<(), QueueError> {
        if self.entries.len() >= self.count_limit {
            return Err(QueueError::CountCapacity);
        }
        let next_bytes = self
            .retained_bytes
            .checked_add(bytes)
            .ok_or(QueueError::ByteCapacity)?;
        if next_bytes > self.byte_limit {
            return Err(QueueError::ByteCapacity);
        }
        // Reserve storage before constructing the retained item. `reserve_exact`
        // prevents an eager allocation proportional to the configured limit.
        self.entries
            .try_reserve_exact(1)
            .map_err(|_| QueueError::ByteCapacity)?;
        let item = build();
        self.entries.push_back((item, bytes));
        self.retained_bytes = next_bytes;
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        let (item, bytes) = self.entries.pop_front()?;
        self.retained_bytes -= bytes;
        Some(item)
    }

    fn front(&self) -> Option<&T> {
        self.entries.front().map(|(item, _)| item)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    #[cfg(test)]
    pub(super) fn allocated_capacity(&self) -> usize {
        self.entries.capacity()
    }
}

struct ControlEntry<T> {
    item: T,
    bytes: usize,
    deadline_ms: u64,
    sequence: u64,
}

/// A non-borrowing data/control queue. Control capacity is reserved *inside*
/// total capacity and is always selected before the next data item.
pub(crate) struct ReservedQueue<T> {
    data: BoundedQueue<T>,
    control: VecDeque<ControlEntry<T>>,
    control_count_limit: usize,
    control_byte_limit: usize,
    control_bytes: usize,
    next_sequence: u64,
}

impl<T> ReservedQueue<T> {
    pub(crate) fn new(
        total_count: usize,
        total_bytes: usize,
        control_count: usize,
        control_bytes: usize,
    ) -> Result<Self, QueueError> {
        let data_count = total_count
            .checked_sub(control_count)
            .ok_or(QueueError::CountCapacity)?;
        let data_bytes = total_bytes
            .checked_sub(control_bytes)
            .ok_or(QueueError::ByteCapacity)?;
        Ok(Self {
            data: BoundedQueue::new(data_count, data_bytes),
            control: VecDeque::new(),
            control_count_limit: control_count,
            control_byte_limit: control_bytes,
            control_bytes: 0,
            next_sequence: 0,
        })
    }

    pub(crate) fn try_push_data_with(
        &mut self,
        bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<(), QueueError> {
        self.data.try_push_with(bytes, build)
    }

    pub(crate) fn try_push_control_with(
        &mut self,
        bytes: usize,
        deadline_ms: u64,
        build: impl FnOnce() -> T,
    ) -> Result<(), QueueError> {
        if bytes > CONTROL_MESSAGE_BYTES {
            return Err(QueueError::ControlItemTooLarge);
        }
        if self.control.len() >= self.control_count_limit {
            return Err(QueueError::CountCapacity);
        }
        let next_bytes = self
            .control_bytes
            .checked_add(bytes)
            .ok_or(QueueError::ByteCapacity)?;
        if next_bytes > self.control_byte_limit {
            return Err(QueueError::ByteCapacity);
        }
        let sequence = self.next_sequence;
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(QueueError::SequenceCapacity)?;
        self.control
            .try_reserve_exact(1)
            .map_err(|_| QueueError::ByteCapacity)?;
        let item = build();
        self.control.push_back(ControlEntry {
            item,
            bytes,
            deadline_ms,
            sequence,
        });
        self.next_sequence = next_sequence;
        self.control_bytes = next_bytes;
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        self.pop_control().or_else(|| self.data.pop())
    }

    /// Selects only the highest-priority control item, leaving queued data in
    /// place. Process drivers use this between decoded frames so cancellation
    /// cannot be delayed by the remainder of an already-read data chunk.
    pub(crate) fn pop_control(&mut self) -> Option<T> {
        let index = self
            .control
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| (entry.deadline_ms, entry.sequence))
            .map(|(index, _)| index)?;
        let entry = self.control.remove(index)?;
        self.control_bytes -= entry.bytes;
        Some(entry.item)
    }

    pub(crate) fn pop_control_if(&mut self, mut pred: impl FnMut(&T) -> bool) -> Option<T> {
        let index = self
            .control
            .iter()
            .enumerate()
            .filter(|(_, entry)| pred(&entry.item))
            .min_by_key(|(_, entry)| (entry.deadline_ms, entry.sequence))
            .map(|(index, _)| index)?;
        let entry = self.control.remove(index)?;
        self.control_bytes -= entry.bytes;
        Some(entry.item)
    }

    pub(crate) fn pop_data_if(&mut self, pred: impl FnOnce(&T) -> bool) -> Option<T> {
        if !self.data.front().is_some_and(pred) {
            return None;
        }
        self.data.pop()
    }

    pub(crate) fn data_front(&self) -> Option<&T> {
        self.data.front()
    }

    pub(crate) fn data_usage(&self) -> (usize, usize) {
        (self.data.len(), self.data.retained_bytes())
    }

    pub(crate) fn control_usage(&self) -> (usize, usize) {
        (self.control.len(), self.control_bytes)
    }

    #[cfg(test)]
    pub(super) fn allocated_capacities(&self) -> (usize, usize) {
        (self.data.allocated_capacity(), self.control.capacity())
    }
}

/// The sole outbound queue for adapter payloads. Capacity accounts for the
/// delimiter that `FrameWriter` appends, so retained wire bytes cannot exceed
/// the frozen egress ceiling.
pub(crate) struct EgressQueue {
    frames: ReservedQueue<OutboundFrame>,
}

impl EgressQueue {
    pub(crate) fn new() -> Result<Self, QueueError> {
        Self::with_limits(
            LIMITS.queues.egress_frames as usize,
            LIMITS.queues.egress_bytes as usize,
            LIMITS.queues.egress_control_frames_inside_total as usize,
            LIMITS.queues.egress_control_bytes_inside_total as usize,
        )
    }

    fn with_limits(
        total_count: usize,
        total_bytes: usize,
        control_count: usize,
        control_bytes: usize,
    ) -> Result<Self, QueueError> {
        Ok(Self {
            frames: ReservedQueue::new(total_count, total_bytes, control_count, control_bytes)?,
        })
    }

    pub(crate) fn try_push_data(&mut self, frame: OutboundFrame) -> Result<(), QueueError> {
        let bytes = frame.wire_len();
        self.frames.try_push_data_with(bytes, || frame)
    }

    pub(crate) fn try_push_control(
        &mut self,
        frame: OutboundFrame,
        deadline_ms: u64,
    ) -> Result<(), QueueError> {
        let bytes = frame.wire_len();
        self.frames
            .try_push_control_with(bytes, deadline_ms, || frame)
    }

    pub(crate) fn pop(&mut self) -> Option<OutboundFrame> {
        self.frames.pop()
    }
}

#[cfg(test)]
mod egress_frame_tests {
    use super::*;

    #[test]
    fn wire_delimiter_is_reserved_inside_the_byte_ceiling() {
        let mut queue = EgressQueue::with_limits(2, 4, 0, 0).unwrap();
        queue
            .try_push_data(OutboundFrame::new(b"{}".to_vec()).unwrap())
            .unwrap();
        assert_eq!(
            queue.try_push_data(OutboundFrame::new(b"x".to_vec()).unwrap()),
            Err(QueueError::ByteCapacity)
        );
        assert_eq!(queue.pop().unwrap().wire_len(), 3);
    }
}
