//! Classifies the exact build a session last ran under against the build
//! admitted now. All decisions come from curated binding metadata: the
//! compatibility family table and the recorded build, never a version range.

use crate::acp::{
    bindings::{compatibility::same_family, registry::DriftLineage},
    contract::SafeText,
    store::{
        ActivateSessionCommand, BindingChange, BindingChangeKind, BindingRefusalReason,
        RecordedBuild, RefuseSessionBindingCommand, recorded_build,
    },
};

use super::CodexSessionError;

/// The build admitted for this activation.
pub(super) struct AdmittedBuild<'a> {
    pub(super) binding_id: &'a str,
    pub(super) digest: &'a str,
    pub(super) harness_version: &'a str,
    pub(super) artifact_digest: &'a str,
    /// `None` for a binding whose sessions never continue across a change.
    pub(super) lineage: Option<DriftLineage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingDrift {
    /// The exact build the session last ran under.
    Same,
    /// A proven-compatible change: continue, then record it with a notice.
    Continue(BindingChangeKind),
    /// The same harness, but this build cannot resume the session.
    Refuse(BindingRefusalReason),
    /// Another harness, or a build change on a binding without lineage.
    Foreign,
}

pub(super) fn classify(
    session_binding_id: &str,
    recorded: &RecordedBuild,
    admitted: &AdmittedBuild<'_>,
) -> BindingDrift {
    let same_version = recorded.harness_version.as_deref() == Some(admitted.harness_version);
    if session_binding_id != admitted.binding_id {
        return BindingDrift::Foreign;
    }
    if recorded.digest == admitted.digest {
        return if same_version {
            BindingDrift::Same
        } else {
            BindingDrift::Foreign
        };
    }
    let Some(lineage) = admitted.lineage else {
        return BindingDrift::Foreign;
    };
    // The binding digest covers the harness version, the artifact and the
    // runtime. With the first two unchanged, only the runtime moved.
    if same_version && recorded.artifact_digest.as_deref() == Some(admitted.artifact_digest) {
        return BindingDrift::Continue(BindingChangeKind::NodeOnly);
    }
    let recorded_family = recorded
        .harness_version
        .as_deref()
        .and_then(lineage.recorded);
    match (recorded_family, lineage.admitted) {
        (Some(recorded), Some(admitted)) if same_family(recorded, admitted) => {
            BindingDrift::Continue(BindingChangeKind::CompatibleAdapter)
        }
        (Some(_), Some(_)) => BindingDrift::Refuse(BindingRefusalReason::OtherFamily),
        _ => BindingDrift::Refuse(BindingRefusalReason::UnknownBuild),
    }
}

/// The binding an activation runs under: the session's durable write scope,
/// and the compatible drift the activation must prove, if any.
pub(super) struct ActivationBinding {
    pub(super) scope: [u8; 32],
    pub(super) change: Option<BindingChange>,
}

/// Resolves an activation against the session's recorded build. A refusal
/// carries the durable command that ends the session; the caller commits it.
pub(super) fn resolve_activation_binding(
    connection: &rusqlite::Connection,
    session_binding_id: &str,
    session_scope: &str,
    admitted: &AdmittedBuild<'_>,
    command: &ActivateSessionCommand,
) -> Result<ActivationBinding, CodexSessionError> {
    let recorded =
        recorded_build(connection, &command.session_id).map_err(|_| CodexSessionError::Intent)?;
    let scope = decode_digest(session_scope).ok_or(CodexSessionError::Intent)?;
    let version =
        |value: &str| SafeText::new("harnessVersion", value).map_err(|_| CodexSessionError::Intent);
    let from_version = recorded
        .harness_version
        .as_deref()
        .map(version)
        .transpose()?;
    let to_version = version(admitted.harness_version)?;
    let kind = match classify(session_binding_id, &recorded, admitted) {
        BindingDrift::Same => {
            return Ok(ActivationBinding {
                scope,
                change: None,
            });
        }
        BindingDrift::Foreign => return Err(CodexSessionError::Intent),
        BindingDrift::Refuse(reason) => {
            return Err(CodexSessionError::BindingIncompatible(Box::new(
                RefuseSessionBindingCommand {
                    project_id: command.project_id.clone(),
                    chat_id: command.chat_id.clone(),
                    session_id: command.session_id.clone(),
                    expected_projection_revision: command.expected_projection_revision,
                    client_intent_id: command.client_intent_id.clone(),
                    reason,
                    from_version,
                    to_version,
                    now_millis: crate::persistence::now_millis(),
                },
            )));
        }
        BindingDrift::Continue(kind) => kind,
    };
    Ok(ActivationBinding {
        scope,
        change: Some(BindingChange {
            kind,
            from_digest: recorded.digest,
            to_digest: admitted.digest.to_owned(),
            to_artifact: admitted.artifact_digest.to_owned(),
            from_version,
            to_version,
        }),
    })
}

