// P1 freezes the private transport before P2 owns its single consumer.
#![allow(dead_code, unused_imports)]

mod egress;
mod framer;
mod ingress;
mod request_digest;
mod writer;

pub(crate) use egress::{BoundedQueue, EgressQueue, QueueError, ReservedQueue};
pub(crate) use framer::{Frame, FrameError, FrameReader, IncrementalFrameDecoder};
pub(crate) use ingress::{StructuralError, validate_structure};
pub(crate) use request_digest::request_payload_digest;
pub(crate) use writer::{
    FrameWriteError, FrameWriteOutcome, FrameWriter, OutboundFrame, write_json_line_blocking,
};

#[cfg(test)]
mod tests;
