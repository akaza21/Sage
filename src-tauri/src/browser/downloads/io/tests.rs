use super::*;

#[cfg(windows)]
#[test]
fn equal_canonical_windows_paths_share_download_admission() {
    let executor = Executor::new(TIMEOUT);
    let folder = std::env::temp_dir().join(format!("sage-io-case-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(folder.join("Downloads")).unwrap();
    // Let the filesystem resolve the aliases. Do not impose Unicode folding
    // rules on either the volume or its canonical directory spelling.
    let first = std::fs::canonicalize(folder.join("Downloads")).unwrap();
    let second = std::fs::canonicalize(folder.join("DOWNLOADS")).unwrap();
    assert_eq!(first, second);
    let held = (0..MAX_PER_DESTINATION)
        .map(|index| {
            executor
                .acquire(if index % 2 == 0 { &first } else { &second })
                .unwrap()
        })
        .collect::<Vec<_>>();
    let extra = executor.acquire(&second);
    drop(held);
    assert!(
        extra.is_none(),
        "equal paths must share the admission bound"
    );
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_stuck_destination_has_bounded_admission_and_does_not_consume_local_capacity() {
    let executor = Executor::new(TIMEOUT);
    let folder = std::env::temp_dir().join(format!("sage-io-bound-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&folder).unwrap();
    let remote = folder.join("remote");
    let local = folder.join("local");
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = mpsc::sync_channel(1);
    let (done, completion) = mpsc::sync_channel(MAX_PER_DESTINATION);
    let first = executor.acquire(&remote).unwrap();
    let first_done = done.clone();
    assert!(first.run(
        move |_, deadline| {
            started.send(()).unwrap();
            let _ = wait.recv();
            deadline.check()
        },
        move |result| {
            first_done.send(result.unwrap_err().kind()).ok();
        },
        |_| {}
    ));
    start.recv_timeout(Duration::from_secs(1)).unwrap();
    for _ in 1..MAX_PER_DESTINATION {
        let output = remote.clone();
        let done = done.clone();
        assert!(executor.acquire(&remote).unwrap().run(
            move |_, _| { std::fs::write(output, b"must not run after expiry") },
            move |result| {
                done.send(result.unwrap_err().kind()).ok();
            },
            |_| {}
        ));
    }
    for _ in 0..MAX_PER_DESTINATION {
        assert_eq!(
            completion
                .recv_timeout(TIMEOUT + Duration::from_secs(1))
                .unwrap(),
            io::ErrorKind::TimedOut
        );
    }
    // Repeated timeouts cannot create new threads, tasks or retained results.
    for _ in 0..100 {
        assert!(executor.acquire(&remote).is_none());
    }
    let (done, completion) = mpsc::sync_channel(1);
    let output = local.clone();
    assert!(executor.acquire(&local).unwrap().run(
        move |_, deadline| {
            deadline.check()?;
            std::fs::write(output, b"local bytes")
        },
        move |result| {
            done.send(result).ok();
        },
        |_| {}
    ));
    completion
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&local).unwrap(), b"local bytes");
    let deadline = Instant::now() + Duration::from_secs(1);
    while executor.active.load(Ordering::Acquire) != MAX_PER_DESTINATION {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let held = (0..MAX_PER_DESTINATION)
        .map(|_| executor.acquire(&local).unwrap())
        .collect::<Vec<_>>();
    assert!(
        executor.acquire(&folder.join("third")).is_none(),
        "global admission must also be bounded"
    );
    drop(held);
    drop(release);
    let deadline = Instant::now() + Duration::from_secs(2);
    while executor.active.load(Ordering::Acquire) != 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!remote.exists(), "expired queued I/O must not resume later");
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn stalled_cleanup_does_not_block_a_new_destination() {
    let executor = Executor::new(TIMEOUT);
    let folder = std::env::temp_dir().join(format!("sage-io-cleanup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&folder).unwrap();
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = mpsc::sync_channel(1);
    executor
        .acquire(&folder.join("remote"))
        .unwrap()
        .cleanup(move |_| {
            started.send(()).unwrap();
            let _ = wait.recv();
        });
    start.recv_timeout(Duration::from_secs(1)).unwrap();
    let output = folder.join("download");
    let path = output.clone();
    let (done, completion) = mpsc::sync_channel(1);
    assert!(executor.acquire(&folder).unwrap().run(
        move |_, deadline| {
            deadline.check()?;
            std::fs::write(path, b"new download")
        },
        move |result| {
            done.send(result).ok();
        },
        |_| {}
    ));
    completion
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(output).unwrap(), b"new download");
    drop(release);
    let deadline = Instant::now() + Duration::from_secs(2);
    while executor.active.load(Ordering::Acquire) != 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::remove_dir_all(folder).unwrap();
}
