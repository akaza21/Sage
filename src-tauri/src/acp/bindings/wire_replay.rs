//! Replays a real harness transcript recorded by
//! `scripts/harness-wire/capture.mjs` through a binding and projects what the
//! binding made of it. The driver knows no harness: each recorded client
//! frame that starts a Sage command carries a `step`, the matching
//! `AdapterCommand` is issued, recorded request ids are re-pointed at the ids
//! the binding chose, and every harness frame goes through `ingest`.

mod driver;
mod projection;

use std::path::PathBuf;

use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) use driver::Replay;

use super::adapter::ResourceMapper;
use crate::acp::transport::{Frame, FrameReader};

/// Where the committed corpus lives.
pub(crate) fn corpus(harness: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/harness-wire")
        .join(harness)
}

/// The sanitized scratch repository every transcript works in.
pub(crate) const ROOT: &str = "C:\\fixture-workspace\\repo";

/// Maps the scratch repository's files to one Sage root.
pub(crate) struct FixtureMapper;

impl ResourceMapper for FixtureMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<
        sage_acp_protocol::intent::SafeResourceRef,
        sage_acp_protocol::intent::UnavailableResourceReason,
    > {
        let path = absolute_path.replace('/', "\\");
        path.strip_prefix(ROOT)
            .and_then(|rest| rest.strip_prefix('\\'))
            .and_then(|relative| {
                sage_acp_protocol::intent::SafeResourceRef::new(
                    sage_acp_protocol::contract::SageRootId::parse("root-replay").ok()?,
                    relative.replace('\\', "/"),
                )
                .ok()
            })
            .ok_or(sage_acp_protocol::intent::UnavailableResourceReason::Foreign)
    }
}

/// The scenarios a harness's corpus holds, so a new capture is replayed
/// without a test naming it.
pub(crate) fn scenarios(harness: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(corpus(harness))
        .expect("corpus")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".jsonl").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

pub(crate) struct Record {
    pub(crate) out: bool,
    step: Option<Value>,
    pub(crate) frame: Value,
}

fn lines(harness: &str, file: &str) -> Vec<String> {
    let path = corpus(harness).join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// The frame of the line in `file` whose SHA-256 starts with `digest`, as a
/// `ref` record names a handshake frame stored once.
fn referenced(harness: &str, file: &str, digest: &str) -> Value {
    let line = lines(harness, file)
        .into_iter()
        .find(|line| {
            let hash = Sha256::digest(line.as_bytes());
            hash.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .starts_with(digest)
        })
        .unwrap_or_else(|| panic!("{harness}: no line of {file} has digest {digest}"));
    let record: Value = serde_json::from_str(&line).expect("referenced line");
    record["frame"].clone()
}

pub(crate) fn load(harness: &str, scenario: &str) -> Vec<Record> {
    lines(harness, &format!("{scenario}.jsonl"))
        .into_iter()
        .map(|line| {
            let record: Value = serde_json::from_str(&line).expect("transcript line");
            let frame = match (record["ref"].as_str(), record["sha256"].as_str()) {
                (Some(file), Some(digest)) => referenced(harness, file, digest),
                _ => record["frame"].clone(),
            };
            Record {
                out: record["direction"] == "out",
                step: record.get("step").cloned(),
                frame,
            }
        })
        .collect()
}

pub(crate) fn frame(value: &Value) -> Frame {
    let mut bytes = serde_json::to_vec(value).expect("frame bytes");
    bytes.push(b'\n');
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(FrameReader::new(bytes.as_slice()).read_frame())
        .expect("frame reads")
        .expect("one frame")
}

/// Compares a replay's summary with the committed `<scenario>.replay.json`.
/// `SAGE_WIRE_BLESS=1` rewrites it instead, after a reviewed recapture. A
/// frame the binding failed to take fails the replay outright, except in a
/// scenario that records the harness refusing to work at all.
pub(crate) fn assert_summary(harness: &str, scenario: &str, summary: &Value) {
    let errors = summary["ingestErrors"].as_array().map_or(0, Vec::len);
    assert!(
        errors == 0 || scenario == "auth-fail",
        "{harness}/{scenario}: the binding failed to take {errors} recorded frames: {}",
        summary["ingestErrors"]
    );
    let path = corpus(harness).join(format!("{scenario}.replay.json"));
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(summary).expect("summary")
    );
    if std::env::var_os("SAGE_WIRE_BLESS").is_some() {
        std::fs::write(&path, rendered).expect("bless");
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path:?}: {error}; bless a reviewed capture"));
    let committed: Value = serde_json::from_str(&committed).expect("committed summary");
    assert_eq!(
        &committed, summary,
        "{harness}/{scenario}: the binding no longer projects the recorded transcript the same way"
    );
}
