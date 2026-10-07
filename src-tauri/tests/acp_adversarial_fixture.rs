#![cfg(feature = "acp-fixture")]

use std::path::Path;
use std::process::{Command, Stdio};

const FIXTURE: &str = env!("CARGO_BIN_EXE_acp-adversarial-fixture");

#[test]
fn the_adversarial_fixture_binary_is_built_alongside_the_test_suite() {
    let executable = Path::new(FIXTURE);
    assert!(
        executable.is_file(),
        "cargo did not build the adversarial fixture binary at {}",
        executable.display()
    );
}

#[test]
fn the_adversarial_fixture_binary_sits_where_the_library_process_tests_resolve_it() {
    let built_in = Path::new(FIXTURE).parent().unwrap().canonicalize().unwrap();
    let guessed_by_library_tests = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap();

    assert_eq!(built_in, guessed_by_library_tests);
}

#[test]
fn an_unparseable_invocation_exits_two_without_reading_standard_input() {
    let status = Command::new(FIXTURE)
        .arg("--not-a-fixture-case")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(2));
}
