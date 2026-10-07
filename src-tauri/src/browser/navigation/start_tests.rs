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
