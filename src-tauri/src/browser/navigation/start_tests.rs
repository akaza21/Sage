use super::*;
use crate::browser::site_permissions::{Decision, PermissionKind, RequestOutcome};

#[test]
fn same_id_redirect_start_revokes_old_origin_permission_before_resolution() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut permissions = SitePermissions::default();
    let RequestOutcome::Prompt(prompt) = permissions.request(
        "browser-1",
        "https://a.example/page",
        PermissionKind::Camera,
        false,
    ) else {
        panic!("initial request should prompt")
    };
    assert!(
        begin(
            &mut tabs,
            &mut permissions,
            "browser-1",
            7,
            "https://a.example/page"
        )
        .0
    );
    let (started, denied) = begin(
        &mut tabs,
        &mut permissions,
        "browser-1",
        7,
        "https://b.example/redirected",
    );
    assert!(!started);
    assert_eq!(denied, vec![prompt.id]);
    assert!(!permissions.resolve(prompt.id, "browser-1", Decision::Allow));
}

#[test]
fn a_navigation_started_after_a_revocation_is_the_one_whose_commit_clears_it() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut permissions = SitePermissions::default();
    begin(
        &mut tabs,
        &mut permissions,
        "browser-1",
        3,
        "https://a.example/",
    );
    permissions.mark_stale(["browser-1"]);
    assert!(!permissions.document_committed("browser-1", 3));
    begin(
        &mut tabs,
        &mut permissions,
        "browser-1",
        4,
        "https://a.example/",
    );
    assert!(permissions.document_committed("browser-1", 4));
    assert!(!permissions.is_stale("browser-1"));
}

#[test]
fn a_redirect_of_a_navigation_started_before_a_revocation_does_not_clear_it() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let mut permissions = SitePermissions::default();
    begin(
        &mut tabs,
        &mut permissions,
        "browser-1",
        3,
        "https://a.example/",
    );
    permissions.mark_stale(["browser-1"]);
    // WebView2 repeats NavigationStarting with the same id on a redirect.
    let (started, _) = begin(
        &mut tabs,
        &mut permissions,
        "browser-1",
        3,
        "https://a.example/next",
    );
    assert!(!started);
    assert!(!permissions.document_committed("browser-1", 3));
    assert!(permissions.is_stale("browser-1"));
    begin(
        &mut tabs,
        &mut permissions,
        "browser-1",
        4,
        "https://a.example/",
    );
    begin(
        &mut tabs,
        &mut permissions,
        "browser-1",
        4,
        "https://a.example/next",
    );
    assert!(permissions.document_committed("browser-1", 4));
}
