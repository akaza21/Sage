use super::*;

use crate::{domain::ProjectLayoutRequest, persistence::Database};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(1);

#[test]
fn document_title_is_bounded_and_tied_to_the_committed_page() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
    assert!(tabs.set_document_title("browser-1", "https://example.com/", "Example\0\n Domain"));
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().title.as_deref(),
        Some("Example Domain")
    );
    let before_title_change = tabs.snapshot("browser-1").unwrap().revision;
    assert!(tabs.set_document_title(
        "browser-1",
        "https://example.com/",
        "Safe\u{061c}\u{202e}evil\u{202c}\u{2066}Title\u{2069}"
    ));
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().title.as_deref(),
        Some("SafeevilTitle")
    );
    assert!(tabs.snapshot("browser-1").unwrap().revision > before_title_change);
    assert!(!tabs.set_document_title("browser-1", "https://old.example/", "Old"));
    assert!(tabs.start_navigation("browser-1", 2, "https://next.example/"));
    assert_eq!(tabs.snapshot("browser-1").unwrap().title, None);
    assert!(!tabs.set_document_title("browser-1", "https://example.com/", "Late"));
    assert!(tabs.finish_navigation("browser-1", 2, Some("https://next.example/")));
    assert!(tabs.set_document_title("browser-1", "https://next.example/", &"x".repeat(200)));
    assert_eq!(
        tabs.snapshot("browser-1")
            .unwrap()
            .title
            .unwrap()
            .chars()
            .count(),
        80
    );
}

#[test]
fn user_screenshot_admits_only_the_settled_selected_revision() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 7, false);
    assert!(super::user_screenshot::capture_target(&tabs, "browser-1", "0").is_err());
    assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
    assert!(super::user_screenshot::capture_target(&tabs, "browser-1", "1").is_err());
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
    let revision = tabs.snapshot("browser-1").unwrap().revision.to_string();
    assert_eq!(
        super::user_screenshot::capture_target(&tabs, "browser-1", &revision).unwrap(),
        7
    );
    assert!(super::user_screenshot::capture_target(&tabs, "browser-1", "0").is_err());
    assert!(super::user_screenshot::capture_target(&tabs, "browser-01", &revision).is_err());
    assert!(super::user_screenshot::capture_target(&tabs, "browser-2", &revision).is_err());
}

