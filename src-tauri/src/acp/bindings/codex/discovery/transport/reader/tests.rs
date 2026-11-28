use super::*;
use crate::acp::transport::IncrementalFrameDecoder;

#[test]
fn discovery_reader_delivers_a_notification_burst_then_the_response_in_order() {
    let deadline = Instant::now() + Duration::from_secs(5);
    let response = br#"{"id":1,"result":{}}"#;
    let pending = (4..15)
        .map(|sequence| ReaderEvent::Frame(notification(sequence)))
        .chain([ReaderEvent::Frame(frame(response)), ReaderEvent::Eof])
        .collect();
    let mut reader = reader_with_full_queue(pending, deadline);

    assert_eq!(
        reader.completion.recv_timeout(Duration::from_millis(20)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    for sequence in 0..15 {
        let ReaderEvent::Frame(actual) = reader.recv_timeout(Duration::from_secs(1)).unwrap()
        else {
            panic!("expected startup notification {sequence}");
        };
        assert_eq!(actual.as_bytes(), notification(sequence).as_bytes());
    }
    let ReaderEvent::Frame(actual) = reader.recv_timeout(Duration::from_secs(1)).unwrap() else {
        panic!("expected initialize response after notifications");
    };
    assert_eq!(actual.as_bytes(), response);
    assert!(matches!(
        reader.recv_timeout(Duration::from_secs(1)).unwrap(),
        ReaderEvent::Eof
    ));
    assert_eq!(reader.finish_before(deadline), Ok(()));
    assert!(!reader.overflowed());
}

#[test]
fn full_reader_channel_is_an_explicit_overflow_not_clean_eof() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let overflowed = AtomicBool::new(false);
    let deadline = Instant::now() + Duration::from_millis(20);
    assert_eq!(
        enqueue_reader_event(
            &sender,
            ReaderEvent::Frame(frame(b"{}")),
            &overflowed,
            deadline,
        ),
        None
    );
    assert_eq!(
        enqueue_reader_event(
            &sender,
            ReaderEvent::Frame(frame(b"{}")),
            &overflowed,
            deadline,
        ),
        Some(ReaderCompletion::Overflow)
    );
    assert!(Instant::now() >= deadline);
    assert!(overflowed.load(Ordering::Acquire));
}

#[test]
fn cleanup_releases_a_reader_waiting_on_a_full_queue() {
    let mut reader = reader_with_full_queue(
        vec![ReaderEvent::Eof],
        Instant::now() + Duration::from_secs(30),
    );
    assert_eq!(
        reader.completion.recv_timeout(Duration::from_millis(20)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    assert_eq!(
        reader.finish_before(Instant::now() + Duration::from_secs(1)),
        Ok(())
    );
    assert!(!reader.overflowed());
}

fn reader_with_full_queue(pending: Vec<ReaderEvent>, deadline: Instant) -> ReaderThread {
    let (sender, events) = mpsc::sync_channel(4);
    let (send_completion, completion) = mpsc::sync_channel(1);
    let overflowed = Arc::new(AtomicBool::new(false));
    for sequence in 0..4 {
        assert_eq!(
            enqueue_reader_event(
                &sender,
                ReaderEvent::Frame(notification(sequence)),
                &overflowed,
                deadline,
            ),
            None
        );
    }
    let reader_overflowed = Arc::clone(&overflowed);
    let handle = thread::spawn(move || {
        for event in pending {
            if let Some(completion) =
                enqueue_reader_event(&sender, event, &reader_overflowed, deadline)
            {
                send_completion.send(completion).unwrap();
                return;
            }
        }
        panic!("test input must end with a terminal event");
    });
    ReaderThread {
        events: Some(events),
        completion,
        overflowed,
        discard: Arc::new(AtomicBool::new(false)),
        handle: Some(handle),
    }
}

fn notification(sequence: usize) -> Frame {
    frame(format!(r#"{{"method":"startup/status","params":{{"sequence":{sequence}}}}}"#).as_bytes())
}

fn frame(payload: &[u8]) -> Frame {
    let mut decoder = IncrementalFrameDecoder::new();
    let mut line = payload.to_vec();
    line.push(b'\n');
    decoder.push_chunk(&line).unwrap().unwrap()
}

/// A harness that keeps writing after the run has its answer, well past any
/// pipe buffer, and only then saves its work.
#[test]
fn discarded_output_lets_a_chatty_harness_finish_its_grace() {
    use crate::acp::process::{TreeExit, graceful_exit};

    let scratch =
        std::env::temp_dir().join(format!("sage-reader-discard-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).unwrap();
    let marker = scratch.join("saved");
    let line = format!("[{}0]", "0,".repeat(48));
    #[cfg(windows)]
    let tree = crate::acp::process::launch_script_for_test(
        81,
        &format!(
            r"%SystemRoot%\System32\findstr.exe x >nul & (for /L %i in (1,1,2000) do @echo {line}) & type nul > {}",
            marker.display()
        ),
    );
    #[cfg(not(windows))]
    let tree = crate::acp::process::launch_shell_for_test(
        81,
        &format!(
            "cat >/dev/null; i=0; while [ $i -lt 2000 ]; do echo '{line}'; i=$((i+1)); done; : > '{}'",
            marker.display()
        ),
    );
    let (mut stdin, stdout, control) = tree.into_parts();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut reader = ReaderThread::spawn(stdout, 1, "test", deadline).unwrap();
    reader.discard_output();
    stdin.close();
    graceful_exit::end_tree_before(&control, TreeExit::ParentShutdown, deadline).unwrap();
    assert!(
        marker.exists(),
        "the harness wrote everything and saved its work"
    );
    assert_eq!(reader.finish_before(deadline), Ok(()));
    let _ = std::fs::remove_dir_all(&scratch);
}

/// A line past the frame limit after the answer must not close the pipe: the
/// harness writes again and only then saves its work.
#[cfg(not(windows))]
#[test]
fn an_oversized_line_during_discard_keeps_the_pipe_open() {
    use crate::acp::process::{TreeExit, graceful_exit};

    let scratch =
        std::env::temp_dir().join(format!("sage-reader-oversized-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).unwrap();
    let marker = scratch.join("saved");
    let tree = crate::acp::process::launch_shell_for_test(
        82,
        &format!(
            "cat >/dev/null; head -c 1100000 /dev/zero | tr '\0' a; echo; echo '[0]'; sleep 1; : > '{}'",
            marker.display()
        ),
    );
    let (mut stdin, stdout, control) = tree.into_parts();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut reader = ReaderThread::spawn(stdout, 1, "test", deadline).unwrap();
    reader.discard_output();
    stdin.close();
    graceful_exit::end_tree_before(&control, TreeExit::ParentShutdown, deadline).unwrap();
    assert!(
        marker.exists(),
        "the harness wrote past the oversized line and saved"
    );
    assert_eq!(reader.finish_before(deadline), Ok(()));
    let _ = std::fs::remove_dir_all(&scratch);
}
