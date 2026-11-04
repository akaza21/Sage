//! The gate's developer-mode path (ADR-0009b).

use super::*;
use crate::acp::browser::{
    BrowserCommand, BrowserResult,
    developer::{CdpAuditEntry, CdpAuditOutcome},
};
use serde_json::{Map, json};
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct Policy {
    enabled: AtomicBool,
    grant: Mutex<Option<(String, String)>>,
    refuse_grant: AtomicBool,
    refuse_audit: AtomicBool,
    audit: Mutex<Vec<CdpAuditEntry>>,
}

impl DeveloperModePolicy for Policy {
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    fn granted(&self, session: &str, origin: &str) -> bool {
        self.grant.lock().unwrap().as_ref() == Some(&(session.to_owned(), origin.to_owned()))
    }
    fn grant(&self, session: &str, origin: &str) -> bool {
        if self.refuse_grant.load(Ordering::SeqCst) {
            return false;
        }
        *self.grant.lock().unwrap() = Some((session.to_owned(), origin.to_owned()));
        true
    }
    fn audit(&self, entry: CdpAuditEntry) -> bool {
        if self.refuse_audit.load(Ordering::SeqCst) {
            return false;
        }
        self.audit.lock().unwrap().push(entry);
        true
    }
}

impl Policy {
    fn on() -> Arc<Self> {
        let policy = Self::default();
        policy.enabled.store(true, Ordering::SeqCst);
        Arc::new(policy)
    }
    fn outcomes(&self) -> Vec<&'static str> {
        self.audit
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.outcome.as_str())
            .collect()
    }
}

#[derive(Clone, Default)]
struct Port(Arc<AtomicUsize>);

impl BrowserCapabilityDispatch for Port {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let result = permit.with_live_lock(|| {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(match &request.command {
                BrowserCommand::Cdp { .. } => BrowserResult::Cdp {
                    result: json!({"result": {"type": "number", "value": 2}}),
                },
                _ => BrowserResult::Act { applied: true },
            })
        })?;
        Ok(Box::new(move || Ok(result)))
    }
}

fn scope() -> BrowserScope {
    BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    }
}

fn authority() -> BrowserAuthority {
    BrowserAuthority::attached(scope(), "https://example.com/page", 1).unwrap()
}

fn cdp(method: &str, params: serde_json::Value) -> BrowserRequest {
    BrowserRequest {
        scope: scope(),
        command: BrowserCommand::Cdp {
            method: method.into(),
            params: params.as_object().cloned().unwrap_or_else(Map::new),
        },
    }
}

fn evaluate() -> BrowserRequest {
    cdp("Runtime.evaluate", json!({"expression": "1 + 1"}))
}

fn gate(
    port: Port,
    policy: &Arc<Policy>,
    choice: BrowserPermissionChoice,
    asked: Arc<AtomicUsize>,
) -> BrowserPermissionGate<Port, impl BrowserPermissionDecider> {
    BrowserPermissionGate::new(
        port,
        move |prompt: &BrowserPermissionPrompt| {
            asked.fetch_add(1, Ordering::SeqCst);
            assert_eq!(prompt.operation, "cdp");
            assert_eq!(prompt.origin, "https://example.com");
            choice
        },
        authority(),
    )
    .with_developer_mode(policy.clone())
}

#[test]
fn without_a_policy_or_with_the_switch_off_nothing_asks_or_runs() {
    let port = Port::default();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut no_policy = BrowserPermissionGate::new(
        port.clone(),
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowDeveloperMode,
        authority(),
    );
    assert_eq!(no_policy.perform(&evaluate()), Err(BrowserFailure::Denied));
    let off = Arc::new(Policy::default());
    let mut gate = gate(
        port.clone(),
        &off,
        BrowserPermissionChoice::AllowDeveloperMode,
        asked.clone(),
    );
    assert_eq!(gate.perform(&evaluate()), Err(BrowserFailure::Denied));
    assert_eq!(asked.load(Ordering::SeqCst), 0);
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
    assert_eq!(off.outcomes(), ["denied"]);
}

