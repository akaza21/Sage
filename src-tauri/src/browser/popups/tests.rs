use super::*;

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