#[test]
fn startup_label_skips_a_browser_owned_by_another_project() {
    let base = std::env::temp_dir().join(format!(
        "sage-browser-label-projects-{}-{}",
        std::process::id(),
        NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&base).unwrap();
    let first_root = base.join("first");
    let second_root = base.join("second");
    fs::create_dir(&first_root).unwrap();
    fs::create_dir(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let first = database.create_project_from_root(&first_root).unwrap();
    let mut tabs = database.bootstrap_snapshot().unwrap().tabs;
    tabs.push(crate::domain::WorkbenchTab {
        id: "browser-1".into(),
        kind: "browser".into(),
        title: "Browser".into(),
        position: tabs.len() as i64,
        pinned: false,
        dirty: false,
        status: None,
        updated_at: None,
        resource: None,
        repository: None,
        diff_ref: None,
    });
    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: first.id,
            tabs,
            active_tab_id: Some("browser-1".into()),
            selected_root_id: Some(first.roots[0].id.clone()),
            selected_resource: None,
        })
        .unwrap();
    let second = database.create_project_from_root(&second_root).unwrap();
    assert_eq!(
        database.repair_active_project().unwrap().as_deref(),
        Some(second.id.as_str())
    );

    let mut native = BrowserTabs::<usize>::new();
    native.next_number = database.next_browser_tab_number().unwrap();
    let next_id = native.reserve_label().unwrap();
    assert_eq!(next_id, "browser-2");
    let mut second_tabs = database.bootstrap_snapshot().unwrap().tabs;
    second_tabs.push(crate::domain::WorkbenchTab {
        id: next_id.clone(),
        kind: "browser".into(),
        title: "Browser".into(),
        position: second_tabs.len() as i64,
        pinned: false,
        dirty: false,
        status: None,
        updated_at: None,
        resource: None,
        repository: None,
        diff_ref: None,
    });
    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: second.id,
            tabs: second_tabs,
            active_tab_id: Some(next_id),
            selected_root_id: Some(second.roots[0].id.clone()),
            selected_resource: None,
        })
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE workbench_tabs SET id='browser-0' WHERE id='browser-1'",
            [],
        )
        .unwrap();
    assert!(database.next_browser_tab_number().is_err());
    database
        .connection()
        .execute(
            "UPDATE workbench_tabs SET id='browser-18446744073709551615' WHERE id='browser-0'",
            [],
        )
        .unwrap();
    assert!(database.next_browser_tab_number().is_err());
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn normal_launch_rejects_active_project_containing_browser_profile() {
    let root = std::env::temp_dir().join(format!(
        "sage-browser-normal-overlap-{}-{}",
        std::process::id(),
        NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let ui = root.join("ui");
    let durable = root.join("durable");
    fs::create_dir(&ui).unwrap();
    fs::create_dir(&durable).unwrap();
    let profile = LaunchProfile::Normal {
        durable_dir: durable,
    };
    assert!(BrowserState::new(&profile, ui, std::slice::from_ref(&root), 1).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rapid_close_and_reopen_never_reuses_a_tab_label() {
    let mut tabs = BrowserTabs::<usize>::new();
    let first = tabs.reserve_label().unwrap();
    tabs.insert(first.clone(), 1, true);
    tabs.close_with(&first, |_| Ok(())).unwrap();
    let second = tabs.reserve_label().unwrap();
    tabs.insert(second.clone(), 2, true);
    assert_ne!(first, second);
    assert!(!tabs.contains(&first));
    assert!(!tabs.start_navigation(&first, 1, "https://example.com/"));
    assert!(tabs.contains(&second));
}

#[test]
fn foreign_browser_labels_cannot_read_or_mutate_an_owned_tab() {
    let mut tabs = BrowserTabs::<usize>::new();
    let label = tabs.reserve_label().unwrap();
    tabs.insert(label.clone(), 1, false);
    assert!(tabs.handle("browser-foreign").is_none());
    assert!(tabs.snapshot("browser-foreign").is_none());
    assert!(!tabs.set_zoom_percent("browser-foreign", 200));
    assert!(!tabs.start_navigation("browser-foreign", 1, "https://example.com/"));
    assert!(tabs.traverse("browser-foreign", -1).is_err());
    assert!(
        tabs.close_with("browser-foreign", |_| panic!("foreign close invoked"))
            .is_err()
    );
    assert!(tabs.contains(&label));
}

#[test]
fn failed_native_close_keeps_the_tab_available_for_retry() {
    let mut tabs = BrowserTabs::<usize>::new();
    let label = tabs.reserve_label().unwrap();
    tabs.insert(label.clone(), 7, false);
    assert!(
        tabs.close_with(&label, |_| Err("close failed".into()))
            .is_err()
    );
    assert!(tabs.contains(&label));
    tabs.close_with(&label, |_| Ok(())).unwrap();
    assert!(!tabs.contains(&label));
}

#[test]
fn late_completion_cannot_publish_a_provisional_url() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, true);
    assert!(tabs.start_navigation("browser-1", 10, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 10, Some("https://a.example/")));
    assert!(tabs.start_navigation("browser-1", 11, "https://b.example/"));
    assert!(!tabs.finish_navigation("browser-1", 10, Some("https://b.example/")));
    let loading = tabs.snapshot("browser-1").unwrap();
    assert!(loading.loading);
    assert_eq!(loading.committed_url.as_deref(), Some("https://a.example/"));
    assert!(tabs.openable_url("browser-1").is_err());
    assert!(tabs.finish_navigation("browser-1", 11, Some("https://b.example/")));
    assert_eq!(
        tabs.openable_url("browser-1").unwrap(),
        "https://b.example/"
    );
}

#[test]
fn committed_history_is_bounded_and_traversal_does_not_append() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, true);
    assert!(tabs.start_navigation("browser-1", 1, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://a.example/")));
    for id in 2..=MAX_BROWSER_HISTORY as u64 + 1 {
        assert!(tabs.start_navigation("browser-1", id, "https://b.example/"));
        assert!(tabs.finish_navigation(
            "browser-1",
            id,
            Some(&format!("https://example.com/{id}"))
        ));
    }
    assert_eq!(tabs.history_len("browser-1"), Some(MAX_BROWSER_HISTORY));
    let target = tabs.traverse("browser-1", -1).unwrap();
    assert_eq!(
        target,
        format!("https://example.com/{}", MAX_BROWSER_HISTORY)
    );
    let id = MAX_BROWSER_HISTORY as u64 + 2;
    assert!(tabs.start_navigation("browser-1", id, &target));
    assert!(tabs.finish_navigation("browser-1", id, Some(&target)));
    assert_eq!(tabs.history_len("browser-1"), Some(MAX_BROWSER_HISTORY));
    assert!(tabs.snapshot("browser-1").unwrap().can_go_forward);
    let id = id + 1;
    assert!(tabs.start_navigation("browser-1", id, "https://new.example/"));
    assert!(tabs.finish_navigation("browser-1", id, Some("https://new.example/")));
    assert!(!tabs.snapshot("browser-1").unwrap().can_go_forward);
    assert_eq!(tabs.history_len("browser-1"), Some(MAX_BROWSER_HISTORY));
    tabs.close_with("browser-1", |_| Ok(())).unwrap();
    assert_eq!(tabs.history_len("browser-1"), None);
}

#[test]
fn same_document_source_updates_only_a_settled_admitted_tab() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, true);
    assert!(tabs.start_navigation("browser-1", 1, "https://example.com/page"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/page")));
    assert!(tabs.source_changed("browser-1", "https://example.com/page#section"));
    assert_eq!(
        tabs.openable_url("browser-1").unwrap(),
        "https://example.com/page#section"
    );
    assert!(!tabs.source_changed("browser-1", "file:///C:/secret"));
    assert!(tabs.start_navigation("browser-1", 2, "https://next.example/"));
    assert!(!tabs.source_changed("browser-1", "https://example.com/page#late"));
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().committed_url.as_deref(),
        Some("https://example.com/page#section")
    );
}

#[test]
fn initial_page_cannot_commit_before_matching_native_navigation() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, true);
    assert!(!tabs.finish_navigation("browser-1", 7, Some("https://redirect.example/")));
    assert!(tabs.snapshot("browser-1").unwrap().loading);
    assert!(tabs.snapshot("browser-1").unwrap().committed_url.is_none());
    assert!(tabs.start_navigation("browser-1", 8, "https://redirect.example/"));
    assert!(!tabs.finish_navigation("browser-1", 7, Some("https://redirect.example/")));
    assert!(tabs.finish_navigation("browser-1", 8, Some("https://redirect.example/")));
    assert_eq!(
        tabs.openable_url("browser-1").unwrap(),
        "https://redirect.example/"
    );
}

