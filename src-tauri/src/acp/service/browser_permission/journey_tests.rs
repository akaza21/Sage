use super::*;
use crate::acp::browser::{
    BrowserAction, BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest,
    BrowserResult, BrowserScope, FakeBrowserCapability, FillSecret,
    permission::{
        BrowserCapabilityDispatch, BrowserDispatchPermit, BrowserPermissionGate,
        PendingBrowserResult,
    },
};
use std::sync::{Arc, Mutex};

fn allow_visible_request(
    service: &super::super::super::AcpService,
    owner: &BrowserPermissionOwner,
) {
    let deadline = Instant::now() + Duration::from_secs(2);
    let request = loop {
        let candidate = service
            .browser_permissions
            .lock()
            .unwrap()
            .keys()
            .next()
            .cloned();
        if let Some(request) = candidate {
            let committed: bool = service.database.connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM server_requests WHERE id=?1 AND kind='browserPermission' AND state='received')",
                [request.as_str()], |row| row.get(0),
            ).unwrap();
            if committed {
                break request;
            }
        }
        assert!(
            Instant::now() < deadline,
            "browser approval card was not published"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let response = service
        .resolve_browser_permission_decision(
            owner.clone(),
            request.clone(),
            BrowserPermissionChoice::AllowOnce,
        )
        .unwrap();
    assert_eq!(response.state, "resolvedAllowed");
    let state: String = service
        .database
        .connection()
        .query_row(
            "SELECT state FROM server_requests WHERE id=?1",
            [request.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "resolvedAllowed");
}

struct Port(Arc<Mutex<FakeBrowserCapability>>);

impl BrowserCapabilityDispatch for Port {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let port = Arc::clone(&self.0);
        let request = request.clone();
        Ok(Box::new(move || {
            permit.with_live_lock(|| {
                permit.mark_issued();
                port.lock().unwrap().perform(&request)
            })
        }))
    }
}

fn approve_operations(
    service: &Arc<super::super::super::AcpService>,
    owner: &BrowserPermissionOwner,
    tab: u64,
    fill: bool,
) {
    let lease = service
        .agent_browser_attachment(
            &owner.project_id,
            &owner.chat_id,
            owner.session_id.as_str(),
            tab,
        )
        .or_else(|| service.browser_attachment(&owner.project_id, &owner.chat_id))
        .unwrap();
    let scope = BrowserScope {
        session_id: owner.session_id.as_str().into(),
        tab_id: tab,
    };
    let port = Arc::new(Mutex::new(FakeBrowserCapability::default()));
    port.lock().unwrap().open_tab(scope.clone());
    let mut handle: Option<crate::acp::browser::ElementHandle> = None;
    for step in 0..2 {
        let authority = lease.authority_for(&scope).unwrap();
        let command = if step == 0 {
            BrowserCommand::SnapshotPage {}
        } else {
            BrowserCommand::Act {
                action: if fill {
                    BrowserAction::Fill {
                        handle: handle.clone().unwrap(),
                        value: FillSecret::new("turn two text".into()),
                    }
                } else {
                    BrowserAction::Click {
                        handle: handle.clone().unwrap(),
                    }
                },
            }
        };
        let request = BrowserRequest {
            scope: scope.clone(),
            command,
        };
        let worker = {
            let service = Arc::clone(service);
            let owner = owner.clone();
            let lease = lease.clone();
            let port = Arc::clone(&port);
            std::thread::spawn(move || {
                let precheck = Arc::clone(&service);
                let witness = Arc::clone(&service);
                let precheck_owner = owner.clone();
                let witness_owner = owner.clone();
                let decider = ServiceBrowserPermissionDecider {
                    service: &service,
                    owner,
                    authority: authority.clone(),
                    attachment: Some(lease),
                };
                BrowserPermissionGate::new(Port(port), decider, authority)
                    .with_owner_guard(move || {
                        precheck.browser_owner_precheck_matches(&precheck_owner)
                    })
                    .with_owner_witness(move || {
                        witness.browser_owner_witness_matches(&witness_owner)
                    })
                    .perform(&request)
            })
        };
        allow_visible_request(service, owner);
        match worker.join().unwrap().unwrap() {
            BrowserResult::SnapshotPage { nodes, .. } => handle = Some(nodes[0].handle.clone()),
            BrowserResult::Act { applied } => assert!(applied),
            _ => panic!("snapshot followed by one action"),
        }
    }
    assert_eq!(port.lock().unwrap().action_count(), 1);
}

