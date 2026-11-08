use serde::{Deserialize, Serialize};

use super::{SafeResourceLocation, SafeText};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileChangeOperation {
    #[default]
    Updated,
    Created,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileChangeFacts {
    pub(crate) resource: SafeResourceLocation,
    pub(crate) summary: SafeText<65536>,
    pub(crate) additions: Option<u64>,
    pub(crate) deletions: Option<u64>,
    pub(crate) operation: FileChangeOperation,
}

impl FileChangeOperation {
    /// The reader-facing line for one changed file, such as `Edited src/lib.rs`.
    /// A path that cannot be shown safely reads as `file`; nothing here names
    /// the harness or invents detail it did not report.
    pub(crate) fn describe(self, location: &SafeResourceLocation) -> String {
        let verb = match self {
            Self::Created => "Created",
            Self::Updated => "Edited",
            Self::Deleted => "Deleted",
        };
        let resource = match location {
            SafeResourceLocation::Resource { resource } => Some(resource),
            SafeResourceLocation::Unavailable {
                display_resource, ..
            } => display_resource.as_ref(),
        };
        let display = resource
            .map(|resource| resource.relative_path.as_str())
            .filter(|path| path.len() <= 480 && !path.chars().any(char::is_control))
            .unwrap_or("file");
        format!("{verb} {display}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        contract::SageRootId,
        intent::{SafeResourceRef, UnavailableResourceReason},
    };

    fn resource(path: &str) -> SafeResourceRef {
        SafeResourceRef::new(SageRootId::parse("root-1").expect("root"), path).expect("path")
    }

    #[test]
    fn each_operation_names_the_mapped_path_or_a_plain_file() {
        let mapped = SafeResourceLocation::Resource {
            resource: resource("src/lib.rs"),
        };
        assert_eq!(
            FileChangeOperation::Updated.describe(&mapped),
            "Edited src/lib.rs"
        );
        assert_eq!(
            FileChangeOperation::Created.describe(&mapped),
            "Created src/lib.rs"
        );
        let gone = SafeResourceLocation::unavailable_with_display(
            UnavailableResourceReason::Missing,
            resource("src/old.rs"),
        );
        assert_eq!(
            FileChangeOperation::Deleted.describe(&gone),
            "Deleted src/old.rs"
        );
        let foreign = SafeResourceLocation::unavailable(UnavailableResourceReason::Foreign);
        assert_eq!(
            FileChangeOperation::Updated.describe(&foreign),
            "Edited file"
        );
    }
}