#[test]
fn approval_grants_the_session_so_later_calls_run_without_a_card() {
    let port = Port::default();
    let policy = Policy::on();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut gate = gate(
        port.clone(),
        &policy,
        BrowserPermissionChoice::AllowDeveloperMode,
        asked.clone(),
    );
    for _ in 0..3 {
        assert_eq!(
            gate.perform(&evaluate()),
            Ok(BrowserResult::Cdp {
                result: json!({"result": {"type": "number", "value": 2}})
            })
        );
    }
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(port.0.load(Ordering::SeqCst), 3);
    assert!(policy.granted("session-1", "https://example.com"));
    assert_eq!(
        policy.outcomes(),
        ["sent", "ok", "sent", "ok", "sent", "ok"]
    );
    let audit = policy.audit.lock().unwrap();
    assert!(audit.iter().all(|entry| entry.method == "Runtime.evaluate"
        && entry.origin == "https://example.com"
        && entry.tab_id == 7
        && entry.params.contains("1 + 1")));
    assert!(
        audit
            .iter()
            .all(|entry| entry.result_bytes.is_some() == (entry.outcome == CdpAuditOutcome::Ok))
    );
}

#[test]
fn only_allow_developer_mode_admits_an_elevated_command() {
    for choice in [
        BrowserPermissionChoice::Deny,
        BrowserPermissionChoice::AllowOnce,
        BrowserPermissionChoice::AllowForOriginSession,
    ] {
        let port = Port::default();
        let policy = Policy::on();
        let mut gate = gate(port.clone(), &policy, choice, Arc::default());
        assert_eq!(
            gate.perform(&evaluate()),
            Err(BrowserFailure::Denied),
            "{choice:?}"
        );
        assert_eq!(port.0.load(Ordering::SeqCst), 0);
        assert!(!policy.granted("session-1", "https://example.com"));
        assert_eq!(policy.outcomes(), ["denied"]);
    }
}

#[test]
fn an_ordinary_origin_grant_never_covers_cdp_and_developer_mode_never_covers_act() {
    let port = Port::default();
    let policy = Policy::on();
    let asked = Arc::new(AtomicUsize::new(0));
    let authority = authority();
    authority.0.lock().unwrap().grant_origin = Some("https://example.com".into());
    let mut gate = BrowserPermissionGate::new(
        port.clone(),
        {
            let asked = asked.clone();
            move |_: &BrowserPermissionPrompt| {
                asked.fetch_add(1, Ordering::SeqCst);
                BrowserPermissionChoice::AllowDeveloperMode
            }
        },
        authority,
    )
    .with_developer_mode(policy.clone());
    assert!(gate.perform(&evaluate()).is_ok());
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the origin grant did not cover cdp"
    );
    let snapshot = BrowserRequest {
        scope: scope(),
        command: BrowserCommand::SnapshotPage {},
    };
    gate.authority.0.lock().unwrap().grant_origin = None;
    assert_eq!(gate.perform(&snapshot), Err(BrowserFailure::Denied));
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "developer mode did not cover snapshotPage"
    );
}

#[test]
fn blocked_commands_are_refused_before_any_card_and_audited() {
    let port = Port::default();
    let policy = Policy::on();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut gate = gate(
        port.clone(),
        &policy,
        BrowserPermissionChoice::AllowDeveloperMode,
        asked.clone(),
    );
    for request in [
        cdp("Target.createTarget", json!({"url": "https://example.com"})),
        cdp("Network.getAllCookies", json!({})),
        cdp(
            "Network.getCookies",
            json!({"urls": ["https://bank.example"]}),
        ),
    ] {
        assert_eq!(gate.perform(&request), Err(BrowserFailure::Denied));
    }
    assert_eq!(asked.load(Ordering::SeqCst), 0);
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
    assert_eq!(policy.outcomes(), ["blocked", "blocked", "blocked"]);
}

#[test]
fn a_grant_the_attachment_refuses_denies_the_call() {
    let port = Port::default();
    let policy = Policy::on();
    policy.refuse_grant.store(true, Ordering::SeqCst);
    let mut gate = gate(
        port.clone(),
        &policy,
        BrowserPermissionChoice::AllowDeveloperMode,
        Arc::default(),
    );
    assert_eq!(gate.perform(&evaluate()), Err(BrowserFailure::Denied));
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
}

#[test]
fn a_stopped_authority_or_a_switch_turned_off_ends_a_granted_session() {
    let port = Port::default();
    let policy = Policy::on();
    let mut gate = gate(
        port.clone(),
        &policy,
        BrowserPermissionChoice::AllowDeveloperMode,
        Arc::default(),
    );
    assert!(gate.perform(&evaluate()).is_ok());
    policy.enabled.store(false, Ordering::SeqCst);
    assert_eq!(gate.perform(&evaluate()), Err(BrowserFailure::Denied));
    policy.enabled.store(true, Ordering::SeqCst);
    gate.authority.stop();
    assert_eq!(gate.perform(&evaluate()), Err(BrowserFailure::Denied));
    assert_eq!(port.0.load(Ordering::SeqCst), 1);
    assert_eq!(policy.outcomes(), ["sent", "ok", "denied", "denied"]);
}