#[test]
fn ninth_browser_tab_is_refused_and_close_releases_capacity() {
    let mut tabs = BrowserTabs::<usize>::new();
    let mut labels = Vec::new();
    for handle in 0..MAX_BROWSER_TABS {
        let label = tabs.reserve_label().unwrap();
        tabs.insert(label.clone(), handle, true);
        labels.push(label);
    }
    assert!(tabs.reserve_label().is_err());
    tabs.close_with(&labels[0], |_| Ok(())).unwrap();
    assert!(tabs.reserve_label().is_ok());
}

#[test]
fn popup_requests_are_denied_unless_url_and_capacity_are_admitted() {
    let mut tabs = BrowserTabs::<usize>::new();
    let opener = tabs.reserve_label().unwrap();
    tabs.insert(opener.clone(), 0, false);
    assert_eq!(
        popup_target(
            &tabs,
            &opener,
            &Url::parse("https://example.com/next").unwrap()
        )
        .as_deref(),
        Some("https://example.com/next")
    );
    for input in [
        "file:///C:/secret",
        "data:text/html,secret",
        "https://user:pass@example.com/",
    ] {
        assert!(popup_target(&tabs, &opener, &Url::parse(input).unwrap()).is_none());
    }
    assert!(
        popup_target(
            &tabs,
            "closed-tab",
            &Url::parse("https://example.com/").unwrap()
        )
        .is_none()
    );
    for handle in 1..MAX_BROWSER_TABS {
        let label = tabs.reserve_label().unwrap();
        tabs.insert(label, handle, true);
    }
    assert!(
        popup_target(
            &tabs,
            &opener,
            &Url::parse("https://example.com/next").unwrap()
        )
        .is_none()
    );
}

