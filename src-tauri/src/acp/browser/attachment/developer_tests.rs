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

#[test]
fn full_revocation_releases_native_state_while_developer_off_preserves_ordinary_state() {
    for (end, full) in [
        (
            (|registry: &BrowserAttachments| {
                registry.take_over("browser-7");
            }) as fn(&BrowserAttachments),
            true,
        ),
        (
            (|registry: &BrowserAttachments| {
                registry.revoke_tab("browser-7");
            }) as fn(&BrowserAttachments),
            true,
        ),
        (
            (|registry: &BrowserAttachments| {
                registry.end_developer_mode();
            }) as fn(&BrowserAttachments),
            false,
        ),
    ] {
        let (registry, lease) = attached();
        assert!(lease.grant_developer_mode(SESSION, ORIGIN));
        let generation = lease.resource_generation();
        let grant_generation = lease.grant_generation();
        let buffer = Arc::new(vec![0u8; 1024]);
        let weak = Arc::downgrade(&buffer);
        lease
            .bind_resources(generation, || true, Box::new(move || drop(buffer)))
            .unwrap();
        end(&registry);
        assert_eq!(weak.upgrade().is_none(), full);
        assert!(!lease.developer_granted(SESSION, ORIGIN));
        assert!(!lease.grant_developer_mode_audited_at(
            SESSION,
            ORIGIN,
            crate::acp::browser::developer::switch_generation(),
            generation,
            grant_generation,
            Arc::new(|_| true)
        ));
        if full {
            let late = Arc::new(vec![0u8; 1024]);
            let weak = Arc::downgrade(&late);
            assert!(
                lease
                    .bind_resources(generation, || true, Box::new(move || drop(late)))
                    .is_err()
            );
            assert!(weak.upgrade().is_none());
        } else {
            assert_eq!(lease.resource_generation(), generation);
        }
    }
}

#[test]
fn a_revocation_between_card_checks_and_grant_publication_refuses_the_stale_card() {
    let (registry, lease) = attached();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    let switch = crate::acp::browser::developer::switch_generation();
    let resource = lease.resource_generation();
    let grant_generation = lease.grant_generation();
    registry.end_developer_mode();
    assert!(!lease.grant_developer_mode_audited_at(
        SESSION,
        ORIGIN,
        switch,
        resource,
        grant_generation,
        Arc::new(|_| true)
    ));
    assert!(!lease.developer_granted(SESSION, ORIGIN));
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
}

#[test]
fn a_cross_origin_commit_invalidates_a_pending_card_without_an_existing_grant() {
    let (registry, stale) = attached();
    let switch = crate::acp::browser::developer::switch_generation();
    let resource = stale.resource_generation();
    let grant_generation = stale.grant_generation();
    registry.navigation_settled("browser-7", "https://other.example/", 2);
    let (project, chat) = ids();
    registry
        .resolve(&project, &chat)
        .unwrap()
        .authority_for(&scope(SESSION))
        .unwrap();
    assert!(!stale.grant_developer_mode_audited_at(
        SESSION,
        ORIGIN,
        switch,
        resource,
        grant_generation,
        Arc::new(|_| true)
    ));
    registry.navigation_settled("browser-7", "https://example.com/", 3);
    assert!(!granted(&registry));
}

#[test]
fn delayed_revocation_cleanup_cannot_release_a_replacement_lane() {
    let (registry, lease) = attached();
    let old = lease.invalidate_resources();
    let buffer = Arc::new(vec![0u8; 1024]);
    let weak = Arc::downgrade(&buffer);
    lease
        .bind_resources(
            lease.resource_generation(),
            || true,
            Box::new(move || drop(buffer)),
        )
        .unwrap();
    lease.release_invalidated_resources(old);
    assert!(weak.upgrade().is_some());
    registry.revoke_tab("browser-7");
    assert!(weak.upgrade().is_none());
}