#[test]
fn turning_the_switch_off_while_the_card_is_open_refuses_the_approval() {
    let port = Port::default();
    let policy = Policy::on();
    let mut gate = BrowserPermissionGate::new(
        port.clone(),
        {
            let policy = policy.clone();
            move |_: &BrowserPermissionPrompt| {
                policy.enabled.store(false, Ordering::SeqCst);
                BrowserPermissionChoice::AllowDeveloperMode
            }
        },
        authority(),
    )
    .with_developer_mode(policy.clone());
    assert_eq!(gate.perform(&evaluate()), Err(BrowserFailure::Denied));
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
    assert!(!policy.granted("session-1", "https://example.com"));
    assert_eq!(policy.outcomes(), ["denied"]);
}

#[test]
fn a_command_whose_audit_row_cannot_be_written_is_not_sent() {
    let port = Port::default();
    let policy = Policy::on();
    policy.refuse_audit.store(true, Ordering::SeqCst);
    let mut gate = gate(
        port.clone(),
        &policy,
        BrowserPermissionChoice::AllowDeveloperMode,
        Arc::default(),
    );
    assert_eq!(gate.perform(&evaluate()), Err(BrowserFailure::Denied));
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
}

/// Turns the switch off, or ends the grant, after the gate's checks and
/// the `sent` row but before the native call issues.
struct RevokeBeforeIssue {
    policy: Arc<Policy>,
    end_grant: bool,
    issued: Arc<AtomicUsize>,
}

impl BrowserCapabilityDispatch for RevokeBeforeIssue {
    fn dispatch(
        &mut self,
        _request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        if self.end_grant {
            *self.policy.grant.lock().unwrap() = None;
        } else {
            self.policy.enabled.store(false, Ordering::SeqCst);
        }
        let result = permit.with_live_lock(|| {
            self.issued.fetch_add(1, Ordering::SeqCst);
            Ok(BrowserResult::Cdp { result: json!({}) })
        })?;
        Ok(Box::new(move || Ok(result)))
    }
}

#[test]
fn switch_off_or_a_grant_ending_after_the_checks_still_stops_native_issue() {
    for end_grant in [false, true] {
        let policy = Policy::on();
        let issued = Arc::new(AtomicUsize::new(0));
        let mut gate = BrowserPermissionGate::new(
            RevokeBeforeIssue {
                policy: policy.clone(),
                end_grant,
                issued: issued.clone(),
            },
            |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowDeveloperMode,
            authority(),
        )
        .with_developer_mode(policy.clone());
        assert_eq!(
            gate.perform(&evaluate()),
            Err(BrowserFailure::Denied),
            "end_grant={end_grant}"
        );
        assert_eq!(issued.load(Ordering::SeqCst), 0, "end_grant={end_grant}");
        assert_eq!(
            policy.outcomes(),
            ["sent", "denied"],
            "end_grant={end_grant}"
        );
    }
}

#[test]
fn navigation_cannot_keep_an_elevated_call_alive_after_revocation() {
    struct NavigateThenRevoke {
        authority: BrowserAuthority,
        policy: Arc<Policy>,
    }
    impl BrowserCapabilityDispatch for NavigateThenRevoke {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            permit.with_live_lock(|| {
                permit.mark_issued();
                Ok(())
            })?;
            self.authority.navigation_started();
            assert!(permit.was_invalidated_by_navigation());
            self.policy.enabled.store(false, Ordering::SeqCst);
            assert!(!permit.is_live());
            assert!(
                !permit.was_invalidated_by_navigation(),
                "revocation ends the navigation exemption"
            );
            Ok(Box::new(move || Err(permit.revoked_failure())))
        }
    }
    let authority = authority();
    let policy = Policy::on();
    let mut gate = BrowserPermissionGate::new(
        NavigateThenRevoke {
            authority: authority.clone(),
            policy: policy.clone(),
        },
        |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowDeveloperMode,
        authority,
    )
    .with_developer_mode(policy);
    assert_eq!(
        gate.perform(&evaluate()),
        Err(BrowserFailure::OutcomeUnknown)
    );
}
