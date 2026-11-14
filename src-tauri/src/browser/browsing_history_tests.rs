use super::*;

fn urls(history: &BrowsingHistory, search: &str) -> Vec<String> {
    history
        .page(search.into(), None, MAX_PAGE)
        .unwrap()
        .visits
        .into_iter()
        .map(|visit| visit.url)
        .collect()
}

#[test]
fn reads_see_every_visit_and_title_recorded_before_them() {
    let history = BrowsingHistory::in_memory();
    history.record_visit("browser-1", "https://a.example/", None);
    history.record_visit("browser-1", "https://b.example/", None);
    history.record_title("browser-1", "Bravo");
    let page = history.page(String::new(), None, 10).unwrap();
    assert_eq!(
        page.visits
            .iter()
            .map(|visit| (visit.url.as_str(), visit.title.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("https://b.example/", Some("Bravo")),
            ("https://a.example/", None)
        ]
    );
    assert!(page.visits[0].visited_at > 1_600_000_000_000);
}

#[test]
fn clearing_removes_visits_queued_before_it_and_keeps_later_ones() {
    let history = BrowsingHistory::in_memory();
    let recorder = history.clone();
    for index in 0..50 {
        recorder.record_visit("browser-1", &format!("https://{index}.example/"), None);
    }
    assert_eq!(history.clear().unwrap(), 50);
    recorder.record_visit("browser-1", "https://after.example/", None);
    assert_eq!(urls(&history, ""), ["https://after.example/"]);
}

#[test]
fn removing_and_searching_go_through_the_worker() {
    let history = BrowsingHistory::in_memory();
    history.record_visit("browser-1", "https://docs.example/", None);
    history.record_visit("browser-1", "https://news.example/", None);
    assert_eq!(urls(&history, "docs"), ["https://docs.example/"]);
    let id = history
        .page("news".into(), None, 1)
        .unwrap()
        .visits
        .remove(0)
        .id;
    assert_eq!(history.remove(vec![id]).unwrap(), 1);
    assert_eq!(urls(&history, ""), ["https://docs.example/"]);
}

#[test]
fn a_disabled_handle_records_nothing_and_reports_reads_unavailable() {
    let history = BrowsingHistory::default();
    history.record_visit("browser-1", "https://a.example/", None);
    history.record_title("browser-1", "A");
    assert_eq!(
        history.page(String::new(), None, 1).unwrap_err(),
        UNAVAILABLE
    );
    assert_eq!(history.clear().unwrap_err(), UNAVAILABLE);
}

#[test]
fn a_store_that_cannot_open_drops_writes_and_fails_reads_without_panicking() {
    let missing = std::env::temp_dir()
        .join(format!("sage-missing-{}", uuid::Uuid::new_v4()))
        .join("nested")
        .join("browsing-history.db");
    let history = BrowsingHistory::open(missing);
    history.record_visit("browser-1", "https://a.example/", None);
    assert_eq!(
        history.page(String::new(), None, 1).unwrap_err(),
        UNAVAILABLE
    );
    assert_eq!(history.remove(vec![1]).unwrap_err(), UNAVAILABLE);
}

#[test]
fn recording_never_waits_for_the_disk() {
    let history = BrowsingHistory::in_memory();
    let started = std::time::Instant::now();
    for index in 0..(QUEUE * 4) {
        history.record_visit("browser-1", &format!("https://{index}.example/"), None);
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    let kept = history.page(String::new(), None, MAX_PAGE).unwrap();
    assert!(!kept.visits.is_empty());
}

fn stalled() -> (BrowsingHistory, Receiver<Request>) {
    let history = BrowsingHistory::in_memory();
    let (sender, receiver) = sync_channel(1);
    let shared = history.0.as_ref().unwrap();
    assert!(shared.queue.set(sender).is_ok());
    (history, receiver)
}

#[test]
fn a_write_the_full_queue_drops_reports_that_it_was_not_queued() {
    let (history, receiver) = stalled();
    assert!(history.record_visit("browser-1", "https://a.example/", None));
    assert!(!history.record_visit("browser-1", "https://b.example/", Some("B")));
    assert!(!history.record_same_document("browser-1", "https://b.example/#c", None));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        Request::Write(Write::Visit { url, .. }) if url == "https://a.example/"
    ));
    assert!(history.record_same_document("browser-1", "https://a.example/#c", None));
    assert!(!BrowsingHistory::default().record_visit("browser-1", "https://a.example/", None));
}