fn decode_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::bindings::compatibility::{
        CompatibilityFamily, claude_build_family, claude_recorded_family,
    };

    const CLAUDE: &str = "claude-agent-acp";
    const OLD_DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const NEW_DIGEST: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const OLD_ARTIFACT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    const NEW_ARTIFACT: &str = "4444444444444444444444444444444444444444444444444444444444444444";

    fn recorded(version: &str, artifact: Option<&str>) -> RecordedBuild {
        RecordedBuild {
            digest: OLD_DIGEST.into(),
            harness_version: Some(version.into()),
            artifact_digest: artifact.map(Into::into),
        }
    }

    fn claude<'a>(
        digest: &'a str,
        version: &'a str,
        sdk: &str,
        artifact: &'a str,
    ) -> AdmittedBuild<'a> {
        AdmittedBuild {
            binding_id: CLAUDE,
            digest,
            harness_version: version,
            artifact_digest: artifact,
            lineage: Some(DriftLineage {
                admitted: claude_build_family(version, sdk),
                recorded: claude_recorded_family,
            }),
        }
    }

    #[test]
    fn an_unchanged_build_is_the_same_binding() {
        let admitted = claude(OLD_DIGEST, "0.76.0", "1.4.0", OLD_ARTIFACT);
        let session = recorded("0.76.0", Some(OLD_ARTIFACT));
        assert_eq!(classify(CLAUDE, &session, &admitted), BindingDrift::Same);
    }

    #[test]
    fn a_node_update_alone_continues_the_session() {
        let admitted = claude(NEW_DIGEST, "0.101.0", "2.4.0", OLD_ARTIFACT);
        let session = recorded("0.101.0", Some(OLD_ARTIFACT));
        assert_eq!(
            classify(CLAUDE, &session, &admitted),
            BindingDrift::Continue(BindingChangeKind::NodeOnly)
        );
    }

    #[test]
    fn a_characterised_adapter_in_the_same_family_continues() {
        let admitted = claude(NEW_DIGEST, "0.101.0", "2.4.0", NEW_ARTIFACT);
        let session = recorded("0.76.0", Some(OLD_ARTIFACT));
        assert_eq!(
            classify(CLAUDE, &session, &admitted),
            BindingDrift::Continue(BindingChangeKind::CompatibleAdapter)
        );
    }

    #[test]
    fn an_uncharacterised_adapter_is_refused_not_widened() {
        for (version, sdk) in [
            ("0.101.1", "2.4.0"),
            ("0.102.0", "2.4.0"),
            ("0.101.0", "2.5.0"),
        ] {
            let admitted = claude(NEW_DIGEST, version, sdk, NEW_ARTIFACT);
            let session = recorded("0.101.0", Some(OLD_ARTIFACT));
            assert_eq!(
                classify(CLAUDE, &session, &admitted),
                BindingDrift::Refuse(BindingRefusalReason::UnknownBuild),
                "{version}+{sdk}"
            );
        }
    }

    #[test]
    fn a_session_recorded_on_an_unknown_build_is_refused() {
        let admitted = claude(NEW_DIGEST, "0.101.0", "2.4.0", NEW_ARTIFACT);
        let session = recorded("0.90.0", Some(OLD_ARTIFACT));
        assert_eq!(
            classify(CLAUDE, &session, &admitted),
            BindingDrift::Refuse(BindingRefusalReason::UnknownBuild)
        );
    }

    #[test]
    fn a_build_in_another_family_is_refused_as_incompatible() {
        const OTHER: CompatibilityFamily =
            CompatibilityFamily::new("claude-acp-v2/claude-transcript-2", 2);
        let admitted = AdmittedBuild {
            lineage: Some(DriftLineage {
                admitted: Some(OTHER),
                recorded: claude_recorded_family,
            }),
            ..claude(NEW_DIGEST, "1.0.0", "3.0.0", NEW_ARTIFACT)
        };
        let session = recorded("0.101.0", Some(OLD_ARTIFACT));
        assert_eq!(
            classify(CLAUDE, &session, &admitted),
            BindingDrift::Refuse(BindingRefusalReason::OtherFamily)
        );
    }

    #[test]
    fn another_harness_or_a_binding_without_lineage_is_foreign() {
        let admitted = claude(NEW_DIGEST, "0.101.0", "2.4.0", NEW_ARTIFACT);
        let session = recorded("0.76.0", Some(OLD_ARTIFACT));
        assert_eq!(
            classify("codex-app-server", &session, &admitted),
            BindingDrift::Foreign
        );
        let without_lineage = AdmittedBuild {
            lineage: None,
            ..claude(NEW_DIGEST, "0.76.0", "1.4.0", OLD_ARTIFACT)
        };
        assert_eq!(
            classify(
                CLAUDE,
                &recorded("0.76.0", Some(OLD_ARTIFACT)),
                &without_lineage
            ),
            BindingDrift::Foreign
        );
    }
}
