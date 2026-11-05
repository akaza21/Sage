use super::*;
use std::{
    io::Cursor,
    process::{Command, Stdio},
};

fn spawn(script: &str) -> ChildGuard {
    let started = Instant::now();
    let child = Command::new("node")
        .args(["-e", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    ChildGuard::new(child, started, Instant::now())
}

fn timeout(child: &mut ChildGuard) -> String {
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        child.wait_until(Instant::now());
    }))
    .unwrap_err();
    *panic.downcast::<String>().unwrap()
}

#[test]
fn timeout_reports_partial_pipes_and_flushed_sink_without_disclosing_output() {
    let mut child = spawn(
        "const fs = require('node:fs'); \
         fs.writeSync(1, 'private stdout'); \
         fs.writeSync(2, 'sage-redaction:sink-emitted\\nprivate stderr'); \
         setInterval(() => {}, 1000);",
    );
    let (_stdout, _stderr) = child.read_streams();
    // Wait for the real readers to observe the writes before forcing the
    // watchdog deadline. This is a readiness barrier, not a latency assertion.
    let deadline = Instant::now() + NODE_TIMEOUT;
    loop {
        let progress = child.progress.lock().unwrap();
        if progress.pipes[0].bytes == 14 && progress.pipes[1].bytes == 42 {
            break;
        }
        drop(progress);
        assert!(
            Instant::now() < deadline,
            "fixture writes were not observed"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let message = timeout(&mut child);
    assert!(message.contains("child=alive"), "{message}");
    assert!(
        message.contains("stdout_bytes=14 stdout_eof=false"),
        "{message}"
    );
    assert!(
        message.contains("stderr_bytes=42 stderr_eof=false"),
        "{message}"
    );
    assert!(message.contains("last_state=waiting-child"), "{message}");
    assert!(message.contains("sink_emitted=yes"), "{message}");
    assert!(!message.contains("private"), "{message}");
    assert!(message.len() < 1024, "{message}");
}

#[test]
fn observation_retains_exit_code_and_actual_pipe_eof() {
    let mut child = spawn(
        "const fs = require('node:fs'); \
         fs.writeSync(1, 'result'); fs.writeSync(2, 'sage-redaction:sink-emitted\\n'); \
         process.exit(23);",
    );
    let (stdout, stderr) = child.read_streams();
    let deadline = Instant::now() + NODE_TIMEOUT;
    assert_eq!(child.wait_until(deadline).code(), Some(23));
    assert_eq!(child.receive_stream(stdout, deadline), b"result");
    assert_eq!(child.receive_stream(stderr, deadline), SINK_EMITTED);
    let message = child.timeout_observation();
    assert!(
        message.contains("child=exited exit_code=Some(23)"),
        "{message}"
    );
    assert!(
        message.contains("stdout_bytes=6 stdout_eof=true"),
        "{message}"
    );
    assert!(
        message.contains("stderr_bytes=28 stderr_eof=true"),
        "{message}"
    );
    assert!(message.contains("sink_emitted=yes"), "{message}");
    #[cfg(windows)]
    assert!(!message.contains("unavailable"), "{message}");
}

#[test]
fn pipe_cap_is_distinct_from_eof() {
    let progress = Arc::new(Mutex::new(Progress::default()));
    let stream = read_node_stream(std::io::repeat(b'x'), 0, Arc::clone(&progress));
    let output = stream.receiver.recv_timeout(NODE_TIMEOUT).unwrap().unwrap();
    assert_eq!(output, vec![b'x'; 65_537]);
    let pipe = progress.lock().unwrap().pipes[0];
    assert_eq!(pipe.bytes, 65_537);
    assert!(!pipe.eof);
}

#[test]
fn emission_marker_split_across_reads_preserves_pipe_output() {
    struct OneByte(Cursor<Vec<u8>>);
    impl Read for OneByte {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(&mut output[..1])
        }
    }
    let progress = Arc::new(Mutex::new(Progress::default()));
    let stream = read_node_stream(
        OneByte(Cursor::new(SINK_EMITTED.to_vec())),
        1,
        Arc::clone(&progress),
    );
    assert_eq!(
        stream.receiver.recv_timeout(NODE_TIMEOUT).unwrap().unwrap(),
        SINK_EMITTED
    );
    let progress = progress.lock().unwrap();
    assert!(progress.sink_emitted);
    assert!(progress.pipes[1].eof);
}

#[test]
fn timeout_reports_live_child_without_disclosing_output() {
    let started = Instant::now();
    let child = Command::new("node")
        .args(["-e", "setInterval(() => {}, 1000)"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard::new(child, started, Instant::now());
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        child.wait_until(Instant::now());
    }))
    .unwrap_err();
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(message.contains("child=alive"), "{message}");
    assert!(
        message.contains("stdout_bytes=0 stdout_eof=false"),
        "{message}"
    );
    assert!(
        message.contains("stderr_bytes=0 stderr_eof=false"),
        "{message}"
    );
    assert!(message.contains("last_state=waiting-child"), "{message}");
    assert!(message.contains("state_at="), "{message}");
    assert!(message.contains("sink_emitted=not-observed"), "{message}");
    #[cfg(windows)]
    {
        assert!(message.contains("userMs="), "{message}");
        assert!(message.contains("kernelMs="), "{message}");
        assert!(!message.contains("unavailable"), "{message}");
    }
    assert!(message.len() < 1024, "{message}");
}
