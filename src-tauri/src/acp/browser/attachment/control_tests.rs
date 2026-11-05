use super::*;
use crate::acp::browser::{
    BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest, BrowserResult,
    developer::{CdpAuditEntry, DeveloperModePolicy},
    permission::{
        BrowserCapabilityDispatch, BrowserDispatchPermit, BrowserPermissionChoice,
        BrowserPermissionGate, BrowserPermissionPrompt, PendingBrowserResult,
    },
};

const TAB: &str = "browser-7";
const SESSION: &str = "session-1";
const ORIGIN: &str = "https://example.com";

fn ids() -> (SageProjectId, SageChatId) {
    (
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
    )
}

fn scope() -> BrowserScope {
    BrowserScope {
        session_id: SESSION.into(),
        tab_id: 7,
    }
}

fn attach_user(registry: &BrowserAttachments) {
    let (project, chat) = ids();
    registry.attach(project, chat, TAB, ORIGIN, 1).unwrap();
}

fn agent_attached(registry: &BrowserAttachments) -> AttachmentLease {
    let (project, chat) = ids();
    registry
        .attach_agent(project.clone(), chat.clone(), SESSION, TAB, ORIGIN, 1)
        .unwrap();
    let lease = registry.agent_resolve(&project, &chat, SESSION, 7).unwrap();
    lease.authority_for(&scope()).unwrap();
    lease
}

struct Policy(AttachmentLease);

impl DeveloperModePolicy for Policy {
    fn enabled(&self) -> bool {
        true
    }
    fn granted(&self, session: &str, origin: &str) -> bool {
        self.0.developer_granted(session, origin)
    }
    fn grant(&self, session: &str, origin: &str) -> bool {
        self.0.grant_developer_mode(session, origin)
    }
    fn audit(&self, _: CdpAuditEntry) -> bool {
        true
    }
}

/// Holds an issued evaluation until the user attaches the tab, then returns
/// its value. The chrome must still describe the elevated work during the wait.
struct PendingEvaluation {
    registry: Arc<BrowserAttachments>,
    outcome: Result<BrowserResult, BrowserFailure>,
    revoke_pending: bool,
}

impl BrowserCapabilityDispatch for PendingEvaluation {
    fn dispatch(
        &mut self,
        _: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        permit.with_live_lock(|| {
            permit.mark_issued();
            Ok(())
        })?;
        let registry = self.registry.clone();
        let outcome = self.outcome.clone();
        let revoke_pending = self.revoke_pending;
        Ok(Box::new(move || {
            attach_user(&registry);
            let (project, chat) = ids();
            let control = registry.agent_control(TAB).unwrap();
            assert_eq!((control.project_id, control.chat_id), (project, chat));
            assert!(!control.user_control);
            assert!(
                control.developer_mode,
                "pending elevated work remains visible"
            );
            assert!(permit.is_live(), "attaching does not cancel the evaluation");
            if revoke_pending {
                registry.end_developer_mode();
                assert!(!registry.agent_control(TAB).unwrap().developer_mode);
                assert!(!permit.is_live(), "revocation ends pending elevated work");
            }
            outcome
        }))
    }
}

#[test]
fn user_attach_keeps_the_agent_grant_and_pending_evaluation_visible_until_revoke() {
    // A command deadline ends its work, not the session/origin approval.
    for (outcome, revoke_pending) in [
        (
            Ok(BrowserResult::Cdp {
                result: serde_json::json!({"value": 2}),
            }),
            false,
        ),
        (Err(BrowserFailure::OutcomeUnknown), false),
        (
            Ok(BrowserResult::Cdp {
                result: serde_json::json!({"value": 2}),
            }),
            true,
        ),
    ] {
        let registry = Arc::new(BrowserAttachments::default());
        let lease = agent_attached(&registry);
        let mut gate = BrowserPermissionGate::new(
            PendingEvaluation {
                registry: registry.clone(),
                outcome: outcome.clone(),
                revoke_pending,
            },
            |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowDeveloperMode,
            lease.authority_for(&scope()).unwrap(),
        )
        .with_developer_mode(Arc::new(Policy(lease.clone())));
        assert_eq!(
            gate.perform(&BrowserRequest {
                scope: scope(),
                command: BrowserCommand::Cdp {
                    method: "Runtime.evaluate".into(),
                    params: serde_json::from_value(serde_json::json!({"expression": "1 + 1"}))
                        .unwrap(),
                },
            }),
            if revoke_pending {
                Err(BrowserFailure::OutcomeUnknown)
            } else {
                outcome
            }
        );
        assert_eq!(lease.developer_granted(SESSION, ORIGIN), !revoke_pending);
        assert_eq!(
            registry.agent_control(TAB).unwrap().developer_mode,
            !revoke_pending
        );
        registry.end_developer_mode();
        assert!(!lease.developer_granted(SESSION, ORIGIN));
        assert!(!registry.agent_control(TAB).unwrap().developer_mode);
    }
}

#[test]
fn tab_control_does_not_hide_an_unpaused_agent_behind_a_paused_user_attachment() {
    let registry = BrowserAttachments::default();
    attach_user(&registry);
    assert!(registry.take_over(TAB));
    let lease = agent_attached(&registry);
    assert!(lease.grant_developer_mode(SESSION, ORIGIN));
    let control = registry.agent_control(TAB).unwrap();
    assert!(!control.user_control, "the Agent can still drive this tab");
    assert!(control.developer_mode);
    registry.take_over(TAB);
    let control = registry.agent_control(TAB).unwrap();
    assert!(control.user_control);
    assert!(!control.developer_mode);
}

