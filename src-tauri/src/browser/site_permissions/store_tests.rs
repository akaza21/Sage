use super::*;

fn attached(store: &SettingsStore) -> SitePermissions {
    let mut permissions = SitePermissions::default();
    permissions.attach_store(store.clone()).unwrap();
    permissions
}

fn prompt(permissions: &mut SitePermissions, tab: &str, url: &str) -> PermissionPrompt {
    match permissions.request(tab, url, PermissionKind::Camera, false) {
        RequestOutcome::Prompt(prompt) => prompt,
        _ => panic!("expected a prompt for {url}"),
    }
}

#[test]
fn an_answer_survives_a_restart_and_applies_to_a_new_tab() {
    let store = SettingsStore::in_memory().unwrap();
    let mut permissions = attached(&store);
    let allowed = prompt(&mut permissions, "browser-1", "https://a.example/call");
    assert!(permissions.resolve(allowed.id, "browser-1", Decision::Allow));
    let blocked = prompt(&mut permissions, "browser-1", "https://b.example/");
    assert!(permissions.resolve(blocked.id, "browser-1", Decision::Deny));

    let mut restarted = attached(&store);
    assert!(matches!(
        restarted.request(
            "browser-9",
            "https://a.example/",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Allow
    ));
    assert!(matches!(
        restarted.request(
            "browser-9",
            "https://b.example/x",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Deny
    ));
    assert!(restarted.pending("browser-9").is_empty());
}

#[test]
fn a_saved_allow_never_reaches_an_agent_tab() {
    let store = SettingsStore::in_memory().unwrap();
    store
        .put_browser_site_permission("https://a.example", PermissionKind::Camera, Decision::Allow)
        .unwrap();
    let mut permissions = attached(&store);
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://a.example/",
            PermissionKind::Camera,
            true
        ),
        RequestOutcome::Deny
    ));
}

#[test]
fn settings_change_and_reset_write_through_to_the_store() {
    let store = SettingsStore::in_memory().unwrap();
    let mut permissions = attached(&store);
    assert!(
        permissions
            .set(
                "https://a.example",
                PermissionKind::Microphone,
                Decision::Allow
            )
            .is_err(),
        "Settings cannot create an answer the user never gave"
    );
    for (origin, kind) in [
        ("https://a.example", PermissionKind::Microphone),
        ("https://b.example:8443", PermissionKind::Geolocation),
    ] {
        let pending = match permissions.request("browser-1", origin, kind, false) {
            RequestOutcome::Prompt(prompt) => prompt,
            _ => panic!("expected a prompt"),
        };
        assert!(permissions.resolve(pending.id, "browser-1", Decision::Allow));
        permissions.navigation_settled("browser-1", None);
    }
    assert_eq!(
        permissions
            .set(
                "https://a.example",
                PermissionKind::Microphone,
                Decision::Deny
            )
            .unwrap(),
        Decision::Allow
    );
    assert_eq!(
        store.browser_site_permissions().unwrap(),
        vec![
            (
                "https://a.example".to_owned(),
                PermissionKind::Microphone,
                Decision::Deny
            ),
            (
                "https://b.example:8443".to_owned(),
                PermissionKind::Geolocation,
                Decision::Allow
            ),
        ]
    );
    assert_eq!(
        permissions
            .reset("https://a.example", PermissionKind::Microphone)
            .unwrap(),
        Some(Decision::Deny)
    );
    assert_eq!(
        permissions
            .reset("https://a.example", PermissionKind::Microphone)
            .unwrap(),
        None
    );
    assert_eq!(store.browser_site_permissions().unwrap().len(), 1);
    assert_eq!(
        permissions.reset_all().unwrap(),
        BTreeSet::from(["https://b.example:8443".to_owned()])
    );
    assert!(store.browser_site_permissions().unwrap().is_empty());
    assert!(permissions.decisions().is_empty());
}

#[test]
fn settings_admit_only_canonical_http_origins() {
    let mut permissions = attached(&SettingsStore::in_memory().unwrap());
    for origin in [
        "https://a.example/",
        "https://a.example/path",
        "HTTPS://A.example",
        "https://user:pass@a.example",
        "https://a.example:443",
        "file:///etc",
        "javascript:alert(1)",
        "ftp://a.example",
        "",
        "null",
    ] {
        assert!(
            permissions
                .set(origin, PermissionKind::Camera, Decision::Allow)
                .is_err(),
            "{origin}"
        );
        assert!(
            permissions.reset(origin, PermissionKind::Camera).is_err(),
            "{origin}"
        );
    }
    assert!(permissions.decisions().is_empty());
}

#[test]
fn a_full_store_denies_new_sites_without_a_prompt_but_keeps_known_ones() {
    let store = SettingsStore::in_memory().unwrap();
    for index in 0..MAX_SITE_PERMISSIONS {
        store
            .put_browser_site_permission(
                &format!("https://site{index}.example"),
                PermissionKind::Camera,
                Decision::Allow,
            )
            .unwrap();
    }
    let mut permissions = attached(&store);
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://new.example/",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Deny
    ));
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://site7.example/",
            PermissionKind::Camera,
            false
        ),
        RequestOutcome::Allow
    ));
    assert!(
        permissions
            .set(
                "https://new.example",
                PermissionKind::Camera,
                Decision::Deny
            )
            .is_err()
    );
    permissions
        .set(
            "https://site7.example",
            PermissionKind::Camera,
            Decision::Deny,
        )
        .unwrap();
}

