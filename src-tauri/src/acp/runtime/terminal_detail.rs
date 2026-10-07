use crate::acp::{process::TreeExit, redaction::sanitize_diagnostic_text};

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
