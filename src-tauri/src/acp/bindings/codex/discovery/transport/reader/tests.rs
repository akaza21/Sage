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
