use super::*;
use crate::acp::browser::{
    BrowserAction, BrowserCommand, BrowserResult, ElementHandle, FillSecret,
};
use std::{cell::Cell, rc::Rc};

struct Port(Rc<Cell<usize>>);
impl BrowserCapability for Port {
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        self.0.set(self.0.get() + 1);
        match &request.command {
            BrowserCommand::OpenTab { .. } => Ok(BrowserResult::OpenTab { tab_id: 9 }),
            BrowserCommand::Navigate { url } => Ok(BrowserResult::Navigate {
                url: url.clone(),
                navigation_epoch: 2,
            }),
            _ => Ok(BrowserResult::SnapshotPage {
                navigation_epoch: 1,
                text: "safe".into(),
                nodes: vec![],
            }),
        }
    }
}
impl BrowserCapabilityDispatch for Port {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let result = permit.with_live_lock(|| self.perform(request))?;
        Ok(Box::new(move || Ok(result)))
    }
}

fn scope() -> BrowserScope {
    BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    }
}
fn request(scope: BrowserScope, command: BrowserCommand) -> BrowserRequest {
    BrowserRequest { scope, command }
}
fn authority() -> BrowserAuthority {
    BrowserAuthority::attached(scope(), "https://example.com/page", 1).unwrap()
}

#[test]
fn open_tab_requires_new_origin_permission_before_dispatch() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 0,
    };
    let actions = Rc::new(Cell::new(0));
    let mut denied = BrowserPermissionGate::new(
        Port(actions.clone()),
        |prompt: &BrowserPermissionPrompt| {
            assert_eq!(prompt.operation, "openTab");
            assert_eq!(prompt.origin, "https://example.com");
            BrowserPermissionChoice::Deny
        },
        BrowserAuthority::attached(scope.clone(), "https://example.com/", 1).unwrap(),
    );
    let request = request(
        scope.clone(),
        BrowserCommand::OpenTab {
            url: "https://example.com/page".into(),
        },
    );
    assert_eq!(denied.perform(&request), Err(BrowserFailure::Denied));
    assert_eq!(actions.get(), 0);
    let mut allowed = BrowserPermissionGate::new(
        Port(actions.clone()),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        BrowserAuthority::attached(scope, "https://example.com/", 1).unwrap(),
    );
    assert_eq!(
        allowed.perform(&request),
        Ok(BrowserResult::OpenTab { tab_id: 9 })
    );
    assert_eq!(actions.get(), 1);
}

#[test]
fn deny_and_foreign_scope_never_reach_the_port() {
    let actions = Rc::new(Cell::new(0));
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::Deny,
        authority(),
    );
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::Denied)
    );
    let mut foreign = scope();
    foreign.tab_id = 8;
    assert_eq!(
        gate.perform(&request(foreign, BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::Denied)
    );
    assert_eq!(actions.get(), 0);
}

#[test]
fn allow_once_prompts_before_each_observation() {
    let actions = Rc::new(Cell::new(0));
    let prompts = Rc::new(Cell::new(0));
    let seen = prompts.clone();
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        move |prompt: &BrowserPermissionPrompt| {
            assert_eq!(prompt.origin, "https://example.com");
            seen.set(seen.get() + 1);
            BrowserPermissionChoice::AllowOnce
        },
        authority(),
    );
    for _ in 0..2 {
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {}))
            .unwrap();
    }
    assert_eq!(prompts.get(), 2);
    assert_eq!(actions.get(), 2);
}

#[test]
fn origin_grant_is_revoked_on_navigation_stop_close_and_owner_change() {
    let actions = Rc::new(Cell::new(0));
    let prompts = Rc::new(Cell::new(0));
    let seen = prompts.clone();
    let authority = authority();
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        move |_: &BrowserPermissionPrompt| {
            seen.set(seen.get() + 1);
            BrowserPermissionChoice::AllowForOriginSession
        },
        authority.clone(),
    );
    let snapshot = || request(scope(), BrowserCommand::SnapshotPage {});
    gate.perform(&snapshot()).unwrap();
    gate.perform(&snapshot()).unwrap();
    assert_eq!(prompts.get(), 1);
    authority.set_origin("https://other.example/path").unwrap();
    gate.perform(&snapshot()).unwrap();
    assert_eq!(prompts.get(), 2);
    authority.owner_changed(2);
    gate.perform(&snapshot()).unwrap();
    assert_eq!(prompts.get(), 3);
    authority.stop();
    assert_eq!(gate.perform(&snapshot()), Err(BrowserFailure::Denied));
    authority.close_tab();
    assert_eq!(gate.perform(&snapshot()), Err(BrowserFailure::Denied));
    assert_eq!(actions.get(), 4);
}

#[test]
fn origin_change_between_prompt_and_action_prevents_side_effect() {
    let actions = Rc::new(Cell::new(0));
    let authority = authority();
    let raced = authority.clone();
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        move |_: &BrowserPermissionPrompt| {
            raced.set_origin("https://other.example").unwrap();
            BrowserPermissionChoice::AllowOnce
        },
        authority,
    );
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::Denied)
    );
    assert_eq!(actions.get(), 0);
}

