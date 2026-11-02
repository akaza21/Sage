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

#[cfg(any(windows, unix))]
#[test]
fn non_unicode_case_exits_two_without_panicking() {
    #[cfg(windows)]
    let case = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    };
    #[cfg(unix)]
    let case = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff])
    };
    let status = Command::new(FIXTURE)
        .arg("--case")
        .arg(case)
        .args(["--seed", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

/// Runs the lost-Steer Codex journey case on the fixture executable with
/// `requests` as its whole standard input, and returns its exit code and the
/// JSON frames it wrote. A replay case ends itself after 120 seconds, so
/// waiting on the process is bounded without a test deadline.
fn lost_steer_journey(requests: &[serde_json::Value]) -> (Option<i32>, Vec<serde_json::Value>) {
    use std::{
        io::{Read, Write},
        thread,
    };

    let mut command = Command::new(FIXTURE);
    command
        .args(["--case", "codex-steer-lost-result", "--seed", "0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        &mut command,
        windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
    let mut process = FixtureProcess(command.spawn().unwrap());
    let stdout = process.0.stdout.take().unwrap();
    let stderr = process.0.stderr.take().unwrap();
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(16_385).read_to_end(&mut bytes).unwrap();
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(1_025).read_to_end(&mut bytes).unwrap();
        bytes
    });
    let mut input = Vec::new();
    for request in requests {
        serde_json::to_writer(&mut input, request).unwrap();
        input.push(b'\n');
    }
    assert!(input.len() < 1_024);
    // Dropping standard input after the write is the client closing it.
    let write = process.0.stdin.take().unwrap().write_all(&input);
    let status = process.0.wait().unwrap();
    let stdout = stdout_reader.join().unwrap();
    let stderr = stderr_reader.join().unwrap();
    write.unwrap();
    assert!(stdout.len() <= 16_384);
    assert!(stderr.is_empty());
    let frames = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    (status.code(), frames)
}

fn codex_journey_requests(with_models: bool, with_steer: bool) -> Vec<serde_json::Value> {
    let mut requests = vec![
        serde_json::json!({"id":1,"method":"initialize","params":{}}),
        serde_json::json!({"method":"initialized"}),
        serde_json::json!({"id":2,"method":"thread/start","params":{}}),
        serde_json::json!({"id":3,"method":"thread/goal/get","params":{}}),
        serde_json::json!({"id":4,"method":"turn/start","params":{}}),
    ];
    if with_models {
        requests.insert(
            3,
            serde_json::json!({"id":90,"method":"model/list","params":{"cursor":null}}),
        );
    }
    if with_steer {
        requests.push(serde_json::json!({"id":5,"method":"turn/steer","params":{}}));
    }
    requests
}

#[test]
fn codex_fixture_executable_answers_goal_before_lost_steer_exit() {
    for with_models in [false, true] {
        let (code, frames) = lost_steer_journey(&codex_journey_requests(with_models, true));
        assert_eq!(
            code,
            Some(17),
            "the actual fixture executable must answer Goal hydration before losing Steer"
        );
        assert_eq!(frames.len(), if with_models { 6 } else { 5 });
        let catalogs: Vec<_> = frames.iter().filter(|frame| frame["id"] == 90).collect();
        assert_eq!(catalogs.len(), usize::from(with_models));
        if with_models {
            assert_eq!(catalogs[0]["result"]["nextCursor"], serde_json::Value::Null);
            assert_eq!(catalogs[0]["result"]["data"].as_array().unwrap().len(), 1);
        }
        let frames: Vec<_> = frames
            .into_iter()
            .filter(|frame| frame["id"] != 90)
            .collect();
        assert_eq!(frames[0]["id"], 1);
        assert_eq!(frames[1]["id"], 2);
        assert_eq!(
            frames[2],
            serde_json::json!({"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"Method not found"}})
        );
        assert_eq!(frames[3]["id"], 4);
        assert_eq!(frames[4]["method"], "turn/started");
        assert_eq!(
            frames[4]["params"]["turn"]["id"],
            frames[3]["result"]["turn"]["id"]
        );
        assert!(frames.iter().all(|frame| frame["id"] != 5));
    }
}

/// Exit 17 is earned by the Steer write: a client that closes its input
/// after the turn starts gets the same frames and a failed run instead.
#[test]
fn codex_fixture_executable_without_a_steer_write_never_reports_the_lost_steer() {
    let (code, frames) = lost_steer_journey(&codex_journey_requests(false, false));
    assert_eq!(code, Some(2));
    assert_eq!(frames.len(), 5);
    assert_eq!(frames[4]["method"], "turn/started");
}

struct FixtureProcess(std::process::Child);

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
