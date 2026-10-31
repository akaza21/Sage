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
    // Far more visits than the queue holds return at once: excess visits
    // are dropped, never waited on.
    assert!(started.elapsed() < Duration::from_secs(2));
    let kept = history.page(String::new(), None, MAX_PAGE).unwrap();
    assert!(!kept.visits.is_empty());
}

/// A handle whose queue holds one message and is drained by the test.
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