#[test]
fn a_failed_visit_never_retargets_its_later_updates_to_the_previous_visit() {
    let directory =
        std::env::temp_dir().join(format!("sage-history-failure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join(FILE_NAME);
    let control = store::open(path.to_str().unwrap()).unwrap();
    let history = BrowsingHistory::open(path.clone());
    let (sender, receiver) = sync_channel(QUEUE);
    history.0.as_ref().unwrap().queue.set(sender).ok().unwrap();
    let worker = std::thread::spawn(move || run(&path, &receiver));
    history.record_visit("browser-1", "https://a.example/", Some("Alpha"));
    let original = history.page(String::new(), None, 10).unwrap().visits;
    control
        .execute_batch(
            "CREATE TRIGGER fail_visit BEFORE INSERT ON browser_visits
         BEGIN SELECT RAISE(ABORT, 'history storage failure'); END;",
        )
        .unwrap();
    history.record_visit("browser-1", "https://b.example/", None);
    assert_eq!(
        history.page(String::new(), None, 10).unwrap().visits,
        original
    );
    control.execute_batch("DROP TRIGGER fail_visit;").unwrap();
    history.record_title("browser-1", "Bravo");
    let after_title = history.page(String::new(), None, 10).unwrap().visits;
    history.record_same_document("browser-1", "https://b.example/#moved", Some("Bravo moved"));
    let after_move = history.page(String::new(), None, 10).unwrap().visits;
    drop(control);
    drop(history);
    worker.join().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(after_title, original);
    assert_eq!(after_move.len(), 2);
    assert_eq!(after_move[1], original[0]);
    assert_eq!(after_move[0].url, "https://b.example/#moved");
    assert_eq!(after_move[0].title.as_deref(), Some("Bravo moved"));
}

#[test]
fn an_idle_worker_releases_sqlite_and_reopens_with_attribution_and_ordering() {
    let directory =
        std::env::temp_dir().join(format!("sage-history-idle-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join(FILE_NAME);
    let wal = directory.join(format!("{FILE_NAME}-wal"));
    let history = BrowsingHistory::open(path.clone());
    let (sender, receiver) = sync_channel(QUEUE);
    history.0.as_ref().unwrap().queue.set(sender).ok().unwrap();
    let worker = std::thread::spawn(move || run(&path, &receiver));
    history.record_visit("browser-1", "https://a.example/", Some("Alpha"));
    history.record_visit("browser-2", "https://b.example/", Some("Beta"));
    let mut expected = history.page(String::new(), None, 10).unwrap().visits;
    let opened = wal.exists();
    // SQLite removes its WAL when its last connection closes. Observe the
    // real file, without adding a resource counter to the production handle.
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    while wal.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let released = !wal.exists();
    history.record_title("browser-1", "Alpha resumed");
    history.record_title("browser-2", "Beta resumed");
    let reopened = history.page(String::new(), None, 10).unwrap().visits;
    let reopened_file = wal.exists();
    history.record_visit("browser-1", "https://before-clear.example/", None);
    let cleared = history.clear().unwrap();
    history.record_visit("browser-1", "https://after-clear.example/", None);
    let after_clear = urls(&history, "");
    drop(history);
    worker.join().unwrap();
    std::fs::remove_dir_all(directory).unwrap();

    assert!(opened, "the first visit must open the real SQLite file");
    assert!(
        released,
        "inactivity must release the SQLite connection and WAL"
    );
    assert!(reopened_file, "later writes must reopen the SQLite file");
    assert_eq!(expected.len(), 2);
    for visit in &mut expected {
        visit.title = Some(
            match visit.url.as_str() {
                "https://a.example/" => "Alpha resumed",
                "https://b.example/" => "Beta resumed",
                _ => panic!("unexpected stored visit"),
            }
            .into(),
        );
    }
    assert_eq!(reopened, expected);
    assert_eq!(cleared, 3);
    assert_eq!(after_clear, ["https://after-clear.example/"]);
}
