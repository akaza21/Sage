use std::sync::Arc;

use serde_json::{Map, Value};

use super::{
    BrowserAuthority, BrowserCapabilityDispatch, BrowserFailure, BrowserPermissionDecider,
    BrowserPermissionGate, BrowserRequest, BrowserResult,
};
use crate::acp::browser::developer::{
    CdpAuditEntry, CdpAuditOutcome, DeveloperModePolicy, admit_command,
};

#[derive(Clone, Copy)]
pub(super) struct Elevated<'a> {
    pub(super) policy: &'a Arc<dyn DeveloperModePolicy>,
    pub(super) origin: &'a str,
    pub(super) record_sent: &'a dyn Fn() -> bool,
}

impl Elevated<'_> {
    /// After the card: the switch is read again (it may have gone off while
    /// the card was open), a new approval is recorded on the attachment (so
    /// Stop, Take over and a cross-origin commit end it whichever authority
    /// asked), and the `sent` row must be written before anything is sent.
    pub(super) fn admit(&self, session: &str, origin: &str, already_granted: bool) -> bool {
        self.policy.enabled()
            && (already_granted || self.policy.grant(session, origin))
            && (self.record_sent)()
    }

    /// Checked under the live lock as the native call issues.
    pub(super) fn issue_check(
        &self,
        session: &str,
        origin: &str,
    ) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let (policy, session, origin) =
            (self.policy.clone(), session.to_owned(), origin.to_owned());
        Arc::new(move || policy.enabled() && policy.granted(&session, &origin))
    }
}

impl BrowserAuthority {
    pub(crate) fn fence(&self) {
        drop(self.0.lock());
    }
}

impl<P, D> BrowserPermissionGate<P, D> {
    pub fn with_developer_mode(mut self, policy: Arc<dyn DeveloperModePolicy>) -> Self {
        self.developer = Some(policy);
        self
    }
}

impl<P: BrowserCapabilityDispatch, D: BrowserPermissionDecider> BrowserPermissionGate<P, D> {
    pub(super) fn perform_elevated(
        &mut self,
        request: &BrowserRequest,
        method: &str,
        params: &Map<String, Value>,
    ) -> Result<BrowserResult, BrowserFailure> {
        // Without a policy there is no switch, store or grant to consult;
        // the session capability audits that refusal (`attached.rs`).
        let developer = self.developer.clone().ok_or(BrowserFailure::Denied)?;
        let (origin, live) = {
            let state = self
                .authority
                .0
                .lock()
                .map_err(|_| BrowserFailure::Unavailable)?;
            (
                state.origin.clone(),
                state.active && state.scope == request.scope,
            )
        };
        let entry = |outcome: CdpAuditOutcome, result_bytes: Option<usize>| CdpAuditEntry {
            session_id: request.scope.session_id.clone(),
            tab_id: request.scope.tab_id,
            origin: origin.clone(),
            method: method.to_owned(),
            params: serde_json::to_string(params).unwrap_or_default(),
            result_bytes,
            outcome,
        };
        let audit = |outcome: CdpAuditOutcome, result_bytes: Option<usize>| {
            developer.audit(entry(outcome, result_bytes));
        };
        if !live || !developer.enabled() {
            audit(CdpAuditOutcome::Denied, None);
            return Err(BrowserFailure::Denied);
        }
        if let Err(failure) = admit_command(method, params, &origin) {
            audit(CdpAuditOutcome::Blocked, None);
            return Err(failure);
        }
        // Written after approval and before sending; if it cannot be
        // written, the command is not sent.
        let record_sent = || developer.audit(entry(CdpAuditOutcome::Sent, None));
        let outcome = self.perform_admitted(
            request,
            Some(Elevated {
                policy: &developer,
                origin: &origin,
                record_sent: &record_sent,
            }),
        );
        match &outcome {
            Ok(BrowserResult::Cdp { result }) => audit(
                CdpAuditOutcome::Ok,
                Some(serde_json::to_vec(result).map_or(0, |bytes| bytes.len())),
            ),
            Ok(_) => audit(CdpAuditOutcome::Failed(BrowserFailure::Unavailable), None),
            Err(BrowserFailure::Denied) => audit(CdpAuditOutcome::Denied, None),
            Err(failure) => audit(CdpAuditOutcome::Failed(*failure), None),
        }
        outcome
    }
}