#[test]
fn tab_indicator_and_privileged_resources_follow_every_attachment_lifecycle() {
    type Transition = fn(&BrowserAttachments, &AttachmentLease);
    let cases: &[(&str, Transition, [bool; 2], bool)] = &[
        (
            "attach/reattach user",
            |r, _| attach_user(r),
            [false, true],
            true,
        ),
        (
            "user moves to another tab",
            |r, _| {
                let (p, c) = ids();
                r.attach(p, c, "browser-9", ORIGIN, 1).unwrap();
                assert!(!r.agent_control("browser-9").unwrap().developer_mode);
            },
            [false, true],
            true,
        ),
        (
            "detach user",
            |r, _| {
                let (p, c) = ids();
                r.detach(&p, &c);
            },
            [false, true],
            true,
        ),
        (
            "detach agent",
            |r, _| {
                let (p, c) = ids();
                r.agent_detach(&p, &c, SESSION, 7);
            },
            [true, false],
            true,
        ),
        (
            "reattach agent",
            |r, _| {
                let (p, c) = ids();
                r.agent_detach(&p, &c, SESSION, 7);
                agent_attached(r);
            },
            [true, false],
            true,
        ),
        (
            "tab close/stop",
            |r, _| r.revoke_tab(TAB),
            [false, false],
            false,
        ),
        (
            "chat retirement with cleanup claimed",
            |r, _| {
                let (p, c) = ids();
                let _claim = r.agent_claim_sweep(TAB).unwrap();
                r.detach(&p, &c);
                r.retire_agent_chat(&p, &c);
                assert!(r.agent_has_label(TAB), "native cleanup remains tracked");
            },
            [false, false],
            false,
        ),
        (
            "chat close",
            |r, _| {
                let (p, c) = ids();
                r.detach(&p, &c);
                r.revoke_agent_chat(&p, &c);
            },
            [false, false],
            false,
        ),
        (
            "revoke developer mode",
            |r, _| {
                r.end_developer_mode();
            },
            [false, false],
            true,
        ),
        (
            "navigation start",
            |r, _| r.navigation_started(TAB),
            [true, true],
            true,
        ),
        (
            "same-origin navigation",
            |r, _| r.navigation_settled(TAB, "https://example.com/next", 2),
            [true, true],
            true,
        ),
        (
            "cross-origin navigation",
            |r, _| r.navigation_settled(TAB, "https://other.example/", 2),
            [false, false],
            true,
        ),
        (
            "return to original origin",
            |r, _| {
                r.navigation_settled(TAB, "https://other.example/", 2);
                r.navigation_settled(TAB, ORIGIN, 3);
            },
            [false, false],
            true,
        ),
        (
            "take over",
            |r, _| {
                r.take_over(TAB);
            },
            [false, false],
            true,
        ),
        (
            "hand back",
            |r, _| {
                r.take_over(TAB);
                r.hand_back(TAB);
            },
            [false, false],
            true,
        ),
        (
            "session replacement",
            |_, lease| {
                lease
                    .authority_for(&BrowserScope {
                        session_id: "session-2".into(),
                        tab_id: 7,
                    })
                    .unwrap();
            },
            [false, false],
            true,
        ),
        ("sleep", |r, _| r.suspend_tab(TAB), [true, true], true),
    ];
    for (name, transition, expected, attached) in cases {
        for (agent, survives) in expected.iter().enumerate() {
            let registry = BrowserAttachments::default();
            let agent_lease = agent_attached(&registry);
            attach_user(&registry);
            let (project, chat) = ids();
            let lease = if agent == 1 {
                agent_lease
            } else {
                let lease = registry.resolve(&project, &chat).unwrap();
                lease.authority_for(&scope()).unwrap();
                lease
            };
            assert!(lease.grant_developer_mode(SESSION, ORIGIN));
            let privileged = Arc::new(String::from("privileged native session"));
            let weak = Arc::downgrade(&privileged);
            let held = Mutex::new(Some(privileged));
            lease
                .developer_scope()
                .bind(Arc::new(move |force| {
                    if force {
                        held.lock().unwrap().take();
                    }
                }))
                .unwrap();
            assert!(registry.agent_control(TAB).unwrap().developer_mode);
            transition(&registry, &lease);
            assert_eq!(
                lease.developer_granted(SESSION, ORIGIN),
                *survives,
                "{name}, agent={agent}"
            );
            let control = registry.agent_control(TAB);
            assert_eq!(control.is_some(), *attached, "{name}, agent={agent}");
            assert_eq!(
                control.is_some_and(|c| c.developer_mode),
                *survives,
                "{name}, agent={agent}"
            );
            assert_eq!(
                weak.upgrade().is_some(),
                *survives,
                "{name}, agent={agent}: revoked native state must be released"
            );
        }
    }
}

#[test]
fn revoking_one_of_two_live_grants_keeps_the_indicator_until_the_last_grant_ends() {
    for end_user in [true, false] {
        let registry = BrowserAttachments::default();
        let agent = agent_attached(&registry);
        attach_user(&registry);
        let (project, chat) = ids();
        let user = registry.resolve(&project, &chat).unwrap();
        user.authority_for(&scope()).unwrap();
        assert!(user.grant_developer_mode(SESSION, ORIGIN));
        assert!(agent.grant_developer_mode(SESSION, ORIGIN));
        let ended = if end_user { &user } else { &agent };
        ended
            .authority_for(&BrowserScope {
                session_id: "session-2".into(),
                tab_id: 7,
            })
            .unwrap();
        assert!(!ended.developer_granted(SESSION, ORIGIN));
        assert!(registry.agent_control(TAB).unwrap().developer_mode);
        registry.end_developer_mode();
        assert!(!registry.agent_control(TAB).unwrap().developer_mode);
    }
}
