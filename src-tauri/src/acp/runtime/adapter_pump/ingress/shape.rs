use crate::acp::transport::Frame;

#[derive(Clone, Copy)]
pub(super) enum IngestMode {
    Immediate,
    Deferred,
}

pub(super) fn opens_json_container(frame: &Frame) -> bool {
    frame
        .as_bytes()
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| matches!(byte, b'{' | b'['))
}