#[test]
fn failed_history_retry_stops_loading_without_publishing_wrong_origin() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, true);
    for (id, url) in [
        (1, "https://a.example/"),
        (2, "https://b.example/"),
        (3, "https://c.example/"),
    ] {
        assert!(tabs.start_navigation("browser-1", id, url));
        assert!(tabs.finish_navigation("browser-1", id, Some(url)));
    }
    assert_eq!(
        tabs.traverse("browser-1", -1).unwrap(),
        "https://b.example/"
    );
    assert!(tabs.start_navigation("browser-1", 4, "https://unexpected.example/"));
    assert!(tabs.finish_navigation("browser-1", 4, Some("https://unexpected.example/")));
    assert_eq!(
        tabs.take_fallback("browser-1").as_deref(),
        Some("https://b.example/")
    );
    assert!(tabs.start_navigation("browser-1", 5, "https://b.example/"));
    assert!(tabs.finish_navigation("browser-1", 5, Some("https://redirect.example/")));
    let snapshot = tabs.snapshot("browser-1").unwrap();
    assert!(!snapshot.loading);
    assert!(snapshot.committed_url.is_none());
    assert!(snapshot.error.is_some());
    assert!(tabs.take_fallback("browser-1").is_none());
    assert_eq!(
        tabs.entries["browser-1"].committed_url.as_deref(),
        Some("https://c.example/")
    );
    assert!(tabs.openable_url("browser-1").is_err());
}

#[test]
fn only_bounded_http_urls_without_credentials_are_admitted() {
    assert_eq!(
        admit_url("https://example.com/path").unwrap().as_str(),
        "https://example.com/path"
    );
    for input in [
        "file:///C:/secret",
        "data:text/html,hello",
        "javascript:alert(1)",
        "https://user:pass@example.com/",
        "https://user@example.com/",
        "https://example.com/\nabout:blank",
        "https://example.com/\u{7f}",
        "about:blank",
        "https://example.com/\u{0}",
    ] {
        assert!(admit_url(input).is_err(), "admitted {input:?}");
    }
    assert!(
        admit_url(&format!(
            "https://example.com/{}",
            "x".repeat(MAX_BROWSER_URL_BYTES)
        ))
        .is_err()
    );
}

fn failed_completion(web_error: i32, source: &str) -> navigation::outcome::Completion<'_> {
    navigation::outcome::Completion {
        success: false,
        web_error,
        source: Some(source),
    }
}

fn committed_tab(urls: &[&str]) -> BrowserTabs<usize> {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, true);
    for (id, url) in urls.iter().enumerate() {
        assert!(tabs.start_navigation("browser-1", id as u64 + 1, url));
        assert!(tabs.finish_navigation("browser-1", id as u64 + 1, Some(url)));
    }
    tabs
}

#[test]
fn navigation_that_became_a_download_keeps_the_previous_page() {
    let mut tabs = committed_tab(&["https://a.example/", "https://a.example/files"]);
    assert!(tabs.set_document_title("browser-1", "https://a.example/files", "Files"));
    tabs.entries.get_mut("browser-1").unwrap().favicon = Some("data:image/png;base64,AA".into());
    let before = tabs.snapshot("browser-1").unwrap();
    // A superseding start while loading must not lose the remembered page.
    assert!(tabs.start_navigation("browser-1", 3, "https://a.example/report.pdf"));
    assert!(tabs.start_navigation("browser-1", 4, "https://a.example/report.pdf"));
    tabs.note_download("browser-1", "https://a.example/report.pdf");
    // WebView2 reports the download's navigation as unsuccessful while it
    // still shows the page it left.
    assert!(
        tabs.settle_navigation(
            "browser-1",
            4,
            failed_completion(9, "https://a.example/files")
        )
        .is_some()
    );
    let after = tabs.snapshot("browser-1").unwrap();
    assert!(!after.loading);
    assert_eq!(after.error, None);
    assert_eq!(
        after.committed_url.as_deref(),
        Some("https://a.example/files")
    );
    assert_eq!(after.title.as_deref(), Some("Files"));
    assert_eq!(after.favicon, before.favicon);
    assert_eq!(
        (after.can_go_back, after.can_go_forward),
        (before.can_go_back, before.can_go_forward)
    );
    assert!(after.revision > before.revision);
    assert_eq!(tabs.history_len("browser-1"), Some(2));
    assert_eq!(
        tabs.openable_url("browser-1").unwrap(),
        "https://a.example/files"
    );
    assert!(tabs.take_fallback("browser-1").is_none());
}

