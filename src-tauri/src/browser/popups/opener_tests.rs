use super::super::*;
use super::*;

fn live_opener() -> BrowserTabs<usize> {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    tabs
}

#[test]
fn native_popup_admission_matches_navigation_policy_plus_initial_blank() {
    let tabs = live_opener();
    assert_eq!(
        native_popup_target(&tabs, "browser-1", "https://example.com/a").as_deref(),
        Some("https://example.com/a")
    );
    assert_eq!(
        native_popup_target(&tabs, "browser-1", "about:blank").as_deref(),
        Some(INITIAL_BLANK)
    );
    for denied in [
        "javascript:alert(1)",
        "data:text/html,hi",
        "file:///C:/secret",
        "tauri://localhost/",
        "http://tauri.localhost/index.html#x\u{7}",
        "about:blank#x",
        "about:blank?x",
        "about:srcdoc",
        "ABOUT:BLANK",
        "https://user:pass@example.com/",
        "",
    ] {
        assert!(
            native_popup_target(&tabs, "browser-1", denied).is_none(),
            "{denied} must be denied"
        );
    }
    assert!(native_popup_target(&tabs, "closed", "about:blank").is_none());
    assert!(native_popup_target(&tabs, "closed", "https://example.com/").is_none());
}

#[test]
fn native_popup_admission_counts_against_the_tab_cap() {
    let mut tabs = live_opener();
    for number in 2..=MAX_BROWSER_TABS {
        tabs.insert(format!("browser-{number}"), number, false);
    }
    assert!(native_popup_target(&tabs, "browser-1", "about:blank").is_none());
    assert!(native_popup_target(&tabs, "browser-1", "https://example.com/").is_none());
    tabs.entries.remove("browser-8");
    tabs.opening.insert("browser-9".into());
    assert!(native_popup_target(&tabs, "browser-1", "https://example.com/").is_none());
}

#[test]
fn native_reservation_is_bounded_hidden_and_reuses_opener_cleanup() {
    let tabs = live_opener();
    let mut requests = PopupRequests::default();
    let blank = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert_eq!(blank.url, INITIAL_BLANK);
    assert!(
        requests.pending().is_empty(),
        "in-flight native popups are not replayed"
    );
    assert_eq!(
        requests.begin(&tabs, blank.id, "browser-1"),
        Err("popup open is in progress".into())
    );
    assert!(requests.native_opened(blank.id, "browser-2"));
    assert!(!requests.native_opened(blank.id, "browser-2"));
    assert_eq!(requests.pending(), vec![blank.clone()]);
    assert_eq!(
        requests.begin(&tabs, blank.id, "browser-1"),
        Ok(PopupBegin::Reuse("browser-2".into()))
    );
    let https = requests
        .reserve_native(&tabs, "browser-1", "https://example.com/")
        .unwrap();
    assert_eq!(
        requests.take_for_opener("browser-1"),
        vec![(blank.id, "browser-2".into())]
    );
    assert!(
        !requests.native_opened(https.id, "browser-3"),
        "closed opener drops in-flight popup"
    );
    for _ in 0..MAX_PENDING_POPUPS {
        requests.reserve_native(&tabs, "browser-1", "https://example.com/");
    }
    assert!(
        requests
            .reserve_native(&tabs, "browser-1", "https://example.com/")
            .is_none()
    );
}

#[test]
fn failed_native_popup_releases_only_its_in_flight_entry() {
    let tabs = live_opener();
    let mut requests = PopupRequests::default();
    let failed = requests
        .reserve_native(&tabs, "browser-1", "https://example.com/")
        .unwrap();
    let kept = requests
        .reserve_native(&tabs, "browser-1", "https://example.com/")
        .unwrap();
    assert!(requests.native_opened(kept.id, "browser-2"));
    requests.native_failed(failed.id);
    requests.native_failed(kept.id);
    assert_eq!(requests.pending(), vec![kept]);
    assert!(!requests.native_opened(failed.id, "browser-3"));
}

#[test]
fn initial_blank_is_admitted_only_for_a_native_opener_linked_child_until_it_navigates() {
    let tabs = live_opener();
    let mut requests = PopupRequests::default();
    assert!(!requests.admit_navigation("browser-2", "about:blank"));
    let blank = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(!requests.admit_navigation("browser-2", "about:blank"));
    assert!(requests.native_opened(blank.id, "browser-2"));
    assert!(requests.admit_navigation("browser-2", "about:blank"));
    assert!(requests.admit_navigation("browser-2", "about:blank"));
    assert!(!requests.admit_navigation("browser-3", "about:blank"));
    for denied in [
        "javascript:alert(1)",
        "data:text/html,x",
        "file:///x",
        "about:srcdoc",
    ] {
        assert!(!requests.admit_navigation("browser-2", denied));
    }
    assert!(!requests.admit_navigation("browser-2", "about:blank"));

    let next = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(requests.native_opened(next.id, "browser-4"));
    assert!(requests.admit_navigation("browser-4", "https://example.com/"));
    assert!(!requests.admit_navigation("browser-4", "about:blank"));

    let https = requests
        .reserve_native(&tabs, "browser-1", "https://example.com/")
        .unwrap();
    assert!(requests.native_opened(https.id, "browser-5"));
    assert!(!requests.admit_navigation("browser-5", "about:blank"));

    let closed = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(requests.native_opened(closed.id, "browser-6"));
    requests.forget_child("browser-6");
    assert!(!requests.admit_navigation("browser-6", "about:blank"));
}
