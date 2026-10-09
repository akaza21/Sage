use super::super::{
    AdapterBatchJournal, AdapterPump, HarnessAdapter, ResourceMapper, STDOUT_PREAMBLE_MAX_BYTES,
    STDOUT_PREAMBLE_MAX_LINES,
};
use crate::acp::redaction::sanitize_diagnostic_text;

use crate::acp::transport::Frame;

#[derive(Clone, Copy)]
pub(super) enum IngestMode {
    Immediate,
    Deferred,
}

fn opens_json_container(frame: &Frame) -> bool {
    frame
        .as_bytes()
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| matches!(byte, b'{' | b'['))
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(super) fn skip_stdout_preamble(&mut self, frame: &Frame) -> bool {
        let Some(mut window) = self.stdout_preamble else {
            return false;
        };
        if opens_json_container(frame) {
            self.stdout_preamble = None;
            return false;
        }
        window.lines += 1;
        window.bytes = window.bytes.saturating_add(frame.len());
        if window.lines > STDOUT_PREAMBLE_MAX_LINES || window.bytes > STDOUT_PREAMBLE_MAX_BYTES {
            self.stdout_preamble = None;
            return false;
        }
        self.stdout_preamble = Some(window);
        tracing::warn!(
            activation_checkpoint = "adapter_pump.stdout.preamble_skipped",
            preamble_lines = window.lines,
            preamble_bytes = window.bytes,
            preamble_max_lines = STDOUT_PREAMBLE_MAX_LINES,
            preamble_max_bytes = STDOUT_PREAMBLE_MAX_BYTES,
            preamble_line = %sanitize_diagnostic_text(&String::from_utf8_lossy(frame.as_bytes())),
            "ACP adapter pump skipped non-protocol stdout preamble before the first frame"
        );
        true
    }
}
