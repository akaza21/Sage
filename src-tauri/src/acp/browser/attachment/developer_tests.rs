use super::*;
use crate::acp::browser::BrowserScope;

const SESSION: &str = "session-1";
const ORIGIN: &str = "https://example.com";

fn ids() -> (SageProjectId, SageChatId) {
    (
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
    )
}

fn scope(session: &str) -> BrowserScope {
    BrowserScope {
        session_id: session.into(),
        tab_id: 7,
    }
}

/// A user attachment on browser-7 whose authority `SESSION` holds.
fn attached() -> (BrowserAttachments, AttachmentLease) {
    let registry = BrowserAttachments::default();
    let (project, chat) = ids();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-7",
            "https://example.com/a",
            1,
        )
        .unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    lease.authority_for(&scope(SESSION)).unwrap();
    (registry, lease)
}

fn granted(registry: &BrowserAttachments) -> bool {
    let (project, chat) = ids();
    registry
        .resolve(&project, &chat)
        .is_some_and(|lease| lease.developer_granted(SESSION, ORIGIN))
}

#[test]
fn a_grant_covers_its_session_and_origin_only_and_shows_in_the_chrome() {
    let (registry, lease) = attached();
    assert!(!lease.developer_granted(SESSION, ORIGIN));
    assert!(!registry.agent_control("browser-7").unwrap().developer_mode);
    assert!(!lease.grant_developer_mode(SESSION, "https://other.example"));
    assert!(!lease.grant_developer_mode("session-2", ORIGIN));
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    assert!(lease.developer_granted(SESSION, ORIGIN));
    assert!(!lease.developer_granted("session-2", ORIGIN));
    assert!(!lease.developer_granted(SESSION, "https://other.example"));
    assert!(registry.agent_control("browser-7").unwrap().developer_mode);
}

#[test]
fn take_over_ends_the_grant_and_hand_back_does_not_restore_it() {
    let (registry, lease) = attached();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    assert!(registry.take_over("browser-7"));
    assert!(!lease.developer_granted(SESSION, ORIGIN));
    assert!(
        !lease.grant_developer_mode(SESSION, ORIGIN),
        "no grant while the user holds the tab"
    );
    assert!(registry.hand_back("browser-7"));
    assert!(!granted(&registry));
    assert!(!registry.agent_control("browser-7").unwrap().developer_mode);
}

#[test]
fn stop_detach_and_tab_close_end_the_grant() {
    let (project, chat) = ids();
    for end in [
        |registry: &BrowserAttachments| registry.revoke_tab("browser-7"),
        |registry: &BrowserAttachments| {
            let (project, chat) = ids();
            registry.detach(&project, &chat);
        },
    ] {
        let (registry, lease) = attached();
        assert!(lease.grant_developer_mode(SESSION, ORIGIN));
        end(&registry);
        assert!(!lease.developer_granted(SESSION, ORIGIN));
        assert!(!lease.grant_developer_mode(SESSION, ORIGIN));
        assert!(registry.resolve(&project, &chat).is_none());
    }
}

#[test]
fn a_same_origin_commit_keeps_the_grant_and_a_cross_origin_one_ends_it() {
    let (registry, lease) = attached();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    registry.navigation_settled("browser-7", "https://example.com/b?reload", 2);
    assert!(
        granted(&registry),
        "a reload or in-site link keeps developer mode"
    );
    registry.navigation_settled("browser-7", "https://example.org/", 3);
    assert!(!granted(&registry));
    registry.navigation_settled("browser-7", "https://example.com/", 4);
    assert!(!granted(&registry), "coming back asks again");
}

#[test]
fn a_stale_lease_cannot_grant_after_a_navigation_retired_its_authority() {
    let (registry, stale) = attached();
    registry.navigation_settled("browser-7", "https://example.org/", 2);
    assert!(!stale.grant_developer_mode(SESSION, ORIGIN));
    assert!(!granted(&registry));
}

#[test]
fn another_session_taking_the_tab_ends_the_grant() {
    let (_registry, lease) = attached();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    lease.authority_for(&scope("session-2")).unwrap();
    assert!(!lease.developer_granted(SESSION, ORIGIN));
}

#[test]
fn the_settings_switch_going_off_ends_every_grant() {
    let (registry, lease) = attached();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    assert_eq!(registry.end_developer_mode(), vec!["browser-7".to_owned()]);
    assert!(!lease.developer_granted(SESSION, ORIGIN));
    assert!(registry.end_developer_mode().is_empty());
}

#[test]
fn a_grant_made_after_the_switch_changed_is_refused() {
    let (_registry, lease) = attached();
    // A call that started under another switch generation (no global bump
    // here, so parallel tests are unaffected).
    let stale = crate::acp::browser::developer::switch_generation().wrapping_add(1);
    assert!(!lease.grant_developer_mode_at(SESSION, ORIGIN, stale));
    assert!(!lease.developer_mode_active());
    let current = crate::acp::browser::developer::switch_generation();
    assert!(lease.grant_developer_mode_at(SESSION, ORIGIN, current));
}
