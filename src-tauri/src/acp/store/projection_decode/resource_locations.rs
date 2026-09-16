use crate::acp::{
    contract::SageRootId,
    intent::{NormalizedPayload, SafeResourceLocation, UnavailableResourceReason},
};

use super::StoreError;

pub(super) fn validate_membership(
    payload: &NormalizedPayload,
    project_roots: &[SageRootId],
) -> Result<(), StoreError> {
    let locations: Vec<&SafeResourceLocation> = match payload {
        NormalizedPayload::FileChangeDiff { resource, .. } => vec![resource],
        NormalizedPayload::ToolCall { locations, .. }
        | NormalizedPayload::ToolUpdateResult { locations, .. } => {
            locations.as_slice().iter().collect()
        }
        _ => vec![],
    };
    for location in locations {
        match location {
            SafeResourceLocation::Resource { resource }
                if !project_roots.iter().any(|root| root == &resource.root_id) =>
            {
                return Err(StoreError::Corrupt("resource root membership"));
            }
            SafeResourceLocation::Unavailable { reason, label }
                if label.as_str() != unavailable_label(*reason) =>
            {
                return Err(StoreError::Corrupt("unavailable resource label"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn unavailable_label(reason: UnavailableResourceReason) -> &'static str {
    match reason {
        UnavailableResourceReason::AmbiguousRoot => "Path belongs to multiple project roots",
        UnavailableResourceReason::Foreign => "Path is outside the active project roots",
        UnavailableResourceReason::Missing => "Path does not exist",
        UnavailableResourceReason::Noncanonical => "Path is not canonical",
        UnavailableResourceReason::Unsupported => "Path cannot be represented safely",
    }
}
