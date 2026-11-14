use sage_acp_protocol::{
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
        NormalizedPayload::ToolCall { locations, .. } => locations.as_slice().iter().collect(),
        _ => vec![],
    };
    for location in locations {
        match location {
            SafeResourceLocation::Resource { resource }
                if !project_roots.iter().any(|root| root == &resource.root_id) =>
            {
                return Err(StoreError::Corrupt("resource root membership"));
            }
            SafeResourceLocation::Unavailable {
                reason,
                label,
                display_resource,
            } => {
                if label.as_str() != unavailable_label(*reason) {
                    return Err(StoreError::Corrupt("unavailable resource label"));
                }
                if let Some(display) = display_resource
                    && (*reason != UnavailableResourceReason::Missing
                        || !project_roots.iter().any(|root| root == &display.root_id)
                        || display.relative_path.as_str().len() > 512
                        || display.relative_path.as_str().chars().any(char::is_control))
                {
                    return Err(StoreError::Corrupt("unavailable display resource"));
                }
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
