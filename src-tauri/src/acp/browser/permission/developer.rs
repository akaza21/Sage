//! The elevated path of the gate (ADR-0009b): a raw CDP command needs the
//! Settings switch, a live authority and the deny list before it may ask,
//! and every attempt is audited, whatever its outcome.

use serde_json::{Map, Value};

use super::{
    BrowserCapabilityDispatch, BrowserFailure, BrowserPermissionDecider, BrowserPermissionGate,
    BrowserRequest, BrowserResult,
};
use crate::acp::browser::developer::{CdpAuditEntry, CdpAuditOutcome, admit_command};

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
        let outcome =
            self.perform_admitted(request, Some((developer.as_ref(), &origin, &record_sent)));
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
