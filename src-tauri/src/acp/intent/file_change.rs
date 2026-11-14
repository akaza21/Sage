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

/// Added and deleted line counts between two whole-file texts, outside their
/// shared leading and trailing lines. Exact for one contiguous change, the
/// shape every single-hunk edit tool produces, and linear in the text size.
pub(crate) fn line_delta(old: &str, new: &str) -> (u64, u64) {
    let old: Vec<&str> = old.split_inclusive('\n').collect();
    let new: Vec<&str> = new.split_inclusive('\n').collect();
    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (
        (new.len() - head - tail) as u64,
        (old.len() - head - tail) as u64,
    )
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

    #[test]
    fn line_delta_counts_only_the_changed_span() {
        assert_eq!(line_delta("a\nb\nc\n", "a\nB\nc\n"), (1, 1));
        assert_eq!(line_delta("", "a\nb\n"), (2, 0));
        assert_eq!(line_delta("a\nb\n", ""), (0, 2));
        assert_eq!(line_delta("a\n", "a\nb\n"), (1, 0));
        assert_eq!(line_delta("a\nb", "a\nb\n"), (1, 1));
        assert_eq!(line_delta("x\nx\n", "x\nx\nx\n"), (1, 0));
        assert_eq!(line_delta("same\n", "same\n"), (0, 0));
    }
}
