use super::*;
use crate::acp::browser::permission::{
    BrowserCapabilityDispatch, BrowserDispatchPermit, BrowserPermissionGate, PendingBrowserResult,
};
use crate::acp::browser::{
    BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest, BrowserResult,
};

#[test]
fn browser_owner_witness_requires_the_live_turn_even_with_a_stale_sql_owner() {
    let (service, runtime_owner) =
        super::super::super::permission::tests::support::service_with_pending_requests(Arc::new(
            Mutex::new(Vec::new()),
        ));
    let browser_owner = service
        .current_browser_owner(
            &runtime_owner.project_id,
            &runtime_owner.chat_id,
            &runtime_owner.session_id,
        )
        .unwrap();
    assert!(!service.browser_owner_witness_matches(&browser_owner));

    let tree = service
        .lane_at(0)
        .scheduler
        .reserve_tree(runtime_owner.clone())
        .unwrap();
    let turn = tree
        .reserve_turn()
        .unwrap()
        .bind_retained(browser_owner.turn_id.clone())
        .unwrap();
    assert!(service.browser_owner_witness_matches(&browser_owner));
    turn.terminal_committed().unwrap();
    assert!(!service.browser_owner_witness_matches(&browser_owner));
}

#[test]
fn browser_owner_witness_denies_a_rotated_turn_before_issue() {
    let (service, runtime_owner) =
        super::super::super::permission::tests::support::service_with_pending_requests(Arc::new(
            Mutex::new(Vec::new()),
        ));
    let browser_owner = service
        .current_browser_owner(
            &runtime_owner.project_id,
            &runtime_owner.chat_id,
            &runtime_owner.session_id,
        )
        .unwrap();
    let tree = service
        .lane_at(0)
        .scheduler
        .reserve_tree(runtime_owner)
        .unwrap();
    let first = tree
        .reserve_turn()
        .unwrap()
        .bind_retained(browser_owner.turn_id.clone())
        .unwrap();
    assert!(service.browser_owner_witness_matches(&browser_owner));
    first.terminal_committed().unwrap();
    let next = tree
        .reserve_turn()
        .unwrap()
        .bind_retained(SageTurnId::parse("turn-after-rebind").unwrap())
        .unwrap();
    assert!(!service.browser_owner_witness_matches(&browser_owner));
    next.terminal_committed().unwrap();
}

#[test]
fn owner_rotation_between_sql_precheck_and_issue_denies_the_call() {
    struct Port;
    impl BrowserCapabilityDispatch for Port {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            Ok(Box::new(move || {
                permit.with_live_lock(|| {
                    permit.mark_issued();
                    Ok(BrowserResult::SnapshotPage {
                        navigation_epoch: 1,
                        text: String::new(),
                        nodes: vec![],
                    })
                })
            }))
        }
    }
    let (service, runtime_owner) =
        super::super::super::permission::tests::support::service_with_pending_requests(Arc::new(
            Mutex::new(Vec::new()),
        ));
    let service = Arc::new(service);
    let owner = service
        .current_browser_owner(
            &runtime_owner.project_id,
            &runtime_owner.chat_id,
            &runtime_owner.session_id,
        )
        .unwrap();
    let tree = service
        .lane_at(0)
        .scheduler
        .reserve_tree(runtime_owner)
        .unwrap();
    let first = tree
        .reserve_turn()
        .unwrap()
        .bind(owner.turn_id.clone())
        .unwrap();
    assert!(service.browser_owner_precheck_matches(&owner));
    let precheck_service = Arc::clone(&service);
    let witness_service = Arc::clone(&service);
    let precheck_owner = owner.clone();
    let witness_owner = owner.clone();
    let scope = BrowserScope {
        session_id: owner.session_id.as_str().into(),
        tab_id: 7,
    };
    let authority = BrowserAuthority::attached(scope.clone(), "https://example.com", 1).unwrap();
    let mut first = Some(first);
    let mut gate = BrowserPermissionGate::new(
        Port,
        move |_: &BrowserPermissionPrompt| {
            first.take().unwrap().terminal_committed().unwrap();
            let _next = tree
                .reserve_turn()
                .unwrap()
                .bind(SageTurnId::parse("turn-after-rebind").unwrap())
                .unwrap();
            BrowserPermissionChoice::AllowOnce
        },
        authority,
    )
    .with_owner_guard(move || precheck_service.browser_owner_precheck_matches(&precheck_owner))
    .with_owner_witness(move || witness_service.browser_owner_witness_matches(&witness_owner));
    assert_eq!(
        gate.perform(&BrowserRequest {
            scope,
            command: BrowserCommand::SnapshotPage {},
        }),
        Err(BrowserFailure::Denied)
    );
}

#[cfg(windows)]
#[test]
fn session_capability_owner_guard_denies_changed_binding() {
    let (service, runtime_owner) =
        super::super::super::permission::tests::support::service_with_pending_requests(Arc::new(
            Mutex::new(Vec::new()),
        ));
    let owner = service
        .current_browser_owner(
            &runtime_owner.project_id,
            &runtime_owner.chat_id,
            &runtime_owner.session_id,
        )
        .unwrap();
    let tree = service
        .lane_at(0)
        .scheduler
        .reserve_tree(runtime_owner)
        .unwrap();
    let _turn = tree
        .reserve_turn()
        .unwrap()
        .bind(owner.turn_id.clone())
        .unwrap();
    assert!(crate::acp::browser::session_capability::session_owner_precheck(&service, &owner));
    assert!(crate::acp::browser::session_capability::session_owner_witness(&service, &owner));
    assert!(
        service
            .database
            .connection()
            .execute(
                "UPDATE agent_sessions SET binding_digest=?2 WHERE id=?1",
                rusqlite::params![owner.session_id.as_str(), "b".repeat(64)],
            )
            .is_err()
    );
    let mut rebound = owner.clone();
    rebound.binding_digest = "b".repeat(64);
    assert!(!crate::acp::browser::session_capability::session_owner_precheck(&service, &rebound));
    assert!(crate::acp::browser::session_capability::session_owner_witness(&service, &owner));
}
