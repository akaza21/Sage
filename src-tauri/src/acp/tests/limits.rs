//! The numeric limit ledger is the authoritative contract; the Rust bounds
//! must carry exactly its values. Limits with an exact/plus-one fixture pair
//! are also executed through their production bound in
//! `acp::fixture::ledger_limit_tests`; this check covers every section,
//! including the deadlines, durable, migration and fixture limits that have
//! no pair.

use std::{fs, path::PathBuf};

use serde_json::Value;

use crate::acp::limits::LIMITS;

fn contract(name: &str) -> String {
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a checkout parent")
        .to_path_buf();
    let relative =
        PathBuf::from("docs/work-items/ADE-0006A-acp-session-foundation/contracts").join(name);
    // The checkout's own contract is authoritative; a sibling `Sage` tree is
    // only a fallback for packaged sources that omit docs.
    let path = std::iter::once(checkout.join(&relative))
        .chain(
            checkout
                .ancestors()
                .take(4)
                .map(|ancestor| ancestor.join("Sage").join(&relative)),
        )
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("authoritative contract artifact {name} was not found"));
    fs::read_to_string(path).expect("contract artifact must be readable")
}

#[test]
fn rust_limits_equal_the_authoritative_numeric_ledger() {
    let ledger: Value = serde_json::from_str(&contract("limits-v1.json")).unwrap();
    let rust = serde_json::to_value(LIMITS).unwrap();
    for section in [
        "wire",
        "queues",
        "process",
        "deadlinesMs",
        "durable",
        "nativeThreadProjection",
        "migration",
        "fixture",
    ] {
        assert_eq!(
            rust.get(section),
            ledger.get(section),
            "numeric limit section {section} differs from the ledger"
        );
    }
}
