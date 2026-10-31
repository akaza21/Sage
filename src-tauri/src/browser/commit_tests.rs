use super::super::{BrowserTabs, browsing_history::BrowsingHistory, navigation};

fn recorded_tab() -> (BrowserTabs<usize>, BrowsingHistory) {
    let history = BrowsingHistory::in_memory();
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.browsing_history = history.clone();
    tabs.insert("browser-1".into(), 1, true);
    (tabs, history)
}

fn visits(history: &BrowsingHistory) -> Vec<(String, Option<String>)> {
    history
        .page(String::new(), None, 100)
        .unwrap()
        .visits
        .into_iter()
        .rev()
        .map(|visit| (visit.url, visit.title))
        .collect()
}

fn visit(url: &str, title: Option<&str>) -> (String, Option<String>) {
    (url.to_owned(), title.map(str::to_owned))
}

#[test]
fn each_newly_committed_page_is_one_visit_named_by_its_title() {
    let (mut tabs, history) = recorded_tab();
    assert!(tabs.start_navigation("browser-1", 1, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://a.example/")));
    assert!(tabs.set_document_title("browser-1", "https://a.example/", "Alpha"));
    // A reload commits the same URL again: not a new visit.
    assert!(tabs.start_navigation("browser-1", 2, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 2, Some("https://a.example/")));
    assert!(tabs.set_document_title("browser-1", "https://a.example/", "Alpha"));
    assert!(tabs.start_navigation("browser-1", 3, "https://b.example/"));
    assert!(tabs.finish_navigation("browser-1", 3, Some("https://b.example/")));
    assert_eq!(
        visits(&history),
        [
            visit("https://a.example/", Some("Alpha")),
            visit("https://b.example/", None),
        ]
    );
}

#[test]
fn same_document_changes_are_visits_and_back_lands_as_a_visit() {
    let (mut tabs, history) = recorded_tab();
    assert!(tabs.start_navigation("browser-1", 1, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://a.example/")));
    assert!(tabs.source_changed("browser-1", "https://a.example/#two"));
    assert_eq!(
        tabs.traverse("browser-1", -1).unwrap(),
        "https://a.example/"
    );
    assert!(tabs.source_changed("browser-1", "https://a.example/"));
    assert_eq!(
        visits(&history),
        [
            visit("https://a.example/", None),
            visit("https://a.example/#two", None),
            visit("https://a.example/", None),
        ]
    );
}

#[test]
fn failed_cancelled_and_refused_navigations_record_nothing() {
    let (mut tabs, history) = recorded_tab();
    assert!(tabs.start_navigation("browser-1", 1, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 1, None));
    assert!(tabs.start_navigation("browser-1", 2, "https://b.example/"));
    assert!(!tabs.finish_navigation("browser-1", 2, Some("file:///C:/secret")));
    assert_eq!(
        tabs.settle_navigation(
            "browser-1",
            2,
            navigation::outcome::Completion {
                success: false,
                web_error: navigation::outcome::WEB_ERROR_OPERATION_CANCELED,
                source: None,
            }
        ),
        Some(navigation::outcome::Settled::Abandoned)
    );
    assert!(visits(&history).is_empty());
}

#[test]
fn a_same_document_change_adopted_when_a_navigation_is_abandoned_is_a_visit() {
    let (mut tabs, history) = recorded_tab();
    assert!(tabs.start_navigation("browser-1", 1, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://a.example/")));
    assert!(tabs.start_navigation("browser-1", 2, "https://a.example/file.bin"));
    assert_eq!(
        tabs.settle_navigation(
            "browser-1",
            2,
            navigation::outcome::Completion {
                success: false,
                web_error: navigation::outcome::WEB_ERROR_OPERATION_CANCELED,
                source: Some("https://a.example/#moved"),
            }
        ),
        Some(navigation::outcome::Settled::Abandoned)
    );
    assert_eq!(
        visits(&history),
        [
            visit("https://a.example/", None),
            visit("https://a.example/#moved", None),
        ]
    );
}
