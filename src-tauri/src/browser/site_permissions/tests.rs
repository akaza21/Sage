use super::*;

#[test]
fn default_is_a_visible_prompt_and_decisions_are_tab_and_origin_scoped() {
    let mut permissions = SitePermissions::default();
    let RequestOutcome::Prompt(prompt) = permissions.request(
        "browser-1",
        "https://a.example/path?secret=1",
        PermissionKind::Camera,
        false,
    ) else {
        panic!("unresolved user permission should prompt");
    };
    assert_eq!(prompt.origin, "https://a.example");
    assert!(!permissions.resolve(prompt.id, "browser-2", Decision::Allow));
    assert!(permissions.resolve(prompt.id, "browser-1", Decision::Allow));
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://a.example/other",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Allow
    ));
    assert!(matches!(
        permissions.request(
            "browser-2",
            "https://a.example/other",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Prompt(_)
    ));
    assert!(matches!(
        permissions.request(
            "browser-2",
            "https://b.example/",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Prompt(_)
    ));
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://a.example/",
            PermissionKind::Microphone,
            false
        ),
        RequestOutcome::Prompt(_)
    ));
}

#[test]
fn agent_and_invalid_sources_are_denied_without_a_prompt() {
    let mut permissions = SitePermissions::default();
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://a.example/",
            PermissionKind::Camera,
            true
        ),
        RequestOutcome::Deny
    ));
    assert!(matches!(
        permissions.request(
            "browser-1",
            "file:///private",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Deny
    ));
    assert!(permissions.pending("browser-1").is_empty());
}

#[test]
fn revoke_and_close_remove_only_target_scope() {
    let mut permissions = SitePermissions::default();
    let RequestOutcome::Prompt(first) = permissions.request(
        "browser-1",
        "https://a.example/",
        PermissionKind::Camera,
        false,
    ) else {
        panic!()
    };
    let RequestOutcome::Prompt(other) = permissions.request(
        "browser-2",
        "https://b.example/",
        PermissionKind::Camera,
        false,
    ) else {
        panic!()
    };
    assert_eq!(permissions.cancel_tab("browser-1"), vec![first.id]);
    assert_eq!(permissions.pending("browser-2")[0].id, other.id);
    assert!(permissions.resolve(other.id, "browser-2", Decision::Allow));
    assert!(!permissions.revoke("browser-1", "https://b.example", PermissionKind::Camera));
    assert_eq!(permissions.decisions("browser-2").len(), 1);
    assert!(permissions.revoke("browser-2", "https://b.example", PermissionKind::Camera));
    assert!(permissions.decisions("browser-2").is_empty());
    let RequestOutcome::Prompt(reopened) = permissions.request(
        "browser-2",
        "https://b.example/",
        PermissionKind::Camera,
        false,
    ) else {
        panic!()
    };
    assert!(permissions.resolve(reopened.id, "browser-2", Decision::Allow));
    permissions.cancel_tab("browser-2");
    assert!(permissions.decisions("browser-2").is_empty());
    assert!(matches!(
        permissions.request(
            "browser-2",
            "https://b.example/",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Deny
    ));
    permissions.finish_close("browser-2");
    assert!(!permissions.closing_tabs.contains("browser-2"));
}

#[test]
fn duplicate_same_scope_requests_share_one_approvable_prompt() {
    let mut permissions = SitePermissions::default();
    let RequestOutcome::Prompt(first) = permissions.request(
        "browser-1",
        "https://a.example/one",
        PermissionKind::Camera,
        false,
    ) else {
        panic!("first request should prompt")
    };
    let RequestOutcome::Prompt(second) = permissions.request(
        "browser-1",
        "https://a.example/two",
        PermissionKind::Camera,
        false,
    ) else {
        panic!("same-scope request should attach to the prompt")
    };
    assert_eq!(second.id, first.id);
    assert_eq!(permissions.pending("browser-1").len(), 1);
    assert!(permissions.resolve(first.id, "browser-1", Decision::Allow));
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://a.example/three",
            PermissionKind::Camera,
            false,
        ),
        RequestOutcome::Allow
    ));
}

#[test]
fn ninth_same_scope_permission_request_is_denied() {
    let mut permissions = SitePermissions::default();
    let mut first_id = None;
    for _ in 0..MAX_WAITERS_PER_PERMISSION {
        let RequestOutcome::Prompt(prompt) = permissions.request(
            "browser-1",
            "https://a.example/path",
            PermissionKind::ClipboardRead,
            false,
        ) else {
            panic!("first eight requests should share a prompt")
        };
        if let Some(id) = first_id {
            assert_eq!(prompt.id, id);
        } else {
            first_id = Some(prompt.id);
        }
    }
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://a.example/next",
            PermissionKind::ClipboardRead,
            false,
        ),
        RequestOutcome::Deny
    ));
    assert_eq!(permissions.pending("browser-1").len(), 1);
}

#[test]
fn cross_origin_navigation_revokes_pending_permission_before_it_can_be_allowed() {
    let mut permissions = SitePermissions::default();
    let RequestOutcome::Prompt(prompt) = permissions.request(
        "browser-1",
        "https://a.example/page",
        PermissionKind::Geolocation,
        false,
    ) else {
        panic!("initial origin should prompt")
    };
    assert_eq!(
        permissions.navigation_to("browser-1", "https://b.example/page"),
        vec![prompt.id]
    );
    assert!(!permissions.resolve(prompt.id, "browser-1", Decision::Allow));
}

#[test]
fn same_origin_navigation_preserves_the_pending_permission() {
    let mut permissions = SitePermissions::default();
    let RequestOutcome::Prompt(prompt) = permissions.request(
        "browser-1",
        "https://a.example/one",
        PermissionKind::Camera,
        false,
    ) else {
        panic!("initial origin should prompt")
    };
    assert!(
        permissions
            .navigation_to("browser-1", "https://a.example/two")
            .is_empty()
    );
    assert!(permissions.resolve(prompt.id, "browser-1", Decision::Allow));
}

#[test]
fn redirect_to_another_origin_revokes_the_old_prompt() {
    let mut permissions = SitePermissions::default();
    let RequestOutcome::Prompt(prompt) = permissions.request(
        "browser-1",
        "https://a.example/one",
        PermissionKind::Notifications,
        false,
    ) else {
        panic!("initial origin should prompt")
    };
    assert!(
        permissions
            .navigation_to("browser-1", "https://a.example/two")
            .is_empty()
    );
    assert_eq!(
        permissions.navigation_settled("browser-1", Some("https://b.example/redirect")),
        vec![prompt.id]
    );
    assert!(!permissions.resolve(prompt.id, "browser-1", Decision::Allow));
}
