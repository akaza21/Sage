use crate::acp::{process::TreeExit, redaction::sanitize_diagnostic_text};
use sage_acp_protocol::lifecycle::{TurnTerminalOutcome, TurnTerminalReason};

const DETAIL_CHARS: usize = 512;

pub(crate) fn harness_exited(exit: Option<TreeExit>) -> Option<String> {
    let detail = match exit? {
        TreeExit::Exited { code: Some(code) } => format!("harness process exited with code {code}"),
        TreeExit::Exited { code: None } => {
            "harness process exited without reporting an exit code".to_owned()
        }
        TreeExit::Cancelled => "harness process was terminated by Sage".to_owned(),
        TreeExit::Deadline => "harness process exceeded its shutdown deadline".to_owned(),
        TreeExit::IdentityMismatch => {
            "harness process identity did not match this session".to_owned()
        }
        TreeExit::ContainmentFailure => "harness process containment failed".to_owned(),
        TreeExit::ProtocolFailure => "harness process stream failed before a clean exit".to_owned(),
        TreeExit::ParentShutdown => {
            "harness process stopped while Sage was shutting down".to_owned()
        }
    };
    bound(detail)
}

/// The runner can stop on a stream failure before the root process's exit is
/// known; the exit observed afterwards still carries the real exit code.
pub(crate) fn observed_exit(runner: TreeExit, root: TreeExit) -> TreeExit {
    match (runner, root) {
        (TreeExit::ProtocolFailure | TreeExit::Exited { code: None }, TreeExit::Exited { .. }) => {
            root
        }
        _ => runner,
    }
}

/// Why a turn ended when its runtime died or was torn down under it. A cause
/// the driver already classified wins over the bare tree exit; the harness's
/// own error, when it reported one, follows Sage's description of the exit.
/// Recovery fails the turn, so a Sage-requested teardown is never reported
/// as a user cancellation.
pub(crate) fn runtime_lost(
    exit: TreeExit,
    teardown: Option<TurnTerminalOutcome>,
    harness_error: Option<&str>,
) -> TurnTerminalOutcome {
    let (reason, described) = match teardown.map(|outcome| (outcome.reason, outcome.message)) {
        Some((TurnTerminalReason::HarnessExited, _)) => {
            let code = if let TreeExit::Exited { code } = exit {
                code
            } else {
                None
            };
            (
                TurnTerminalReason::HarnessExited,
                harness_exited(Some(TreeExit::Exited { code })),
            )
        }
        Some((TurnTerminalReason::CancelledByUser, _)) => (
            TurnTerminalReason::Other,
            harness_exited(Some(TreeExit::Cancelled)),
        ),
        Some((reason, message)) => (reason, message.or_else(|| harness_exited(Some(exit)))),
        None => (exit_reason(exit), harness_exited(Some(exit))),
    };
    let harness_error = match exit {
        TreeExit::Cancelled | TreeExit::ParentShutdown => None,
        _ => sanitized(harness_error),
    };
    let message = match (described, harness_error) {
        (Some(described), Some(error)) => bound(format!("{described}: {error}")),
        (described, error) => described.or(error).and_then(bound),
    };
    TurnTerminalOutcome {
        reason,
        message,
        remediation: None,
    }
}

/// The cause recorded when Sage settles a runtime whose exit its runner did
/// not observe, so it can name neither an exit code nor a harness error.
pub(crate) fn runtime_stopped() -> TurnTerminalOutcome {
    TurnTerminalOutcome {
        reason: TurnTerminalReason::Other,
        message: Some("harness process stopped before this turn finished".to_owned()),
        remediation: None,
    }
}

/// The cause recorded on startup for a turn whose runtime outlived the
/// previous Sage process.
pub(crate) fn sage_closed() -> TurnTerminalOutcome {
    TurnTerminalOutcome {
        reason: TurnTerminalReason::Other,
        message: Some("Sage closed before this turn finished".to_owned()),
        remediation: None,
    }
}

fn exit_reason(exit: TreeExit) -> TurnTerminalReason {
    match exit {
        TreeExit::Exited { .. } => TurnTerminalReason::HarnessExited,
        TreeExit::ProtocolFailure => TurnTerminalReason::ProtocolError,
        TreeExit::Deadline => TurnTerminalReason::Timeout,
        TreeExit::Cancelled
        | TreeExit::ParentShutdown
        | TreeExit::IdentityMismatch
        | TreeExit::ContainmentFailure => TurnTerminalReason::Other,
    }
}

pub(crate) fn protocol_error(summary: &str) -> Option<String> {
    bound(sanitize_diagnostic_text(summary))
}

pub(crate) fn auth_required(message: Option<&str>) -> Option<String> {
    bound(sanitized(message)?)
}

pub(crate) fn remediation(value: Option<&'static str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    bound(value.to_owned())
}

pub(crate) fn refused_version(observed: &str, characterised: &[&str]) -> Option<String> {
    let observed = sanitized(Some(observed))?;
    let detail = if characterised.is_empty() {
        format!("harness reported protocol version {observed}, which Sage has not characterised")
    } else {
        format!(
            "harness reported protocol version {observed}, but Sage supports {}",
            characterised.join(", ")
        )
    };
    bound(detail)
}

fn sanitized(value: Option<&str>) -> Option<String> {
    let value = sanitize_diagnostic_text(value?.trim());
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

fn bound(detail: String) -> Option<String> {
    if detail.is_empty() {
        return None;
    }
    if detail.chars().count() <= DETAIL_CHARS {
        return Some(detail);
    }
    let mut bounded: String = detail.chars().take(DETAIL_CHARS - 1).collect();
    bounded.push('…');
    Some(bounded)
}

#[cfg(test)]
mod tests;
