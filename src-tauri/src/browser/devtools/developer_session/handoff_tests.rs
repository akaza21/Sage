//! Production slot admission and permission gates with a native session-ID fixture.
use super::*;
use crate::{
    acp::browser::{
        BrowserCapability, BrowserCommand, BrowserRequest, BrowserResult, BrowserScope,
        attachment::{AttachmentLease, BrowserAttachments},
        permission::{
            BrowserCapabilityDispatch, BrowserPermissionChoice, BrowserPermissionGate,
            BrowserPermissionPrompt, PendingBrowserResult,
        },
        session_capability::LeaseDeveloperPolicy,
    },
    settings_store::SettingsStore,
};
use sage_acp_protocol::contract::{SageChatId, SageProjectId};
use std::sync::atomic::{AtomicUsize, Ordering};

const TAB: &str = "browser-7";
const ORIGIN: &str = "https://example.com";

fn ids() -> (SageProjectId, SageChatId) {
    (
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
    )
}
fn scope() -> BrowserScope {
    BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    }
}

struct NativeSession(Mutex<State>);
impl TabSession for NativeSession {
    fn reserve(&self) -> Result<(), BrowserFailure> {
        self.0.lock().unwrap().reserve()
    }
    fn close(self: &Arc<Self>, force: bool) {
        self.0.lock().unwrap().close(force);
    }
}

#[derive(Clone)]
struct Backend {
    slot: Slot<NativeSession>,
    next_id: Arc<AtomicUsize>,
    settings: SettingsStore,
}

struct Port {
    backend: Backend,
    grant: DeveloperScope,
    during: Option<Box<dyn FnOnce() + Send>>,
}
impl BrowserCapabilityDispatch for Port {
    fn dispatch(
        &mut self,
        _: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let (session, _) = self.backend.slot.acquire(TAB, &self.grant, |_| {
            let id = self.backend.next_id.fetch_add(1, Ordering::Relaxed);
            NativeSession(Mutex::new(State {
                phase: Phase::Ready(format!("tab-session-{id}")),
                busy: true,
                ..State::default()
            }))
        })?;
        permit.with_live_lock(|| {
            permit.mark_issued();
            Ok(())
        })?;
        let during = self.during.take();
        Ok(Box::new(move || {
            if let Some(during) = during {
                during();
            }
            let result = permit.with_live_lock(|| {
                let state = session.0.lock().unwrap();
                match &state.phase {
                    Phase::Ready(id) => Ok(BrowserResult::Cdp {
                        result: serde_json::json!({"sessionId": id, "value": 2}),
                    }),
                    _ => Err(BrowserFailure::Denied),
                }
            });
            session.0.lock().unwrap().busy = false;
            result
        }))
    }
}

fn evaluate(
    lease: &AttachmentLease,
    backend: &Backend,
    approve: bool,
    during: Option<Box<dyn FnOnce() + Send>>,
) -> Result<BrowserResult, BrowserFailure> {
    let mut gate = BrowserPermissionGate::new(
        Port {
            backend: backend.clone(),
            grant: lease.developer_scope(),
            during,
        },
        move |_: &BrowserPermissionPrompt| {
            if approve {
                BrowserPermissionChoice::AllowDeveloperMode
            } else {
                BrowserPermissionChoice::Deny
            }
        },
        lease
            .authority_for(&scope())
            .ok_or(BrowserFailure::Denied)?,
    )
    .with_developer_mode(LeaseDeveloperPolicy::new(
        lease.clone(),
        backend.settings.clone(),
    ));
    gate.perform(&BrowserRequest {
        scope: scope(),
        command: BrowserCommand::Cdp {
            method: "Runtime.evaluate".into(),
            params: serde_json::from_value(serde_json::json!({"expression": "1 + 1"})).unwrap(),
        },
    })
}

struct Database(std::path::PathBuf);
impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}
struct Fixture {
    registry: Arc<BrowserAttachments>,
    agent: AttachmentLease,
    backend: Backend,
    _database: Database,
}
fn fixture() -> Fixture {
    let registry = Arc::new(BrowserAttachments::default());
    let (project, chat) = ids();
    registry
        .attach_agent(project.clone(), chat.clone(), "session-1", TAB, ORIGIN, 1)
        .unwrap();
    let agent = registry
        .agent_resolve(&project, &chat, "session-1", 7)
        .unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../work");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("tab-handoff-{}.db", uuid::Uuid::new_v4()));
    let settings = SettingsStore::open(&path).unwrap();
    settings.set_browser_developer_mode(true).unwrap();
    Fixture {
        registry,
        agent,
        backend: Backend {
            slot: Slot::default(),
            next_id: Arc::new(AtomicUsize::new(1)),
            settings,
        },
        _database: Database(path),
    }
}
fn attach_user(registry: &BrowserAttachments) -> AttachmentLease {
    let (project, chat) = ids();
    registry
        .attach(project.clone(), chat.clone(), TAB, ORIGIN, 1)
        .unwrap();
    registry.resolve(&project, &chat).unwrap()
}
fn evaluated() -> Result<BrowserResult, BrowserFailure> {
    Ok(BrowserResult::Cdp {
        result: serde_json::json!({"sessionId":"tab-session-1", "value":2}),
    })
}