#[test]
fn a_failed_save_leaves_the_prompt_pending_and_memory_unchanged() {
    let store = SettingsStore::in_memory().unwrap();
    let mut permissions = attached(&store);
    let pending = prompt(&mut permissions, "browser-1", "https://a.example/");
    store.break_browser_site_permissions_for_tests();
    assert!(!permissions.resolve(pending.id, "browser-1", Decision::Allow));
    assert!(permissions.decisions().is_empty());
    assert_eq!(permissions.pending("browser-1").len(), 1);
    assert!(
        permissions
            .set("https://a.example", PermissionKind::Camera, Decision::Allow)
            .is_err()
    );
    assert!(permissions.decisions().is_empty());
}

#[test]
fn a_saved_answer_settles_the_same_sites_prompts_in_other_tabs() {
    let mut permissions = attached(&SettingsStore::in_memory().unwrap());
    let first = prompt(&mut permissions, "browser-1", "https://a.example/");
    let second = prompt(&mut permissions, "browser-2", "https://a.example/other");
    let unrelated = prompt(&mut permissions, "browser-3", "https://b.example/");
    let mic = match permissions.request(
        "browser-2",
        "https://a.example/",
        PermissionKind::Microphone,
        false,
    ) {
        RequestOutcome::Prompt(prompt) => prompt,
        _ => panic!(),
    };
    assert_eq!(
        permissions.resolve_site(first.id, "browser-1", Decision::Allow),
        Some(vec![(second.id, "browser-2".to_owned())])
    );
    let left = [
        permissions.pending("browser-2"),
        permissions.pending("browser-3"),
    ]
    .concat();
    assert_eq!(
        left.iter().map(|prompt| prompt.id).collect::<Vec<_>>(),
        vec![second.id, mic.id, unrelated.id]
    );
    assert_eq!(
        permissions.resolve_site(first.id, "browser-1", Decision::Allow),
        None
    );
}

#[test]
fn rows_that_no_longer_admit_are_deleted_on_load() {
    let store = SettingsStore::in_memory().unwrap();
    store
        .put_browser_site_permission(
            "https://a.example/path",
            PermissionKind::Camera,
            Decision::Allow,
        )
        .unwrap();
    store
        .put_browser_site_permission("https://b.example", PermissionKind::Camera, Decision::Allow)
        .unwrap();
    let permissions = attached(&store);
    assert_eq!(store.browser_site_permissions().unwrap().len(), 1);
    assert_eq!(
        permissions.decisions(),
        vec![PermissionDecisionView {
            origin: "https://b.example".into(),
            kind: PermissionKind::Camera,
            decision: Decision::Allow,
        }]
    );
}

#[test]
fn a_stale_tab_clears_only_when_a_navigation_started_after_the_mark_commits() {
    let mut permissions = SitePermissions::default();
    permissions.mark_stale(["browser-1", "browser-2"]);
    assert!(permissions.is_stale("browser-1"));
    assert!(!permissions.document_committed("browser-3", 1));
    assert!(!permissions.document_committed("browser-1", 4));
    assert!(permissions.is_stale("browser-1"));
    permissions.navigation_started("browser-1", 5);
    assert!(!permissions.document_committed("browser-1", 4));
    assert!(permissions.document_committed("browser-1", 5));
    assert!(!permissions.is_stale("browser-1"));
    assert_eq!(
        permissions.stale_tabs(),
        BTreeSet::from(["browser-2".to_owned()])
    );
}

#[test]
fn only_the_latest_navigation_after_the_mark_clears_it() {
    let mut permissions = SitePermissions::default();
    permissions.mark_stale(["browser-1"]);
    permissions.navigation_started("browser-1", 5);
    permissions.navigation_started("browser-1", 6);
    assert!(!permissions.document_committed("browser-1", 5));
    assert!(permissions.is_stale("browser-1"));
    assert!(permissions.document_committed("browser-1", 6));
}

#[test]
fn a_second_revocation_discards_navigations_started_before_it() {
    let mut permissions = SitePermissions::default();
    permissions.mark_stale(["browser-1"]);
    permissions.navigation_started("browser-1", 5);
    permissions.mark_stale(["browser-1"]);
    assert!(!permissions.document_committed("browser-1", 5));
    assert!(permissions.is_stale("browser-1"));
}

#[test]
fn navigations_on_tabs_that_are_not_stale_mark_nothing() {
    let mut permissions = SitePermissions::default();
    permissions.navigation_started("browser-1", 5);
    assert!(!permissions.document_committed("browser-1", 5));
    assert!(permissions.stale_tabs().is_empty());
}

#[test]
fn a_closing_tab_stays_stale_until_its_native_close_succeeds() {
    let mut permissions = SitePermissions::default();
    permissions.mark_stale(["browser-1"]);
    permissions.cancel_tab("browser-1");
    assert!(permissions.is_stale("browser-1"));
    permissions.finish_close("browser-1");
    assert!(permissions.stale_tabs().is_empty());
}
