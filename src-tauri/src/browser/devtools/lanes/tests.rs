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
