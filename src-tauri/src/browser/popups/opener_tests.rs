use super::super::*;
use super::*;
use std::time::{Duration, Instant};

fn t0() -> Instant {
    Instant::now()
}

fn linked(intent: &PopupIntent, tab_id: &str) -> PopupIntent {
    PopupIntent {
        native_tab_id: Some(tab_id.into()),
        ..intent.clone()
    }
}

fn admits(requests: &mut PopupRequests, tab_id: &str, uri: &str) -> bool {
    requests.admit_navigation(tab_id, uri) == NavigationVerdict::Admit
}

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
    assert!(requests.native_opened(blank.id, "browser-2", t0()));
    assert!(!requests.native_opened(blank.id, "browser-2", t0()));
    assert_eq!(requests.pending(), vec![linked(&blank, "browser-2")]);
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
        !requests.native_opened(https.id, "browser-3", t0()),
        "closed opener drops in-flight popup"
    );
    for filled in 1..MAX_PENDING_POPUPS {
        assert!(
            requests
                .reserve_native(&tabs, "browser-1", "https://example.com/")
                .is_some(),
            "entry {filled} fits"
        );
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
    assert!(requests.native_opened(kept.id, "browser-2", t0()));
    requests.native_failed(failed.id);
    requests.native_failed(kept.id);
    assert_eq!(requests.pending(), vec![linked(&kept, "browser-2")]);
    assert!(!requests.native_opened(failed.id, "browser-3", t0()));
}

#[test]
fn initial_blank_is_admitted_only_for_a_native_opener_linked_child_until_it_navigates() {
    let tabs = live_opener();
    let mut requests = PopupRequests::default();
    assert!(!admits(&mut requests, "browser-2", "about:blank"));
    let blank = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(!admits(&mut requests, "browser-2", "about:blank"));
    assert!(requests.native_opened(blank.id, "browser-2", t0()));
    assert!(admits(&mut requests, "browser-2", "about:blank"));
    assert!(admits(&mut requests, "browser-2", "about:blank"));
    assert!(!admits(&mut requests, "browser-3", "about:blank"));
    for denied in [
        "javascript:alert(1)",
        "data:text/html,x",
        "file:///x",
        "about:srcdoc",
    ] {
        assert!(!admits(&mut requests, "browser-2", denied));
    }
    assert!(!admits(&mut requests, "browser-2", "about:blank"));

    let next = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(requests.native_opened(next.id, "browser-4", t0()));
    assert!(admits(&mut requests, "browser-4", "https://example.com/"));
    assert!(!admits(&mut requests, "browser-4", "about:blank"));

    let https = requests
        .reserve_native(&tabs, "browser-1", "https://example.com/")
        .unwrap();
    assert!(requests.native_opened(https.id, "browser-5", t0()));
    assert!(!admits(&mut requests, "browser-5", "about:blank"));

    let closed = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(requests.native_opened(closed.id, "browser-6", t0()));
    requests.forget_child("browser-6");
    assert!(!admits(&mut requests, "browser-6", "about:blank"));
}

#[test]
fn dispatch_failure_releases_the_reserved_slot() {
    let tabs = live_opener();
    let mut requests = PopupRequests::default();
    let reserved = (0..MAX_PENDING_POPUPS)
        .map(|_| {
            requests
                .reserve_native(&tabs, "browser-1", "https://example.com/")
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        requests
            .reserve_native(&tabs, "browser-1", "https://example.com/")
            .is_none()
    );
    requests.native_failed(reserved[0].id);
    assert!(
        requests
            .reserve_native(&tabs, "browser-1", "https://example.com/")
            .is_some()
    );
}

#[test]
fn unacknowledged_native_children_expire_at_the_deadline_only() {
    let tabs = live_opener();
    let mut requests = PopupRequests::default();
    let start = Instant::now();
    let late = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    let acked = requests
        .reserve_native(&tabs, "browser-1", "https://example.com/")
        .unwrap();
    let fresh = requests
        .reserve_native(&tabs, "browser-1", "https://example.com/")
        .unwrap();
    assert!(requests.native_opened(late.id, "browser-2", start));
    assert!(requests.native_opened(acked.id, "browser-3", start));
    let later = start + Duration::from_secs(10);
    assert!(requests.native_opened(fresh.id, "browser-4", later));
    assert!(requests.ack(acked.id, "browser-1", "browser-3"));

    let before = start + NATIVE_ACK_DEADLINE - Duration::from_millis(1);
    assert!(requests.take_expired_native(before).is_empty());
    let due = start + NATIVE_ACK_DEADLINE;
    assert_eq!(
        requests.take_expired_native(due),
        vec![(late.id, "browser-2".into())]
    );
    assert!(requests.take_expired_native(due).is_empty());
    requests.finish_cleanup(late.id, "browser-2");
    assert_eq!(requests.pending(), vec![linked(&fresh, "browser-4")]);
    assert_eq!(
        requests.take_expired_native(later + NATIVE_ACK_DEADLINE),
        vec![(fresh.id, "browser-4".into())]
    );
    assert!(requests.native_deadlines.is_empty());
}

#[test]
fn a_blank_child_whose_first_real_navigation_is_refused_is_abandoned() {
    let tabs = live_opener();
    let mut requests = PopupRequests::default();
    // WebView2 reports `window.open('javascript:…')` as an about:blank request.
    let scripted = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(requests.native_opened(scripted.id, "browser-2", t0()));
    assert_eq!(
        requests.admit_navigation("browser-2", "about:blank"),
        NavigationVerdict::Admit
    );
    assert_eq!(
        requests.admit_navigation("browser-2", "javascript:fetch('/x')"),
        NavigationVerdict::Abandon
    );
    // Only the navigation that spent the grant abandons; WebView2 and wry
    // both ask about the same navigation.
    assert_eq!(
        requests.admit_navigation("browser-2", "javascript:fetch('/x')"),
        NavigationVerdict::Deny
    );
    for refused in [
        "data:text/html,x",
        "file:///x",
        "blob:https://a/b",
        "sage://x",
    ] {
        let child = requests
            .reserve_native(&tabs, "browser-1", "about:blank")
            .unwrap();
        assert!(requests.native_opened(child.id, "browser-3", t0()));
        assert_eq!(
            requests.admit_navigation("browser-3", refused),
            NavigationVerdict::Abandon,
            "{refused}"
        );
        requests.forget_child("browser-3");
    }
    let blank = requests
        .reserve_native(&tabs, "browser-1", "about:blank")
        .unwrap();
    assert!(requests.native_opened(blank.id, "browser-4", t0()));
    assert_eq!(
        requests.admit_navigation("browser-4", "https://example.com/"),
        NavigationVerdict::Admit
    );
    assert_eq!(
        requests.admit_navigation("browser-4", "javascript:void(0)"),
        NavigationVerdict::Deny
    );
    assert_eq!(
        requests.admit_navigation("browser-1", "javascript:void(0)"),
        NavigationVerdict::Deny
    );
}
