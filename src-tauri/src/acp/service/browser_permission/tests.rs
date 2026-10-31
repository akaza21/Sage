use super::*;
use crate::acp::browser::BrowserScope;
use crate::acp::browser::permission::BrowserPermissionDecider;
use std::sync::{Arc, Mutex};

#[path = "journey_tests.rs"]
mod journey_tests;
#[path = "owner_guard_tests.rs"]
mod owner_guard_tests;

#[test]
fn open_tab_without_attachment_uses_the_visible_permission_path() {
    let (service, owner) = super::super::permission::tests::support::service_with_pending_requests(
        Arc::new(Mutex::new(Vec::new())),
    );
    let service = Arc::new(service);
    let browser_owner = BrowserPermissionOwner {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: "a".repeat(64),
    };
    let prompt = BrowserPermissionPrompt {
        scope: BrowserScope {
            session_id: browser_owner.session_id.as_str().into(),
            tab_id: 0,
        },
        origin: "https://example.com".into(),
        operation: "openTab",
    };
    let worker = {
        let service = Arc::clone(&service);
        std::thread::spawn(move || {
            service.request_browser_permission(browser_owner, &prompt, || true)
        })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let request_id = loop {
        if let Some((request_id, _)) = service.browser_permissions.lock().unwrap().iter().next() {
            let committed: bool = service.database.connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM server_requests WHERE id=?1 AND kind='browserPermission')",
                [request_id.as_str()],
                |row| row.get(0),
            ).unwrap();
            if committed {
                break request_id.clone();
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "openTab permission did not become visible"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let decision_owner = BrowserPermissionOwner {
        project_id: owner.project_id,
        chat_id: owner.chat_id,
        session_id: owner.session_id,
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: String::new(),
    };
    service
        .resolve_browser_permission_decision(
            decision_owner,
            request_id,
            BrowserPermissionChoice::AllowOnce,
        )
        .unwrap();
    assert_eq!(worker.join().unwrap(), BrowserPermissionChoice::AllowOnce);
}

#[test]
fn local_browser_request_waits_for_a_visible_durable_permission() {
    let harness_commands = Arc::new(Mutex::new(Vec::new()));
    let (service, owner) = super::super::permission::tests::support::service_with_pending_requests(
        Arc::clone(&harness_commands),
    );
    let service = Arc::new(service);
    let browser_owner = BrowserPermissionOwner {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: "a".repeat(64),
    };
    let prompt = BrowserPermissionPrompt {
        scope: BrowserScope {
            session_id: browser_owner.session_id.as_str().into(),
            tab_id: 7,
        },
        origin: "https://example.com".into(),
        operation: "snapshotPage",
    };
    let worker = {
        let service = Arc::clone(&service);
        std::thread::spawn(move || {
            service.request_browser_permission(browser_owner, &prompt, || true)
        })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let pending = loop {
        if let Some((request_id, _)) = service.browser_permissions.lock().unwrap().iter().next() {
            break request_id.clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "browser request never entered the visible pending registry"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    loop {
        let count: i64 = service.database.connection().query_row(
            "SELECT COUNT(*) FROM server_requests WHERE kind='browserPermission' AND state='received'",
            [], |row| row.get(0),
        ).unwrap();
        if count == 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "browser request was not durably committed"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let decision_owner = BrowserPermissionOwner {
        project_id: owner.project_id,
        chat_id: owner.chat_id,
        session_id: owner.session_id,
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: String::new(),
    };
    let mut wrong_owner = decision_owner.clone();
    wrong_owner.turn_id = SageTurnId::parse("other-turn").unwrap();
    assert!(
        service
            .resolve_browser_permission_decision(
                wrong_owner,
                pending.clone(),
                BrowserPermissionChoice::AllowOnce
            )
            .is_err()
    );
    assert_eq!(service.browser_permissions.lock().unwrap().len(), 1);
    let response = service
        .resolve_browser_permission_decision(
            decision_owner,
            pending.clone(),
            BrowserPermissionChoice::AllowOnce,
        )
        .unwrap();
    assert_eq!(response.state, "resolvedAllowed");
    assert_eq!(
        serde_json::to_value(&response).unwrap()["source"],
        "browser"
    );
    assert_eq!(worker.join().unwrap(), BrowserPermissionChoice::AllowOnce);
    assert!(service.browser_permissions.lock().unwrap().is_empty());
    let state: String = service
        .database
        .connection()
        .query_row(
            "SELECT state FROM server_requests WHERE id=?1",
            [pending.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "resolvedAllowed");
    assert!(
        harness_commands.lock().unwrap().is_empty(),
        "local browser permission must not reply to the harness"
    );
}

#[test]
fn stopped_tab_denies_a_pending_browser_permission_without_a_harness_reply() {
    let harness_commands = Arc::new(Mutex::new(Vec::new()));
    let (service, owner) = super::super::permission::tests::support::service_with_pending_requests(
        Arc::clone(&harness_commands),
    );
    let service = Arc::new(service);
    let scope = BrowserScope {
        session_id: owner.session_id.as_str().into(),
        tab_id: 7,
    };
    let authority = BrowserAuthority::attached(scope.clone(), "https://example.com", 1).unwrap();
    let browser_owner = BrowserPermissionOwner {
        project_id: owner.project_id,
        chat_id: owner.chat_id,
        session_id: owner.session_id,
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: "a".repeat(64),
    };
    let worker = {
        let service = Arc::clone(&service);
        let authority = authority.clone();
        std::thread::spawn(move || {
            let mut decider = ServiceBrowserPermissionDecider {
                service: &service,
                owner: browser_owner,
                authority,
                attachment: None,
            };
            decider.decide(&BrowserPermissionPrompt {
                scope,
                origin: "https://example.com".into(),
                operation: "snapshotPage",
            })
        })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let request_id = loop {
        if let Some((request_id, _)) = service.browser_permissions.lock().unwrap().iter().next() {
            let count: i64 = service
                .database
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM server_requests WHERE id=?1",
                    [request_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            if count == 1 {
                break request_id.clone();
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "browser request did not become durable"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    authority.stop();
    assert_eq!(worker.join().unwrap(), BrowserPermissionChoice::Deny);
    let state: String = service
        .database
        .connection()
        .query_row(
            "SELECT state FROM server_requests WHERE id=?1",
            [request_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "resolvedDeclined");
    assert!(harness_commands.lock().unwrap().is_empty());
}

#[test]
fn detaching_the_chat_tab_denies_a_pending_browser_permission() {
    let (service, owner) = super::super::permission::tests::support::service_with_pending_requests(
        Arc::new(Mutex::new(Vec::new())),
    );
    let service = Arc::new(service);
    service
        .attach_browser_tab(
            owner.project_id.clone(),
            owner.chat_id.clone(),
            "browser-7",
            "https://example.com",
            1,
        )
        .unwrap();
    let lease = service
        .browser_attachment(&owner.project_id, &owner.chat_id)
        .unwrap();
    let scope = BrowserScope {
        session_id: owner.session_id.as_str().into(),
        tab_id: 7,
    };
    let authority = BrowserAuthority::attached(scope.clone(), "https://example.com", 1).unwrap();
    let browser_owner = BrowserPermissionOwner {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id,
        turn_id: SageTurnId::parse("turn-permission").unwrap(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: "a".repeat(64),
    };
    let worker = {
        let service = Arc::clone(&service);
        std::thread::spawn(move || {
            let mut decider = ServiceBrowserPermissionDecider {
                service: &service,
                owner: browser_owner,
                authority,
                attachment: Some(lease),
            };
            decider.decide(&BrowserPermissionPrompt {
                scope,
                origin: "https://example.com".into(),
                operation: "snapshotPage",
            })
        })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some((request_id, _)) = service.browser_permissions.lock().unwrap().iter().next() {
            let count: i64 = service
                .database
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM server_requests WHERE id=?1",
                    [request_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            if count == 1 {
                break;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "browser request did not become durable"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    service
        .detach_browser_tab(&owner.project_id, &owner.chat_id)
        .unwrap();
    assert_eq!(worker.join().unwrap(), BrowserPermissionChoice::Deny);
    assert_eq!(service.database.connection().query_row("SELECT COUNT(*) FROM server_requests WHERE kind='browserPermission' AND state='resolvedDeclined'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn removed_chat_tab_revokes_its_browser_attachment_before_another_call() {
    let (service, owner) = super::super::permission::tests::support::service_with_pending_requests(
        Arc::new(Mutex::new(Vec::new())),
    );
    service
        .attach_browser_tab(
            owner.project_id.clone(),
            owner.chat_id.clone(),
            "browser-7",
            "https://example.com",
            1,
        )
        .unwrap();
    let lease = service
        .browser_attachment(&owner.project_id, &owner.chat_id)
        .unwrap();
    service
        .database
        .connection()
        .execute(
            "DELETE FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
            rusqlite::params![owner.project_id.as_str(), owner.chat_id.as_str()],
        )
        .unwrap();
    assert!(
        service
            .browser_attachment(&owner.project_id, &owner.chat_id)
            .is_none()
    );
    assert!(!lease.is_live());
}