#[test]
fn approval_arriving_during_navigation_issues_nothing() {
    let actions = Rc::new(Cell::new(0));
    let authority = authority();
    let starting = authority.clone();
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        move |_: &BrowserPermissionPrompt| {
            starting.navigation_started();
            BrowserPermissionChoice::AllowOnce
        },
        authority,
    );
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::Denied)
    );
    assert_eq!(actions.get(), 0);
}

#[test]
fn changed_tab_revision_after_approval_issues_nothing() {
    let actions = Rc::new(Cell::new(0));
    let current = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let changed = current.clone();
    let checked = current.clone();
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        move |_: &BrowserPermissionPrompt| {
            changed.store(false, std::sync::atomic::Ordering::SeqCst);
            BrowserPermissionChoice::AllowOnce
        },
        authority(),
    )
    .with_tab_revision_guard(move || checked.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::Denied)
    );
    assert_eq!(actions.get(), 0);
}

#[test]
fn changed_owner_after_approval_issues_nothing() {
    let actions = Rc::new(Cell::new(0));
    let current = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let changed = current.clone();
    let checked = current.clone();
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        move |_: &BrowserPermissionPrompt| {
            changed.store(false, std::sync::atomic::Ordering::SeqCst);
            BrowserPermissionChoice::AllowOnce
        },
        authority(),
    )
    .with_owner_guard(move || checked.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::Denied)
    );
    assert_eq!(actions.get(), 0);
}

#[test]
fn owner_guard_is_not_queried_by_the_pending_poll() {
    struct PollingPort;
    impl BrowserCapabilityDispatch for PollingPort {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            Ok(Box::new(move || {
                for _ in 0..5 {
                    assert!(permit.is_live());
                }
                permit.with_live_lock(|| {
                    permit.mark_issued();
                    Ok(BrowserResult::SnapshotPage {
                        navigation_epoch: 1,
                        text: "safe".into(),
                        nodes: vec![],
                    })
                })
            }))
        }
    }
    let checks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = checks.clone();
    let mut gate = BrowserPermissionGate::new(
        PollingPort,
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority(),
    )
    .with_owner_guard(move || {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    });
    assert!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {}))
            .is_ok()
    );
    assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn owner_change_after_issue_reports_unknown_outcome() {
    struct IssuedPort(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl BrowserCapabilityDispatch for IssuedPort {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            let current = self.0.clone();
            Ok(Box::new(move || {
                permit.with_live_lock(|| {
                    permit.mark_issued();
                    Ok(())
                })?;
                current.store(false, std::sync::atomic::Ordering::SeqCst);
                Ok(BrowserResult::SnapshotPage {
                    navigation_epoch: 1,
                    text: "safe".into(),
                    nodes: vec![],
                })
            }))
        }
    }
    let current = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let checked = current.clone();
    let mut gate = BrowserPermissionGate::new(
        IssuedPort(current),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority(),
    )
    .with_owner_guard(move || checked.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::OutcomeUnknown)
    );
}

#[test]
fn issued_navigate_can_report_its_result_after_navigation_starts() {
    struct NavigatingPort(BrowserAuthority);
    impl BrowserCapabilityDispatch for NavigatingPort {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            permit.with_live_lock(|| Ok(()))?;
            let authority = self.0.clone();
            Ok(Box::new(move || {
                authority.navigation_started();
                Ok(BrowserResult::Navigate {
                    url: "https://target.example/".into(),
                    navigation_epoch: 2,
                })
            }))
        }
    }
    let authority = authority();
    let mut gate = BrowserPermissionGate::new(
        NavigatingPort(authority.clone()),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority,
    );
    assert!(matches!(
        gate.perform(&request(
            scope(),
            BrowserCommand::Navigate {
                url: "https://target.example/".into(),
            },
        )),
        Ok(BrowserResult::Navigate { .. })
    ));
}

#[test]
fn click_that_starts_navigation_reports_applied_instead_of_denied() {
    struct NavigatingClick(BrowserAuthority);
    impl BrowserCapabilityDispatch for NavigatingClick {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            permit.with_live_lock(|| {
                permit.mark_issued();
                Ok(())
            })?;
            let authority = self.0.clone();
            Ok(Box::new(move || {
                authority.navigation_started();
                Ok(BrowserResult::Act { applied: true })
            }))
        }
    }
    let authority = authority();
    let mut gate = BrowserPermissionGate::new(
        NavigatingClick(authority.clone()),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority,
    );
    assert_eq!(
        gate.perform(&request(
            scope(),
            BrowserCommand::Act {
                action: BrowserAction::Click {
                    handle: ElementHandle::from_token("handle".into()),
                },
            },
        )),
        Ok(BrowserResult::Act { applied: true })
    );
}

