use crate::acp::{
    bindings::characterisation::{Characterisation, valid_numeric_version},
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::manifest::CuratedCapabilityCeiling;

const VERSION_TOKEN_BYTES_MAX: usize = 32;
/// The debug desktop fixture's identity. It replays the observed 0.146 wire,
/// so its sessions carry the 0.146 capability evidence under their own name.
const DESKTOP_FIXTURE_PROTOCOL_IDENTITY: &str = "codex-app-server-desktop-fixture-0.146.0";

/// Whether a stored session identity is the debug desktop fixture's. Always
/// false outside a Windows debug `acp-fixture` build, where no such session
/// can be activated.
pub(crate) fn is_desktop_fixture_protocol(identity: &str) -> bool {
    cfg!(all(windows, feature = "acp-fixture", debug_assertions))
        && identity == DESKTOP_FIXTURE_PROTOCOL_IDENTITY
}

#[derive(Debug, Clone, Copy, Default)]
struct OptionalCapabilities {
    resume_session: bool,
    load_session: bool,
    close_session: bool,
    cancel: bool,
    active_turn_steer: bool,
    model_list: bool,
    child_conversation_projection: bool,
    child_transcript_read: bool,
    collaboration_event_projection: bool,
    native_session_fork: bool,
}

#[derive(Debug, Clone, Copy)]
struct ObservedCapabilityEvidence {
    version: &'static str,
    optional: OptionalCapabilities,
}

const OBSERVED_CAPABILITY_EVIDENCE: [ObservedCapabilityEvidence; 1] =
    [ObservedCapabilityEvidence {
        version: "0.146.0",
        optional: OptionalCapabilities {
            resume_session: true,
            load_session: false,
            close_session: false,
            cancel: true,
            active_turn_steer: true,
            model_list: true,
            child_conversation_projection: false,
            child_transcript_read: true,
            collaboration_event_projection: false,
            // `thread/fork` params and response are pinned from the 0.146
            // generated schema (testdata/acp/codex/observed-0146-fork-schema.json).
            native_session_fork: true,
        },
    }];

#[derive(Debug, Clone, Copy)]
pub(crate) struct ObservedBuild<'a> {
    pub(crate) reported_version: &'a str,
    pub(crate) characterisation: Characterisation,
}

fn observed_protocol_identity(reported_version: &str) -> String {
    let token = observed_version(reported_version).unwrap_or("unreadable");
    format!("codex-app-server-observed-{token}")
}

fn observed_version(reported_version: &str) -> Option<&str> {
    reported_version
        .split_ascii_whitespace()
        .next_back()
        .filter(|token| {
            token.len() <= VERSION_TOKEN_BYTES_MAX && valid_numeric_version(token, 3, 3)
        })
}

fn observed_capability_evidence(build: ObservedBuild<'_>) -> OptionalCapabilities {
    if !build.characterisation.is_characterised() {
        return OptionalCapabilities::default();
    }
    observed_version(build.reported_version)
        .and_then(|version| {
            OBSERVED_CAPABILITY_EVIDENCE
                .iter()
                .find(|evidence| evidence.version == version)
        })
        .map(|evidence| evidence.optional)
        .unwrap_or_default()
}

fn managed_capability_evidence(ceiling: &CuratedCapabilityCeiling) -> OptionalCapabilities {
    OptionalCapabilities {
        resume_session: ceiling.resume_session,
        load_session: ceiling.load_session,
        close_session: ceiling.close_session,
        cancel: ceiling.cancel,
        active_turn_steer: true,
        model_list: ceiling.model_list,
        child_conversation_projection: ceiling.child_conversation_projection,
        child_transcript_read: true,
        collaboration_event_projection: ceiling.collaboration_event_projection,
        native_session_fork: ceiling.protocol_identity == super::manifest::PINNED_PROTOCOL_IDENTITY,
    }
}

