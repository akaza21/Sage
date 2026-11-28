//! The numeric limit ledger is the authoritative contract; the Rust bounds
//! must carry exactly its values.

use std::{fs, path::PathBuf};

use serde_json::Value;

use super::LIMITS;

fn contract(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/sage-core has a checkout two levels up")
        .join("src-tauri/testdata/acp/contracts")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("contract artifact {}: {error}", path.display()))
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
