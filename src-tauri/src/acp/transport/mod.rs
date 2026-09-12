mod egress;
mod framer;
mod ingress;
mod p02_egress;
mod request_digest;
mod writer;

#[allow(unused_imports, reason = "fixture and transport-test entrypoints")]
pub(crate) use egress::{BoundedQueue, EgressQueue, QueueError, ReservedQueue};
#[allow(unused_imports, reason = "transport-test reader entrypoint")]
pub(crate) use framer::{Frame, FrameError, FrameReader, IncrementalFrameDecoder};
pub(crate) use ingress::{StructuralError, validate_structure};
#[allow(unused_imports, reason = "P-02 Decision B adapter egress entrypoint")]
pub(crate) use p02_egress::{
    DeadlineAsyncWriter, P02Cancellation, P02Egress, P02EgressOutcome, write_p02_response_until,
};
pub(crate) use request_digest::request_payload_digest;
#[allow(
    unused_imports,
    reason = "ordinary transport writer remains an admitted adapter seam"
)]
pub(crate) use writer::{
    FrameWriteError, FrameWriteOutcome, FrameWriter, OutboundFrame, OutboundFrameClass,
};

#[cfg(test)]
mod tests;