#[test]
fn denied_or_cancelled_navigation_keeps_the_page() {
    // An Agent download is denied in `begin()` and cancelled; a blocked or
    // stopped navigation is cancelled. Either way nothing commits.
    let mut tabs = committed_tab(&["https://a.example/", "https://b.example/"]);
    assert!(tabs.start_navigation("browser-1", 3, "https://b.example/file.bin"));
    assert_eq!(
        tabs.settle_navigation(
            "browser-1",
            3,
            failed_completion(
                navigation::outcome::WEB_ERROR_OPERATION_CANCELED,
                "https://b.example/"
            )
        ),
        Some(navigation::outcome::Settled::Abandoned)
    );
    let after = tabs.snapshot("browser-1").unwrap();
    assert_eq!(after.committed_url.as_deref(), Some("https://b.example/"));
    assert!(!after.loading && after.error.is_none() && after.can_go_back);
    assert_eq!(tabs.history_len("browser-1"), Some(2));
}

#[test]
fn abandoned_back_navigation_keeps_the_cursor_and_allows_another_traversal() {
    let mut tabs = committed_tab(&["https://a.example/", "https://b.example/"]);
    assert_eq!(
        tabs.traverse("browser-1", -1).unwrap(),
        "https://a.example/"
    );
    assert!(tabs.start_navigation("browser-1", 3, "https://a.example/"));
    assert!(
        tabs.settle_navigation(
            "browser-1",
            3,
            failed_completion(
                navigation::outcome::WEB_ERROR_OPERATION_CANCELED,
                "https://b.example/"
            )
        )
        .is_some()
    );
    assert!(tabs.take_fallback("browser-1").is_none());
    let after = tabs.snapshot("browser-1").unwrap();
    assert_eq!(after.committed_url.as_deref(), Some("https://b.example/"));
    assert!(!after.loading && after.error.is_none());
    assert!(after.can_go_back && !after.can_go_forward);
    assert_eq!(
        tabs.traverse("browser-1", -1).unwrap(),
        "https://a.example/"
    );
}

#[test]
fn real_load_failure_is_reported_instead_of_a_blank_new_tab() {
    let mut tabs = committed_tab(&["https://a.example/"]);
    assert!(tabs.start_navigation("browser-1", 2, "https://missing.example/"));
    assert_eq!(
        tabs.settle_navigation(
            "browser-1",
            2,
            failed_completion(13, "https://missing.example/")
        ),
        Some(navigation::outcome::Settled::Failed)
    );
    let after = tabs.snapshot("browser-1").unwrap();
    assert!(!after.loading);
    assert_eq!(after.error, Some("The site's address could not be found"));
    assert!(tabs.openable_url("browser-1").is_err());
    // The next navigation clears the error as before.
    assert!(tabs.start_navigation("browser-1", 3, "https://a.example/"));
    assert!(tabs.finish_navigation("browser-1", 3, Some("https://a.example/")));
    assert_eq!(tabs.snapshot("browser-1").unwrap().error, None);
}

const CANCELED: i32 = navigation::outcome::WEB_ERROR_OPERATION_CANCELED;

#[test]
fn cancel_after_hash_drift_is_abandoned_and_never_counts_as_a_commit() {
    // The page changes its hash while the user's navigation is pending;
    // `committed_url` does not follow it during the load.
    let mut tabs = committed_tab(&["https://a.example/page"]);
    assert!(tabs.start_navigation("browser-1", 2, "https://a.example/file.bin"));
    assert!(!tabs.source_changed("browser-1", "https://a.example/page#drift"));
    assert_eq!(
        tabs.settle_navigation(
            "browser-1",
            2,
            failed_completion(CANCELED, "https://a.example/page#drift")
        ),
        Some(navigation::outcome::Settled::Abandoned)
    );
    let after = tabs.snapshot("browser-1").unwrap();
    assert!(!after.loading && after.error.is_none());
    // What is on screen is adopted as a same-document change.
    assert_eq!(
        after.committed_url.as_deref(),
        Some("https://a.example/page#drift")
    );
    assert!(after.can_go_back);
}

