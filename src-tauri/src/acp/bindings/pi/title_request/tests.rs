use super::*;
use std::thread;
pub(crate) static PROCESS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fixture(scenario: &str) -> Command {
    let mut command = Command::new("node");
    command
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/acp/bindings/pi/title_request/fixture.mjs"
        ))
        .arg(scenario)
        .args(TITLE_ARGS)
        .args(["--provider", "fixture", "--model", "same-model"]);
    command
}

fn finish(request: &mut TitleRequest) -> Option<String> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(result) = request.poll() {
            return result;
        }
        assert!(Instant::now() < end, "fixture did not finish");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn isolated_rpc_accepts_only_a_title_with_all_resources_disabled() {
    let _process = PROCESS_LOCK.lock().unwrap();
    let mut request = TitleRequest::start(
        fixture("success"),
        "User:\nCheck Sage\nAssistant:\nChecked Sage",
    )
    .unwrap();
    assert_eq!(
        finish(&mut request).as_deref(),
        Some("Check Sage chat titles")
    );
    assert!(request.tree.is_empty());
}

#[test]
fn a_valid_rpc_title_is_collected_before_tree_cleanup_without_sending_eof() {
    let _process = PROCESS_LOCK.lock().unwrap();
    let mut request = TitleRequest::start(fixture("eof-error"), "first exchange").unwrap();
    assert_eq!(
        finish(&mut request).as_deref(),
        Some("Check Sage chat titles")
    );
    assert!(request.tree.is_empty());
}

#[test]
fn failures_and_oversize_output_preserve_the_local_fallback() {
    let _process = PROCESS_LOCK.lock().unwrap();
    for scenario in ["nonzero", "malformed", "oversize", "stream-oversize"] {
        let mut request = TitleRequest::start(fixture(scenario), "first exchange").unwrap();
        assert_eq!(finish(&mut request), None, "{scenario}");
    }
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../work")
            .join(format!("pi-title-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn bounded_exchange_is_sent_once() {
    let _process = PROCESS_LOCK.lock().unwrap();
    let directory = Directory::new();
    let mut command = fixture("success");
    command.env("TITLE_FIXTURE_DIR", &directory.0);
    let input = "\u{754c}".repeat(1365) + "x";
    assert_eq!(input.len(), 4096);
    let mut request = TitleRequest::start(command, &input).unwrap();
    assert!(finish(&mut request).is_some());
    drop(request);
    let lines = std::fs::read_to_string(directory.0.join("isolated-input.jsonl")).unwrap();
    assert_eq!(lines.lines().count(), 1);
    let prompt: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
    assert_eq!(prompt["message"], input);
    assert!(TitleRequest::start(fixture("success"), &"x".repeat(4097)).is_err());
}

#[cfg(windows)]
#[test]
fn timeout_and_cancellation_kill_the_whole_process_tree() {
    let _process = PROCESS_LOCK.lock().unwrap();
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    for timeout in [true, false] {
        let directory = Directory::new();
        let mut command = fixture("hang");
        command.env("TITLE_FIXTURE_DIR", &directory.0);
        let mut request = TitleRequest::start(command, "first exchange").unwrap();
        let marker = directory.0.join("child.pid");
        let end = Instant::now() + Duration::from_secs(5);
        while !marker.exists() {
            assert!(Instant::now() < end, "fixture child did not start");
            thread::sleep(Duration::from_millis(5));
        }
        let pid: u32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
        // SAFETY: opening a synchronization-only handle to our fixture child.
        let child = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        assert!(!child.is_null());
        if timeout {
            assert_eq!(finish(&mut request), None);
        } else {
            request.cancel();
            assert_eq!(finish(&mut request), None);
        }
        drop(request); // The same ownership drop is used by adapter cancellation.
        // SAFETY: this test owns the live handle until CloseHandle.
        assert_eq!(unsafe { WaitForSingleObject(child, 2000) }, WAIT_OBJECT_0);
        unsafe {
            CloseHandle(child);
        }
    }
}
