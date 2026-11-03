use std::{fs, path::PathBuf};

use crate::acp::fixture_protocol::{validate_limit_fixture_coverage, validate_limits_document};

fn contract_path(name: &str) -> PathBuf {
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a checkout parent")
        .to_path_buf();
    let relative =
        PathBuf::from("docs/work-items/ADE-0006A-acp-session-foundation/contracts").join(name);
    // The checkout's own contract is authoritative; a sibling `Sage` tree is
    // only a fallback for packaged sources that omit docs.
    std::iter::once(checkout.join(&relative))
        .chain(
            checkout
                .ancestors()
                .take(4)
                .map(|ancestor| ancestor.join("Sage").join(&relative)),
        )
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("authoritative contract artifact {name} was not found"))
}

fn contract(name: &str) -> String {
    fs::read_to_string(contract_path(name)).expect("contract artifact must be readable")
}

#[test]
fn rust_limits_equal_the_authoritative_numeric_ledger() {
    validate_limits_document(&contract("limits-v1.json")).expect("limits must be synchronized");
}

#[test]
fn every_named_limit_fixture_has_exact_and_plus_one_values() {
    validate_limit_fixture_coverage(
        &contract("limits-v1.json"),
        &contract("limit-fixture-coverage-v1.json"),
        &contract("fixture-vectors-v1.json"),
    )
    .expect("limit fixture coverage must be complete");
}