#[test]
fn joined_tab_snapshot_and_click_permissions_are_visible_and_resolve() {
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
    let tree = service.scheduler.reserve_tree(runtime_owner).unwrap();
    let _turn = tree
        .reserve_turn()
        .unwrap()
        .bind_retained(owner.turn_id.clone())
        .unwrap();
    service
        .attach_browser_tab(
            owner.project_id.clone(),
            owner.chat_id.clone(),
            "browser-7",
            "https://example.com/",
            1,
        )
        .unwrap();
    service
        .browser_attachment(&owner.project_id, &owner.chat_id)
        .unwrap()
        .authority_for(&BrowserScope {
            session_id: owner.session_id.as_str().into(),
            tab_id: 7,
        })
        .unwrap();
    drop(service.begin_browser_agent_action("browser-7"));
    service.adopt_browser_popup(
        "browser-7",
        "browser-9",
        Some(("https://example.com/child", 1)),
    );
    assert!(service.browser_agent_control("browser-9").is_some());
    approve_operations(&service, &owner, 9, false);
}

#[test]
fn fresh_first_chat_turn_two_renews_browser_owner_after_queue_dispatch() {
    let (service, runtime_owner) =
        super::super::super::permission::tests::support::service_with_pending_requests(Arc::new(
            Mutex::new(Vec::new()),
        ));
    let service = Arc::new(service);
    let first = service
        .current_browser_owner(
            &runtime_owner.project_id,
            &runtime_owner.chat_id,
            &runtime_owner.session_id,
        )
        .unwrap();
    service
        .attach_browser_tab(
            first.project_id.clone(),
            first.chat_id.clone(),
            "browser-1",
            "https://example.com/",
            1,
        )
        .unwrap();
    let tree = service.scheduler.reserve_tree(runtime_owner).unwrap();
    let turn = tree
        .reserve_turn()
        .unwrap()
        .bind_retained(first.turn_id.clone())
        .unwrap();
    service
        .database
        .connection()
        .execute(
            "UPDATE turns SET state='completed',terminal_at=2 WHERE id=?1",
            [first.turn_id.as_str()],
        )
        .unwrap();
    turn.terminal_committed().unwrap();
    service.database.connection().execute(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at)
             VALUES('turn-two',?1,?2,?3,1,1,'turn-two','queued','completeBytes',0,2,2)",
            rusqlite::params![first.project_id.as_str(), first.chat_id.as_str(), first.session_id.as_str()],
        ).unwrap();
    assert!(
        service
            .current_browser_owner(&first.project_id, &first.chat_id, &first.session_id)
            .is_none()
    );
    service
        .database
        .connection()
        .execute_batch("UPDATE turns SET state='sending' WHERE id='turn-two'; UPDATE turns SET state='running' WHERE id='turn-two';")
        .unwrap();
    let _turn = tree
        .reserve_turn()
        .unwrap()
        .bind_retained(SageTurnId::parse("turn-two").unwrap())
        .unwrap();
    let owner = service
        .current_browser_owner(&first.project_id, &first.chat_id, &first.session_id)
        .unwrap();
    assert_eq!(owner.turn_id.as_str(), "turn-two");
    assert!(service.browser_owner_precheck_matches(&owner));
    assert!(service.browser_owner_witness_matches(&owner));
    assert!(!service.browser_owner_precheck_matches(&first));
    approve_operations(&service, &owner, 1, true);
}

#[test]
fn refused_join_leaves_the_user_tab_plain_and_pending_approval_resolvable() {
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
    let tree = service.scheduler.reserve_tree(runtime_owner).unwrap();
    let _turn = tree
        .reserve_turn()
        .unwrap()
        .bind_retained(owner.turn_id.clone())
        .unwrap();
    service
        .attach_browser_tab(
            owner.project_id.clone(),
            owner.chat_id.clone(),
            "browser-7",
            "https://example.com/",
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
    let authority = lease.authority_for(&scope).unwrap();
    drop(service.begin_browser_agent_action("browser-7"));
    for label in ["browser-9", "browser-10"] {
        service.adopt_browser_popup("browser-7", label, Some(("https://example.com/child", 1)));
        assert!(service.browser_agent_control(label).is_some());
    }
    let worker = {
        let service = Arc::clone(&service);
        let owner = owner.clone();
        std::thread::spawn(move || {
            use crate::acp::browser::permission::BrowserPermissionDecider;
            ServiceBrowserPermissionDecider {
                service: &service,
                owner,
                authority,
                attachment: Some(lease),
            }
            .decide(&BrowserPermissionPrompt {
                scope,
                origin: "https://example.com".into(),
                operation: "snapshotPage",
            })
        })
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while service.browser_permissions.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    drop(service.begin_browser_agent_action("browser-7"));
    service.adopt_browser_popup(
        "browser-7",
        "browser-11",
        Some(("https://example.com/child", 1)),
    );
    assert!(service.browser_agent_control("browser-11").is_none());
    assert_eq!(service.browser_permissions.lock().unwrap().len(), 1);
    allow_visible_request(&service, &owner);
    assert_eq!(worker.join().unwrap(), BrowserPermissionChoice::AllowOnce);
}
