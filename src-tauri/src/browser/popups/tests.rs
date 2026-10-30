use super::*;

#[test]
fn closing_an_opener_retains_quarantined_children_until_confirmed_cleanup() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut requests = PopupRequests::default();
    let intent = requests
        .enqueue(
            &tabs,
            "browser-1",
            &Url::parse("https://example.com/").unwrap(),
        )
        .unwrap();
    requests.begin(&tabs, intent.id, "browser-1").unwrap();
    assert!(requests.opened(intent.id, "browser-2"));
    for _ in 0..2 {
        assert_eq!(
            requests.take_for_opener("browser-1"),
            vec![(intent.id, "browser-2".into())]
        );
        assert!(requests.pending().is_empty());
    }
    requests.finish_cleanup(intent.id, "browser-2");
    assert!(requests.take_for_opener("browser-1").is_empty());
}

#[test]
fn failed_preparation_retains_cleanup_identity_before_the_popup_is_opened() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut requests = PopupRequests::default();
    let intent = requests
        .enqueue(
            &tabs,
            "browser-1",
            &Url::parse("https://example.com/").unwrap(),
        )
        .unwrap();
    requests.begin(&tabs, intent.id, "browser-1").unwrap();
    requests.quarantine(intent.id, Some("browser-2".into()));
    assert!(requests.cleanup_target(intent.id, "browser-1", "browser-2"));
    assert!(requests.pending().is_empty());
    requests.finish_cleanup(intent.id, "browser-2");
    assert!(requests.entries.is_empty());
}

#[test]
fn failed_popup_close_keeps_the_exact_child_quarantined_until_cleanup_succeeds() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut requests = PopupRequests::default();
    let intent = requests
        .enqueue(
            &tabs,
            "browser-1",
            &Url::parse("https://example.com/").unwrap(),
        )
        .unwrap();
    requests.begin(&tabs, intent.id, "browser-1").unwrap();
    assert!(requests.opened(intent.id, "browser-2"));
    assert!(requests.cleanup_target(intent.id, "browser-1", "browser-2"));
    assert!(requests.cleanup_target(intent.id, "browser-1", "browser-2"));
    assert!(!requests.cleanup_target(intent.id, "browser-1", "browser-3"));
    assert!(!requests.ack(intent.id, "browser-1", "browser-2"));
    assert!(requests.pending().is_empty());
    requests.forget_child("browser-2");
    assert!(requests.entries.is_empty());
    assert!(requests.native_deadlines.is_empty());
}

#[test]
fn fallback_popup_without_an_ack_expires_and_closed_children_leave_no_deadlines() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut requests = PopupRequests::default();
    let intent = requests
        .enqueue(
            &tabs,
            "browser-1",
            &Url::parse("https://example.com/").unwrap(),
        )
        .unwrap();
    requests.begin(&tabs, intent.id, "browser-1").unwrap();
    assert!(requests.opened(intent.id, "browser-2"));
    assert_eq!(
        requests.take_expired_native(std::time::Instant::now() + opener::NATIVE_ACK_DEADLINE),
        vec![(intent.id, "browser-2".into())]
    );
    requests.finish_cleanup(intent.id, "browser-2");
    for _ in 0..32 {
        let intent = requests
            .enqueue(
                &tabs,
                "browser-1",
                &Url::parse("https://example.com/").unwrap(),
            )
            .unwrap();
        requests.begin(&tabs, intent.id, "browser-1").unwrap();
        assert!(requests.opened(intent.id, "browser-2"));
        requests.forget_child("browser-2");
        assert!(requests.native_deadlines.is_empty());
    }
}

#[test]
fn one_native_popup_intent_replays_and_acks_only_its_opened_tab() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut requests = PopupRequests::default();
    let requested = Url::parse("https://example.com/path").unwrap();
    let intent = requests.enqueue(&tabs, "browser-1", &requested).unwrap();
    assert_eq!(requests.pending(), vec![intent.clone()]);
    assert_eq!(
        requests.begin(&tabs, intent.id, "browser-1").unwrap(),
        PopupBegin::Create(intent.url.clone())
    );
    requests.opened(intent.id, "browser-2");
    assert_eq!(
        requests.begin(&tabs, intent.id, "browser-1").unwrap(),
        PopupBegin::Reuse("browser-2".into())
    );
    assert!(!requests.ack(intent.id, "browser-1", "browser-3"));
    assert!(!requests.ack(intent.id, "browser-3", "browser-2"));
    assert!(requests.ack(intent.id, "browser-1", "browser-2"));
    assert!(requests.pending().is_empty());
}

#[test]
fn popup_intents_are_bounded_and_reject_closed_openers_and_unsafe_urls() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut requests = PopupRequests::default();
    assert!(
        requests
            .enqueue(
                &tabs,
                "closed",
                &Url::parse("https://example.com/").unwrap()
            )
            .is_none()
    );
    assert!(
        requests
            .enqueue(&tabs, "browser-1", &Url::parse("file:///secret").unwrap())
            .is_none()
    );
    for _ in 0..MAX_PENDING_POPUPS {
        assert!(
            requests
                .enqueue(
                    &tabs,
                    "browser-1",
                    &Url::parse("https://example.com/").unwrap()
                )
                .is_some()
        );
    }
    assert!(
        requests
            .enqueue(
                &tabs,
                "browser-1",
                &Url::parse("https://example.com/").unwrap()
            )
            .is_none()
    );
}

#[test]
fn closing_opener_invalidates_an_in_flight_open_and_returns_opened_children() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut requests = PopupRequests::default();
    let url = Url::parse("https://example.com/").unwrap();
    let opening = requests.enqueue(&tabs, "browser-1", &url).unwrap();
    let opened = requests.enqueue(&tabs, "browser-1", &url).unwrap();
    assert!(matches!(
        requests.begin(&tabs, opening.id, "browser-1"),
        Ok(PopupBegin::Create(_))
    ));
    assert!(matches!(
        requests.begin(&tabs, opened.id, "browser-1"),
        Ok(PopupBegin::Create(_))
    ));
    assert!(requests.opened(opened.id, "browser-3"));
    assert_eq!(
        requests.take_for_opener("browser-1"),
        vec![(opened.id, "browser-3".into())]
    );
    assert!(!requests.opened(opening.id, "browser-2"));
    assert!(requests.pending().is_empty());
}