#[test]
fn revocation_releases_buffers_before_a_slow_audit_writer_finishes() {
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;
    let (registry, lease) = attached();
    let registry = Arc::new(registry);
    let (ready, entered) = sync_channel(1);
    let (resume, blocked) = sync_channel(1);
    let blocked = Mutex::new(blocked);
    let audit = Arc::new(move |outcome| {
        if outcome == crate::acp::browser::developer::CdpAuditOutcome::Revoked {
            ready.send(()).unwrap();
            blocked.lock().unwrap().recv().unwrap();
        }
        true
    });
    assert!(lease.grant_developer_mode_audited_at(
        SESSION,
        ORIGIN,
        crate::acp::browser::developer::switch_generation(),
        lease.resource_generation(),
        lease.grant_generation(),
        audit
    ));
    let buffer = Arc::new(vec![0u8; 1024]);
    let weak = Arc::downgrade(&buffer);
    lease
        .bind_resources(
            lease.resource_generation(),
            || true,
            Box::new(move || drop(buffer)),
        )
        .unwrap();
    let worker_registry = registry.clone();
    let worker = std::thread::spawn(move || worker_registry.revoke_tab("browser-7"));
    let started = entered.recv_timeout(Duration::from_secs(5)).is_ok();
    let released = weak.upgrade().is_none();
    resume.send(()).unwrap();
    worker.join().unwrap();
    assert!(started);
    assert!(
        released,
        "slow audit I/O must not retain revoked native state"
    );
}

#[test]
fn hiding_releases_diagnostics_after_the_active_operation_consumes_its_result() {
    let (registry, lease) = attached();
    let operation = lease.begin_diagnostic_operation().unwrap();
    let result = Arc::new(String::from("console result"));
    let weak = Arc::downgrade(&result);
    lease
        .diagnostic_scope()
        .unwrap()
        .with_registered_diagnostics(|| Ok(((), Some(Box::new(move || drop(result))))))
        .unwrap();
    registry.suspend_tab("browser-7");
    assert_eq!(&**weak.upgrade().unwrap(), "console result");
    drop(operation);
    assert!(weak.upgrade().is_none());
}

#[test]
fn privileged_state_is_released_on_revocation_without_losing_ordinary_handles() {
    let (registry, lease) = attached();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    let ordinary = Arc::new(String::from("snapshot handle"));
    let ordinary_weak = Arc::downgrade(&ordinary);
    lease.bind_resources(lease.resource_generation(), || true, Box::new(move || drop(ordinary))).unwrap();
    let privileged = Arc::new(String::from("privileged session"));
    let privileged_weak = Arc::downgrade(&privileged);
    let held = Mutex::new(Some(privileged));
    lease.developer_scope().bind(Arc::new(move |force| {
        if force { held.lock().unwrap().take(); }
    })).unwrap();
    registry.end_developer_mode();
    assert!(privileged_weak.upgrade().is_none());
    assert_eq!(&**ordinary_weak.upgrade().unwrap(), "snapshot handle");
    registry.revoke_tab("browser-7");
    assert!(ordinary_weak.upgrade().is_none());
}

#[test]
fn late_privileged_cleanup_cannot_end_a_reapproved_session() {
    let (registry, lease) = attached();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    let old = lease.developer_scope();
    let old_generation = lease.grant_generation();
    registry.end_developer_mode();
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    let state = Arc::new(String::from("replacement session"));
    let weak = Arc::downgrade(&state);
    let held = Mutex::new(Some(state));
    lease.developer_scope().bind(Arc::new(move |_| { held.lock().unwrap().take(); })).unwrap();
    assert_eq!(old.bind(Arc::new(|_| {})), Err(crate::acp::browser::BrowserFailure::Denied));
    lease.release_developer_resources(None, Some(old_generation));
    assert_eq!(&**weak.upgrade().unwrap(), "replacement session");
    registry.end_developer_mode();
    assert!(weak.upgrade().is_none());
}
