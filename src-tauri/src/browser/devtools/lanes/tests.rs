use super::*;
use crate::acp::{
    browser::{
        BrowserAction, BrowserCapability, BrowserCommand, BrowserRequest, BrowserResult,
        ViewportRegion, WaitCondition, attachment::BrowserAttachments,
    },
    contract::{SageChatId, SageProjectId},
};
use crate::browser::devtools::{
    capability::{DevtoolsCapability, DevtoolsPort},
    snapshot::{DocumentStamp, NativeNode},
};

#[derive(Clone)]
struct Page(u64);

impl DevtoolsPort for Page {
    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
        Ok((
            DocumentStamp {
                navigation_id: 1,
                generation: 1,
            },
            vec![NativeNode {
                backend_id: self.0,
                depth: 1,
                role: "button".into(),
                name: "Continue".into(),
            }],
        ))
    }

    fn act(
        &mut self,
        stamp: DocumentStamp,
        backend_id: u64,
        _: &BrowserAction,
    ) -> Result<bool, BrowserFailure> {
        Ok(stamp.navigation_id == 1 && backend_id == self.0)
    }

    fn capture(&mut self, _: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }

    fn matches(
        &mut self,
        _: &WaitCondition,
        _: std::time::Duration,
    ) -> Result<bool, BrowserFailure> {
        Ok(false)
    }
}

#[test]
fn joined_tab_snapshots_then_clicks_after_using_the_opener_lane() {
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach_agent(
            project.clone(),
            chat.clone(),
            "session-1",
            "browser-7",
            "https://example.com/",
            1,
        )
        .unwrap();
    drop(registry.begin_agent_action("browser-7"));
    assert!(registry.adopt_popup(
        "browser-7",
        "browser-9",
        Some(("https://example.com/child", 1)),
        &|_, _, _| true
    ));
    let mut lanes = AttachmentLanes::default();
    let mut child_handle = None;
    for number in [9, 7] {
        let scope = BrowserScope {
            session_id: "session-1".into(),
            tab_id: number,
        };
        let lease = registry
            .agent_resolve(&project, &chat, "session-1", number)
            .unwrap();
        let lane = lanes
            .get_or_try_insert(&scope, &lease, || {
                Ok(DevtoolsCapability::new(scope.clone(), Page(number)))
            })
            .unwrap();
        let BrowserResult::SnapshotPage { nodes, .. } = lane
            .perform(&BrowserRequest {
                scope,
                command: BrowserCommand::SnapshotPage {},
            })
            .unwrap()
        else {
            panic!("snapshot")
        };
        if number == 9 {
            child_handle = Some(nodes[0].handle.clone());
        }
    }
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 9,
    };
    let lease = registry
        .agent_resolve(&project, &chat, "session-1", 9)
        .unwrap();
    let lane = lanes
        .get_or_try_insert(&scope, &lease, || panic!("joined lane was discarded"))
        .unwrap();
    assert_eq!(
        lane.perform(&BrowserRequest::act(
            scope,
            BrowserAction::Click {
                handle: child_handle.unwrap()
            }
        )),
        Ok(BrowserResult::Act { applied: true })
    );
    registry.revoke_tab("browser-7");
    assert!(!lease.is_live());
}

#[test]
fn a_new_session_or_attachment_never_reuses_an_old_scope_lane() {
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-1",
            "https://example.com/",
            1,
        )
        .unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    let mut lanes = AttachmentLanes::default();
    for session in ["session-1", "session-2"] {
        let scope = BrowserScope {
            session_id: session.into(),
            tab_id: 1,
        };
        let lane = lanes
            .get_or_try_insert(&scope, &lease, || {
                Ok(DevtoolsCapability::new(scope.clone(), Page(1)))
            })
            .unwrap();
        assert!(
            lane.perform(&BrowserRequest {
                scope,
                command: BrowserCommand::SnapshotPage {}
            })
            .is_ok()
        );
    }
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-1",
            "https://example.com/",
            1,
        )
        .unwrap();
    let replacement = registry.resolve(&project, &chat).unwrap();
    let scope = BrowserScope {
        session_id: "session-2".into(),
        tab_id: 1,
    };
    let mut created = false;
    lanes
        .get_or_try_insert(&scope, &replacement, || {
            created = true;
            Ok(DevtoolsCapability::new(scope.clone(), Page(1)))
        })
        .unwrap();
    assert!(created);
}