pub(crate) fn projected_capabilities(
    ceiling: &CuratedCapabilityCeiling,
    observed: Option<ObservedBuild<'_>>,
) -> Result<(CapabilitySnapshotV1, bool), ProjectionUnavailable> {
    if !(ceiling.new_session && ceiling.prompt && ceiling.cancel && ceiling.close_session) {
        return Err(ProjectionUnavailable::MandatorySessionCore);
    }
    let (identity, optional) = match observed {
        None => (
            ceiling.protocol_identity.clone(),
            managed_capability_evidence(ceiling),
        ),
        Some(build) => (
            observed_protocol_identity(build.reported_version),
            observed_capability_evidence(build),
        ),
    };
    let snapshot = CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", &identity)
            .map_err(|_| ProjectionUnavailable::Identity)?,
        new_session: ceiling.new_session,
        prompt: ceiling.prompt,
        cancel: optional.cancel,
        close_session: optional.close_session,
        resume_session: optional.resume_session,
        load_session: optional.load_session,
        child_conversation_projection: optional.child_conversation_projection,
        collaboration_event_projection: optional.collaboration_event_projection,
        native_session_fork: optional.native_session_fork,
        active_turn_steer: optional.active_turn_steer,
        child_transcript_read: optional.child_transcript_read,
        multi_root_extension_schema_id: None,
        model_count: 0,
        config_count: 0,
    };
    Ok((snapshot, optional.model_list))
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) fn projected_desktop_fixture_capabilities(
    ceiling: &CuratedCapabilityCeiling,
) -> Result<CapabilitySnapshotV1, ProjectionUnavailable> {
    let (mut snapshot, _) = projected_capabilities(
        ceiling,
        Some(ObservedBuild {
            reported_version: "0.146.0",
            characterisation: Characterisation::Characterised,
        }),
    )?;
    snapshot.protocol_version =
        RequiredText::new("protocolVersion", DESKTOP_FIXTURE_PROTOCOL_IDENTITY)
            .map_err(|_| ProjectionUnavailable::Identity)?;
    Ok(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProjectionUnavailable {
    #[error("the projected Codex protocol identity is not representable")]
    Identity,
    #[error("the projected Codex capability ceiling omits a mandatory session operation")]
    MandatorySessionCore,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned() -> CuratedCapabilityCeiling {
        CuratedCapabilityCeiling {
            protocol_identity: super::super::manifest::PINNED_PROTOCOL_IDENTITY.into(),
            new_session: true,
            resume_session: true,
            load_session: true,
            close_session: true,
            prompt: true,
            cancel: true,
            model_list: true,
            child_conversation_projection: true,
            collaboration_event_projection: true,
        }
    }

    #[test]
    fn an_observed_build_never_borrows_the_pinned_protocol_identity() {
        let (snapshot, _) = projected_capabilities(
            &pinned(),
            Some(ObservedBuild {
                reported_version: "codex-cli 0.146.0",
                characterisation: Characterisation::Characterised,
            }),
        )
        .unwrap();
        assert_eq!(
            snapshot.protocol_version.as_str(),
            "codex-app-server-observed-0.146.0"
        );
        assert_ne!(
            snapshot.protocol_version.as_str(),
            super::super::manifest::PINNED_PROTOCOL_IDENTITY
        );
    }

    #[test]
    fn managed_0146_admits_steer_and_child_transcript_after_exact_pin_admission() {
        let (snapshot, models) = projected_capabilities(&pinned(), None).unwrap();
        assert_eq!(
            snapshot.protocol_version.as_str(),
            super::super::manifest::PINNED_PROTOCOL_IDENTITY
        );
        assert!(models);
        assert!(snapshot.resume_session && snapshot.collaboration_event_projection);
        assert!(snapshot.active_turn_steer);
        assert!(snapshot.child_transcript_read);
    }

    #[test]
    fn an_uncharacterised_build_can_start_and_prompt_without_claiming_cancel_or_close() {
        let (snapshot, models) = projected_capabilities(
            &pinned(),
            Some(ObservedBuild {
                reported_version: "0.999.0",
                characterisation: Characterisation::Uncharacterised,
            }),
        )
        .unwrap();
        assert!(snapshot.new_session && snapshot.prompt);
        assert!(!snapshot.cancel);
        assert!(!snapshot.close_session);
        assert!(!snapshot.resume_session);
        assert!(!snapshot.load_session);
        assert!(!snapshot.child_conversation_projection);
        assert!(!snapshot.collaboration_event_projection);
        assert!(!snapshot.child_transcript_read);
        assert!(!models);
        assert_eq!(
            snapshot.protocol_version.as_str(),
            "codex-app-server-observed-0.999.0"
        );
    }

    #[test]
    fn a_capability_ceiling_without_the_mandatory_session_core_is_refused() {
        let mut ceiling = pinned();
        ceiling.close_session = false;
        assert_eq!(
            projected_capabilities(&ceiling, None),
            Err(ProjectionUnavailable::MandatorySessionCore)
        );
    }

    #[test]
    fn observed_0146_projects_only_its_live_evidence() {
        let (snapshot, models) = projected_capabilities(
            &pinned(),
            Some(ObservedBuild {
                reported_version: "codex-cli 0.146.0",
                characterisation: Characterisation::Characterised,
            }),
        )
        .unwrap();
        assert!(snapshot.new_session && snapshot.prompt);
        assert!(snapshot.cancel);
        assert!(snapshot.active_turn_steer);
        assert!(!snapshot.close_session);
        assert!(snapshot.resume_session);
        assert!(!snapshot.load_session);
        assert!(!snapshot.child_conversation_projection);
        assert!(!snapshot.collaboration_event_projection);
        assert!(snapshot.child_transcript_read);
        assert!(snapshot.native_session_fork);
        assert!(models);
    }

    #[test]
    fn native_fork_is_admitted_only_for_the_pinned_or_characterised_0146_build() {
        let (managed, _) = projected_capabilities(&pinned(), None).unwrap();
        assert!(managed.native_session_fork);
        let mut other = pinned();
        other.protocol_identity = "codex-app-server-0.147.0-00000000".into();
        let (unpinned, _) = projected_capabilities(&other, None).unwrap();
        assert!(!unpinned.native_session_fork);
        for (reported, characterisation) in [
            ("codex-cli 0.146.0", Characterisation::Uncharacterised),
            ("codex-cli 0.147.0", Characterisation::Characterised),
        ] {
            let (observed, _) = projected_capabilities(
                &pinned(),
                Some(ObservedBuild {
                    reported_version: reported,
                    characterisation,
                }),
            )
            .unwrap();
            assert!(!observed.native_session_fork, "{reported}");
        }
    }

    #[test]
    fn observed_0145_without_live_optional_evidence_keeps_only_the_session_core() {
        let (snapshot, models) = projected_capabilities(
            &pinned(),
            Some(ObservedBuild {
                reported_version: "codex-cli 0.145.0",
                characterisation: Characterisation::Uncharacterised,
            }),
        )
        .unwrap();
        assert!(snapshot.new_session && snapshot.prompt);
        assert!(!snapshot.cancel);
        assert!(!snapshot.active_turn_steer);
        assert!(!snapshot.close_session);
        assert!(!snapshot.resume_session);
        assert!(!snapshot.load_session);
        assert!(!snapshot.child_conversation_projection);
        assert!(!snapshot.collaboration_event_projection);
        assert!(!snapshot.child_transcript_read);
        assert!(!snapshot.native_session_fork);
        assert!(!models);
    }

    #[test]
    fn only_the_debug_desktop_fixture_identity_counts_as_the_fixture_protocol() {
        assert!(!is_desktop_fixture_protocol(
            "codex-app-server-observed-0.146.0"
        ));
        assert!(!is_desktop_fixture_protocol(
            "codex-app-server-desktop-fixture-0.147.0"
        ));
        assert_eq!(
            is_desktop_fixture_protocol(DESKTOP_FIXTURE_PROTOCOL_IDENTITY),
            cfg!(all(windows, feature = "acp-fixture", debug_assertions))
        );
    }

    #[test]
    fn an_unreadable_version_is_named_as_unreadable_rather_than_guessed() {
        for reported in ["", "   ", "0.146.0\u{7}", &"9".repeat(64)] {
            let (snapshot, _) = projected_capabilities(
                &pinned(),
                Some(ObservedBuild {
                    reported_version: reported,
                    characterisation: Characterisation::Uncharacterised,
                }),
            )
            .unwrap();
            assert_eq!(
                snapshot.protocol_version.as_str(),
                "codex-app-server-observed-unreadable"
            );
        }
    }
}
