//! Curated compatibility families: which characterised harness builds can
//! continue each other's native sessions.
//!
//! A family names one native session lineage: the ACP protocol version a
//! build negotiates and the transcript format its native session ids refer
//! to. Two builds in the same family can resume each other's sessions; a
//! build in another family, or a build that is not listed at all, cannot.
//! The table is data. Nothing infers a family from a version range.

/// One native session lineage. The id is curated and never shown to users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompatibilityFamily {
    id: &'static str,
    /// The ACP `protocolVersion` every build in this family negotiates.
    acp_protocol: u16,
}

impl CompatibilityFamily {
    #[cfg(test)]
    pub(crate) const fn new(id: &'static str, acp_protocol: u16) -> Self {
        Self { id, acp_protocol }
    }
}

/// claude-agent-acp speaking ACP v1 over Claude Code transcript files.
const CLAUDE_ACP_V1: CompatibilityFamily = CompatibilityFamily {
    id: "claude-acp-v1/claude-transcript-1",
    acp_protocol: 1,
};

/// One characterised `(claude-agent-acp, @agentclientprotocol/sdk)` build.
struct ClaudeBuild {
    adapter: &'static str,
    acp_sdk: &'static str,
    family: CompatibilityFamily,
}

/// Exact Claude builds whose wire and behaviour Sage has characterised. A
/// build is admitted only as a whole pair: an adapter from one row with the
/// SDK from another row is uncharacterised. Each adapter version appears in
/// at most one row, so a recorded adapter version names one family.
const CLAUDE_BUILDS: [ClaudeBuild; 2] = [
    ClaudeBuild {
        adapter: "0.76.0",
        acp_sdk: "1.4.0",
        family: CLAUDE_ACP_V1,
    },
    ClaudeBuild {
        adapter: "0.101.0",
        acp_sdk: "2.4.0",
        family: CLAUDE_ACP_V1,
    },
];

/// The family of a characterised Claude build, or `None` when the exact
/// `(adapter, sdk)` pair is not in the table.
pub(crate) fn claude_build_family(adapter: &str, acp_sdk: &str) -> Option<CompatibilityFamily> {
    CLAUDE_BUILDS
        .iter()
        .find(|build| build.adapter == adapter && build.acp_sdk == acp_sdk)
        .map(|build| build.family)
}

/// The family of a Claude adapter version recorded on a session. Sessions
/// record only the adapter version; the table keeps that unambiguous.
pub(crate) fn claude_recorded_family(adapter: &str) -> Option<CompatibilityFamily> {
    CLAUDE_BUILDS
        .iter()
        .find(|build| build.adapter == adapter)
        .map(|build| build.family)
}

/// Whether a session from `recorded` can continue on `admitted`: the same
/// lineage and therefore the same negotiated ACP protocol.
pub(crate) fn same_family(recorded: CompatibilityFamily, admitted: CompatibilityFamily) -> bool {
    recorded.id == admitted.id && recorded.acp_protocol == admitted.acp_protocol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_characterised_claude_builds_share_one_family() {
        let old = claude_build_family("0.76.0", "1.4.0").expect("0.76 is characterised");
        let new = claude_build_family("0.101.0", "2.4.0").expect("0.101 is characterised");
        assert!(same_family(old, new));
        assert_eq!(claude_recorded_family("0.76.0"), Some(old));
        assert_eq!(claude_recorded_family("0.101.0"), Some(new));
    }

    #[test]
    fn a_build_outside_the_table_has_no_family() {
        for (adapter, sdk) in [
            ("0.101.1", "2.4.0"),
            ("0.102.0", "2.4.0"),
            ("0.101.0", "1.4.0"),
            ("0.76.0", "2.4.0"),
        ] {
            assert_eq!(claude_build_family(adapter, sdk), None, "{adapter}+{sdk}");
        }
        assert_eq!(claude_recorded_family("0.90.0"), None);
    }

    #[test]
    fn a_different_lineage_or_protocol_is_another_family() {
        let lineage = CompatibilityFamily::new("claude-acp-v1/claude-transcript-2", 1);
        let protocol = CompatibilityFamily::new(CLAUDE_ACP_V1.id, 2);
        assert!(!same_family(CLAUDE_ACP_V1, lineage));
        assert!(!same_family(CLAUDE_ACP_V1, protocol));
    }
}