#[test]
fn sleeping_keeps_handles_and_approval_but_releases_diagnostic_history() {
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-1",
            "https://example.com/",
            1,
        )
        .unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    lease.authority_for(&scope).unwrap();
    assert!(lease.grant_developer_mode("session-1", "https://example.com"));
    let mut lanes = AttachmentLanes::default();
    let lane = lanes
        .get_or_try_insert(&scope, &lease, || {
            Ok(DevtoolsCapability::new(scope.clone(), Page(1)))
        })
        .unwrap();
    let BrowserResult::SnapshotPage { nodes, .. } = lane
        .perform(&BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        })
        .unwrap()
    else {
        panic!("snapshot")
    };
    let rings = lane.diagnostics();
    rings.lock().unwrap().push_console("log", "before sleeping");
    lease
        .bind_resources(
            lease.resource_generation(),
            || true,
            Box::new(lane.release_buffers()),
        )
        .unwrap();
    let sleeping_rings = rings.clone();
    lease
        .diagnostic_scope()
        .unwrap()
        .with_registered_diagnostics(|| {
            Ok((
                (),
                Some(Box::new(move || sleeping_rings.lock().unwrap().suspend())),
            ))
        })
        .unwrap();
    registry.suspend_tab("browser-1");
    assert!(lease.developer_granted("session-1", "https://example.com"));
    assert!(
        rings
            .lock()
            .unwrap()
            .console(None, 10)
            .unwrap()
            .1
            .is_empty()
    );
    let lane = lanes
        .get_or_try_insert(&scope, &lease, || {
            panic!("sleep discarded semantic handles")
        })
        .unwrap();
    assert_eq!(
        lane.perform(&BrowserRequest::act(
            scope.clone(),
            BrowserAction::Click {
                handle: nodes[0].handle.clone()
            }
        )),
        Ok(BrowserResult::Act { applied: true })
    );
    registry.end_developer_mode();
    assert!(!lease.developer_granted("session-1", "https://example.com"));
    assert!(
        lane.perform(&BrowserRequest {
            scope,
            command: BrowserCommand::SnapshotPage {}
        })
        .is_ok()
    );
}

#[test]
fn switching_developer_mode_off_preserves_an_ordinary_lane() {
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-1",
            "https://example.com/",
            1,
        )
        .unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut lane = DevtoolsCapability::new(scope.clone(), Page(1));
    lease
        .bind_resources(
            lease.resource_generation(),
            || true,
            Box::new(lane.release_buffers()),
        )
        .unwrap();
    registry.end_developer_mode();
    assert!(
        lane.perform(&BrowserRequest {
            scope,
            command: BrowserCommand::SnapshotPage {}
        })
        .is_ok()
    );
}

#[test]
fn switching_developer_mode_off_during_an_ordinary_wait_keeps_its_result_and_history() {
    use std::sync::Arc;
    struct RevokingPage {
        registry: Arc<BrowserAttachments>,
        ready: bool,
    }
    impl DevtoolsPort for RevokingPage {
        fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
            Page(1).snapshot()
        }
        fn capture(&mut self, _: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure> {
            Err(BrowserFailure::Unavailable)
        }
        fn matches(
            &mut self,
            _: &WaitCondition,
            _: std::time::Duration,
        ) -> Result<bool, BrowserFailure> {
            if self.ready {
                return Ok(true);
            }
            self.registry.end_developer_mode();
            self.ready = true;
            Ok(false)
        }
    }
    let registry = Arc::new(BrowserAttachments::default());
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-1",
            "https://example.com/",
            1,
        )
        .unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    lease.authority_for(&scope).unwrap();
    assert!(lease.grant_developer_mode("session-1", "https://example.com"));
    let mut lane = DevtoolsCapability::new(
        scope.clone(),
        RevokingPage {
            registry,
            ready: false,
        },
    );
    lane.diagnostics()
        .lock()
        .unwrap()
        .push_console("log", "ordinary history");
    lease
        .bind_resources(
            lease.resource_generation(),
            || true,
            Box::new(lane.release_buffers()),
        )
        .unwrap();
    assert_eq!(
        lane.perform(&BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::WaitFor {
                condition: WaitCondition::Text {
                    text: "ready".into()
                },
                timeout_ms: 5000
            }
        }),
        Ok(BrowserResult::WaitFor { matched: true })
    );
    let BrowserResult::ReadConsole { entries, .. } = lane
        .perform(&BrowserRequest {
            scope,
            command: BrowserCommand::ReadConsole {
                cursor: None,
                limit: 1,
            },
        })
        .unwrap()
    else {
        panic!("console")
    };
    assert_eq!(entries[0].text, "ordinary history");
    assert!(!lease.developer_granted("session-1", "https://example.com"));
}
