//! The built-in commands a new-chat draft offers before its session exists,
//! projected from the harness's curated known tier. Reads installation
//! metadata and the discovered release only; never starts a harness process.

use super::HarnessCatalog;
use crate::{
    acp::bindings::release_version::ReleaseVersion, harness_discovery::known_harness_spec,
};

impl HarnessCatalog {
    /// Command names, at most `MAX_DRAFT_BUILTINS`; none for a harness
    /// without curated command metadata.
    pub fn draft_builtin_commands(&self, harness_id: &str) -> Vec<String> {
        let Some(table) = known_harness_spec(harness_id).and_then(|spec| spec.known_commands)
        else {
            return Vec::new();
        };
        let installed = self
            .discovered(harness_id)
            .and_then(|harness| harness.version.as_deref().and_then(ReleaseVersion::parse));
        table()
            .draft_builtins(installed.as_ref())
            .map(Into::into)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_discovery::InstalledHarness;

    #[test]
    fn a_draft_projects_each_curated_harness_table_and_nothing_otherwise() {
        let catalog = HarnessCatalog::default();
        // Before, between and after the releases Claude's `since` rows name:
        // those rows are native controls, so no discovered release admits them.
        for version in [None, Some("2.1.151"), Some("2.1.200"), Some("2.1.300")] {
            catalog.discovered.replace(vec![InstalledHarness {
                id: "claude-code",
                executable: "claude".into(),
                version: version.map(Into::into),
            }]);
            assert_eq!(
                catalog.draft_builtin_commands("claude-code"),
                ["fast", "agents"],
                "{version:?}"
            );
        }
        assert_eq!(catalog.draft_builtin_commands("codex"), ["skills"]);
        for harness in ["pi", "gemini-cli", "sage-simulator", "missing"] {
            assert!(
                catalog.draft_builtin_commands(harness).is_empty(),
                "{harness}"
            );
        }
    }
}