#[test]
fn agent_evaluate_then_user_approval_and_reattach_reuse_the_same_tab_session() {
    let f = fixture();
    assert_eq!(evaluate(&f.agent, &f.backend, true, None), evaluated());
    let user = attach_user(&f.registry);
    assert_eq!(
        evaluate(&user, &f.backend, false, None),
        Err(BrowserFailure::Denied)
    );
    assert_eq!(evaluate(&user, &f.backend, true, None), evaluated());
    let (project, chat) = ids();
    f.registry.detach(&project, &chat);
    let user = attach_user(&f.registry);
    assert_eq!(evaluate(&user, &f.backend, true, None), evaluated());
    f.registry.agent_detach(&project, &chat, "session-1", 7);
    assert_eq!(evaluate(&user, &f.backend, true, None), evaluated());
    f.registry.end_developer_mode();
    assert_eq!(
        user.developer_scope().with_live(TAB, || Ok(())),
        Err(BrowserFailure::Denied)
    );
    // Re-approval cannot acquire a native session whose detach is outstanding.
    assert_eq!(
        evaluate(&user, &f.backend, true, None),
        Err(BrowserFailure::Unavailable)
    );
    f.registry.revoke_tab(TAB);
    assert_eq!(
        evaluate(&user, &f.backend, true, None),
        Err(BrowserFailure::Denied)
    );
}

#[test]
fn outstanding_evaluation_finishes_on_its_original_grant_during_detach_handoff() {
    for reattach in [false, true] {
        let f = fixture();
        let handoff = f.registry.clone();
        let in_flight = f.backend.clone();
        assert_eq!(
            evaluate(
                &f.agent,
                &f.backend,
                true,
                Some(Box::new(move || {
                    let user = attach_user(&handoff);
                    // Approval succeeds but the original command still owns the slot.
                    assert_eq!(
                        evaluate(&user, &in_flight, true, None),
                        Err(BrowserFailure::Unavailable)
                    );
                    let (project, chat) = ids();
                    handoff.agent_detach(&project, &chat, "session-1", 7);
                    if reattach {
                        handoff
                            .attach_agent(project, chat, "session-1", TAB, ORIGIN, 1)
                            .unwrap();
                    }
                    assert!(handoff.agent_control(TAB).unwrap().developer_mode);
                }))
            ),
            evaluated()
        );
        let (project, chat) = ids();
        let user = f.registry.resolve(&project, &chat).unwrap();
        assert_eq!(evaluate(&user, &f.backend, true, None), evaluated());
        assert_eq!(
            f.agent.developer_scope().with_live(TAB, || Ok(())),
            Err(BrowserFailure::Denied)
        );
        assert_eq!(
            user.developer_scope().with_live("browser-8", || Ok(())),
            Err(BrowserFailure::Denied)
        );
    }
}

#[test]
fn revoke_during_handoff_closes_the_session_and_rejects_the_original_completion() {
    let f = fixture();
    let handoff = f.registry.clone();
    let backend = f.backend.clone();
    assert_eq!(
        evaluate(
            &f.agent,
            &f.backend,
            true,
            Some(Box::new(move || {
                let user = attach_user(&handoff);
                assert_eq!(
                    evaluate(&user, &backend, true, None),
                    Err(BrowserFailure::Unavailable)
                );
                let (project, chat) = ids();
                handoff.agent_detach(&project, &chat, "session-1", 7);
                handoff.revoke_tab(TAB);
                assert!(handoff.agent_control(TAB).is_none());
                let replacement = attach_user(&handoff);
                assert_eq!(
                    evaluate(&replacement, &backend, true, None),
                    Err(BrowserFailure::Unavailable)
                );
            }))
        ),
        Err(BrowserFailure::OutcomeUnknown)
    );
}

#[test]
fn original_command_cannot_consume_a_reapproved_grant_on_the_same_attachment() {
    let f = fixture();
    let registry = f.registry.clone();
    let backend = f.backend.clone();
    let agent = f.agent.clone();
    assert_eq!(
        evaluate(
            &f.agent,
            &f.backend,
            true,
            Some(Box::new(move || {
                registry.end_developer_mode();
                assert_eq!(
                    evaluate(&agent, &backend, true, None),
                    Err(BrowserFailure::Unavailable)
                );
            }))
        ),
        Err(BrowserFailure::OutcomeUnknown)
    );
    assert!(f.agent.developer_granted("session-1", ORIGIN));
    // The original guard must leave the replacement grant's command count alone.
    let command = f.agent.developer_command("session-1", ORIGIN).unwrap();
    assert!(command.is_live("session-1", ORIGIN));
    drop(command);
}