#[test]
fn cancel_after_a_failed_load_keeps_reporting_the_failure() {
    let mut tabs = committed_tab(&["https://a.example/"]);
    assert!(tabs.start_navigation("browser-1", 2, "https://missing.example/"));
    tabs.note_content_loading("browser-1", 2, true);
    assert!(
        tabs.settle_navigation(
            "browser-1",
            2,
            failed_completion(13, "https://missing.example/")
        )
        .is_some()
    );
    assert!(tabs.start_navigation("browser-1", 3, "https://c.example/"));
    assert_eq!(
        tabs.settle_navigation(
            "browser-1",
            3,
            failed_completion(CANCELED, "https://missing.example/")
        ),
        Some(navigation::outcome::Settled::Abandoned)
    );
    let after = tabs.snapshot("browser-1").unwrap();
    assert_eq!(after.error, Some("The site's address could not be found"));
    assert!(after.committed_url.is_none());
    assert!(tabs.openable_url("browser-1").is_err());
}

#[test]
fn unrelated_download_does_not_hide_a_failed_reload() {
    let mut tabs = committed_tab(&["https://a.example/"]);
    assert!(tabs.start_navigation("browser-1", 2, "https://a.example/"));
    // An `<a download>` / blob download that is not this navigation.
    tabs.note_download("browser-1", "blob:https://a.example/123");
    tabs.note_content_loading("browser-1", 2, true);
    assert_eq!(
        tabs.settle_navigation("browser-1", 2, failed_completion(11, "https://a.example/")),
        Some(navigation::outcome::Settled::Failed)
    );
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().error,
        Some("The site could not be reached")
    );
}

#[test]
fn redirected_navigation_that_became_a_download_keeps_the_page() {
    let mut tabs = committed_tab(&["https://a.example/"]);
    assert!(tabs.start_navigation("browser-1", 2, "https://a.example/get"));
    tabs.note_redirect("browser-1", 2, "https://cdn.example/file.zip");
    tabs.note_download("browser-1", "https://cdn.example/file.zip");
    assert_eq!(
        tabs.settle_navigation("browser-1", 2, failed_completion(9, "https://a.example/")),
        Some(navigation::outcome::Settled::Abandoned)
    );
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().committed_url.as_deref(),
        Some("https://a.example/")
    );
}

#[test]
fn committed_error_documents_and_auth_pages_keep_their_url() {
    for web_error in [0, 17, 18] {
        let mut tabs = committed_tab(&["https://a.example/"]);
        assert!(tabs.start_navigation("browser-1", 2, "https://b.example/private"));
        tabs.note_content_loading("browser-1", 2, false);
        assert_eq!(
            tabs.settle_navigation(
                "browser-1",
                2,
                failed_completion(web_error, "https://b.example/private")
            ),
            Some(navigation::outcome::Settled::Committed),
            "status {web_error}"
        );
        let after = tabs.snapshot("browser-1").unwrap();
        assert_eq!(after.error, None);
        assert_eq!(
            after.committed_url.as_deref(),
            Some("https://b.example/private")
        );
    }
}

#[test]
fn stopped_reload_that_already_committed_does_not_restore_the_old_title() {
    let mut tabs = committed_tab(&["https://a.example/"]);
    assert!(tabs.set_document_title("browser-1", "https://a.example/", "Old"));
    assert!(tabs.start_navigation("browser-1", 2, "https://a.example/"));
    tabs.note_content_loading("browser-1", 2, false);
    assert_eq!(
        tabs.settle_navigation(
            "browser-1",
            2,
            failed_completion(CANCELED, "https://a.example/")
        ),
        Some(navigation::outcome::Settled::Committed)
    );
    let after = tabs.snapshot("browser-1").unwrap();
    assert_eq!(after.title, None);
    assert_eq!(after.committed_url.as_deref(), Some("https://a.example/"));
}
