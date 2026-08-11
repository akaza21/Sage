use std::path::Path;

use crate::acp::bindings::codex::policy::TEMPORARY_RESIDUE_RETENTION_HOURS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResidueMarker(String);

impl ResidueMarker {
    pub(crate) fn parse(value: &str) -> Result<Self, RetentionError> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RetentionError::Marker);
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetentionDecision {
    Protected,
    ReclaimableResidue,
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RetentionError {
    #[error("Codex residue marker is not exact lowercase hexadecimal identity")]
    Marker,
}

/// Classifies state without deleting it. Unknown and fresh paths are preserved.
pub(crate) fn classify_codex_state_path(
    relative_path: &Path,
    marker: &ResidueMarker,
    age_hours: u64,
) -> RetentionDecision {
    let Some(relative) = relative_path.to_str() else {
        return RetentionDecision::Preserve;
    };
    let relative = relative.replace('/', "\\");
    if protected(&relative) {
        return RetentionDecision::Protected;
    }
    let login = format!(r"tmp\sage-login-{}.tmp", marker.0);
    let runtime = format!(r"tmp\sage-runtime-{}.tmp", marker.0);
    if age_hours >= u64::from(TEMPORARY_RESIDUE_RETENTION_HOURS)
        && matches!(relative.as_str(), value if value == login || value == runtime)
    {
        RetentionDecision::ReclaimableResidue
    } else {
        RetentionDecision::Preserve
    }
}

fn protected(relative: &str) -> bool {
    matches!(
        relative,
        "auth.json" | "config.toml" | "history.jsonl" | "session_index.jsonl"
    ) || relative.starts_with(r"sessions\")
        || relative.starts_with(r"archived_sessions\")
        || sqlite_state(relative)
}

fn sqlite_state(relative: &str) -> bool {
    let Some(name) = relative.strip_suffix(".sqlite").or_else(|| {
        relative
            .strip_suffix(".sqlite-wal")
            .or_else(|| relative.strip_suffix(".sqlite-shm"))
    }) else {
        return false;
    };
    ["state_", "logs_", "goals_", "memories_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}
