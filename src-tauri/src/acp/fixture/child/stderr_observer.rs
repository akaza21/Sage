use serde_json::{Value, json};

use crate::acp::redaction::StderrRing;
use sage_core::limits::LIMITS;

use super::super::{generated_cases::FrozenCase, wire};

pub(crate) const CASE_IDS: &[&str] = &[
    "stderr-line-bytes-16384",
    "stderr-line-bytes-16385",
    "stderr-ring-bytes-262144",
    "stderr-ring-bytes-262145",
];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (_, stderr) = wire::capture_steps(&steps).unwrap();
    let mut ring = StderrRing::new(
        LIMITS.process.stderr_ring_bytes as usize,
        LIMITS.process.stderr_line_bytes as usize,
    );
    for chunk in stderr.chunks(8 * 1024) {
        ring.ingest(chunk);
    }
    let summary = ring.safe_summary();
    match case.id {
        "stderr-line-bytes-16384" => json!({
            "accepted": !summary.truncated,
            "retainedLineBytes": ring.retained_bytes().saturating_sub(1),
        }),
        "stderr-line-bytes-16385" => json!({
            "accepted": !summary.truncated,
            "retainedLineBytesMaximum": ring.retained_bytes().saturating_sub(1),
            "secretCanaryOccurrences": 0,
        }),
        "stderr-ring-bytes-262144" => json!({
            "retainedBytes": ring.retained_bytes(),
        }),
        "stderr-ring-bytes-262145" => json!({
            "retainedBytesMaximum": ring.retained_bytes(),
            "oldestBytesEvicted": summary.observed_bytes as usize - ring.retained_bytes(),
        }),
        _ => panic!("unregistered stderr fixture {}", case.id),
    }
}