#[test]
fn stop_after_issue_reports_unknown_even_if_callback_completes() {
    struct IssuedPort(BrowserAuthority);
    impl BrowserCapabilityDispatch for IssuedPort {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            permit.with_live_lock(|| {
                permit.mark_issued();
                Ok(())
            })?;
            let authority = self.0.clone();
            Ok(Box::new(move || {
                authority.stop();
                Ok(BrowserResult::SnapshotPage {
                    navigation_epoch: 1,
                    text: "safe".into(),
                    nodes: vec![],
                })
            }))
        }
    }
    let authority = authority();
    let mut gate = BrowserPermissionGate::new(
        IssuedPort(authority.clone()),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority,
    );
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::OutcomeUnknown)
    );
}

#[test]
fn owner_change_after_navigation_does_not_inherit_navigation_success_exception() {
    struct ChangedOwner(BrowserAuthority);
    impl BrowserCapabilityDispatch for ChangedOwner {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            permit.with_live_lock(|| {
                permit.mark_issued();
                Ok(())
            })?;
            let authority = self.0.clone();
            Ok(Box::new(move || {
                authority.navigation_started();
                authority.owner_changed(2);
                Ok(BrowserResult::Navigate {
                    url: "https://target.example/".into(),
                    navigation_epoch: 2,
                })
            }))
        }
    }
    let authority = authority();
    let mut gate = BrowserPermissionGate::new(
        ChangedOwner(authority.clone()),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority,
    );
    assert_eq!(
        gate.perform(&request(
            scope(),
            BrowserCommand::Navigate {
                url: "https://target.example/".into(),
            },
        )),
        Err(BrowserFailure::OutcomeUnknown)
    );
}

#[test]
fn navigation_uses_target_origin_and_fill_secret_never_enters_prompt() {
    let actions = Rc::new(Cell::new(0));
    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = seen.clone();
    let mut gate = BrowserPermissionGate::new(
        Port(actions.clone()),
        move |prompt: &BrowserPermissionPrompt| {
            captured.borrow_mut().push(format!("{prompt:?}"));
            BrowserPermissionChoice::AllowOnce
        },
        authority(),
    );
    gate.perform(&request(
        scope(),
        BrowserCommand::Navigate {
            url: "https://target.example/path".into(),
        },
    ))
    .unwrap();
    gate.perform(&request(
        scope(),
        BrowserCommand::Act {
            action: BrowserAction::Fill {
                handle: ElementHandle::from_token("handle".into()),
                value: FillSecret::new("do-not-leak".into()),
            },
        },
    ))
    .unwrap();
    let prompt_text = seen.borrow().join(" ");
    assert!(prompt_text.contains("https://target.example"));
    assert!(!prompt_text.contains("do-not-leak"));
    assert_eq!(actions.get(), 2);
}

#[test]
fn navigation_completion_can_revoke_authority_without_deadlocking_the_gate() {
    struct CallbackPort(
        BrowserAuthority,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    );
    impl BrowserCapabilityDispatch for CallbackPort {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let authority = self.0.clone();
            let side_effects = self.1.clone();
            std::thread::spawn(move || {
                authority.stop();
                let _ = sender.send(());
            });
            Ok(Box::new(move || {
                receiver
                    .recv_timeout(std::time::Duration::from_millis(200))
                    .map_err(|_| BrowserFailure::Unavailable)?;
                permit.with_live_lock(|| {
                    side_effects.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(BrowserResult::Navigate {
                        url: "https://target.example/".into(),
                        navigation_epoch: 2,
                    })
                })
            }))
        }
    }
    let authority = authority();
    let side_effects = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut gate = BrowserPermissionGate::new(
        CallbackPort(authority.clone(), side_effects.clone()),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority,
    );
    assert_eq!(
        gate.perform(&request(
            scope(),
            BrowserCommand::Navigate {
                url: "https://target.example/".into()
            }
        )),
        Err(BrowserFailure::Denied)
    );
    assert_eq!(side_effects.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn issued_timeout_surfaces_unknown_outcome_even_after_permit_revocation() {
    struct TimedOutPort;
    impl BrowserCapabilityDispatch for TimedOutPort {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            permit.with_live_lock(|| Ok(()))?;
            Ok(Box::new(move || {
                permit.revoke();
                Err(BrowserFailure::OutcomeUnknown)
            }))
        }
    }
    let mut gate = BrowserPermissionGate::new(
        TimedOutPort,
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
        authority(),
    );
    assert_eq!(
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {})),
        Err(BrowserFailure::OutcomeUnknown)
    );
}

#[test]
fn stop_interrupts_a_pending_dispatch_without_waiting_for_its_timeout() {
    struct PendingPort(std::sync::mpsc::SyncSender<()>);
    impl BrowserCapabilityDispatch for PendingPort {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            self.0.send(()).unwrap();
            Ok(Box::new(move || {
                while permit.is_live() {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(BrowserFailure::Denied)
            }))
        }
    }
    let authority = authority();
    let stopper = authority.clone();
    let (ready, started) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let mut gate = BrowserPermissionGate::new(
            PendingPort(ready),
            |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
            authority,
        );
        gate.perform(&request(scope(), BrowserCommand::SnapshotPage {}))
    });
    started
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    stopper.stop();
    assert_eq!(worker.join().unwrap(), Err(BrowserFailure::Denied));
}
