//! The installed, running and latest releases of a harness, and what updating
//! it unlocks, for Settings › Harnesses.

use sage_domain::harness::{HarnessDefinition, MAX_UPDATE_UNLOCKS};

use super::{catalog_definition::safe_display_version, published_releases::PublishedReleases};
use crate::{
    acp::{
        bindings::{registry::BindingRegistry, release_version::ReleaseVersion},
        command_placement::KnownCommandTable,
    },
    harness_discovery::KnownHarnessSpec,
};

/// Expects `definition.version` to hold the installed release already.
pub(super) fn annotate(
    definition: &mut HarnessDefinition,
    spec: Option<&KnownHarnessSpec>,
    published: &PublishedReleases,
    registry: &BindingRegistry,
) {
    // npm output is untrusted: project only a bounded version token.
    definition.latest_version = published
        .latest_for(&definition.id)
        .as_deref()
        .and_then(safe_display_version);
    definition.update = published
        .update_for(&definition.id, definition.version.as_deref())
        .filter(|update| definition.latest_version.as_ref() == Some(&update.latest_version));
    if let (Some(update), Some(known_commands)) = (
        definition.update.as_mut(),
        spec.and_then(|spec| spec.known_commands),
    ) {
        update.unlocks = unlocks(
            known_commands(),
            definition.version.as_deref(),
            &update.latest_version,
        );
    }
    let Some(record) = registry
        .for_harness(&definition.id)
        .filter(|record| record.admission_available())
    else {
        return;
    };
    definition.runs_version = record
        .runs_version()
        .as_deref()
        .and_then(safe_display_version);
    definition.direct_from_version = registry
        .direct_binding_minimum(&definition.id)
        .map(Into::into);
}

fn unlocks(table: &KnownCommandTable, installed: Option<&str>, latest: &str) -> Vec<String> {
    let (Some(installed), Some(latest)) = (
        installed.and_then(ReleaseVersion::parse),
        ReleaseVersion::parse(latest),
    ) else {
        return Vec::new();
    };
    table
        .introduced_between(&installed, &latest)
        .take(MAX_UPDATE_UNLOCKS)
        .map(Into::into)
        .collect()
}
