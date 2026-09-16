use crate::acp::{
    contract::{BoundedVec, SafeText},
    intent::{COMMAND_RESOURCE_LIMIT, SafeResourceLocation, ToolCategory},
    projection::ToolPresentation,
};

use super::super::StoreError;

fn contract(_: crate::acp::contract::ContractError) -> StoreError {
    StoreError::Corrupt("projection contract")
}

pub(super) fn presentation(
    category: ToolCategory,
    label: SafeText<512>,
    locations: BoundedVec<SafeResourceLocation, { COMMAND_RESOURCE_LIMIT }>,
) -> Result<ToolPresentation, StoreError> {
    Ok(match category {
        ToolCategory::File => ToolPresentation::File {
            label,
            resources: locations,
        },
        ToolCategory::Diff => ToolPresentation::Diff {
            label,
            resources: locations,
        },
        ToolCategory::Command => ToolPresentation::Command {
            label,
            summary: None,
            resources: locations,
        },
        ToolCategory::Search => ToolPresentation::Search {
            query_summary: SafeText::new("query", label.as_str()).map_err(contract)?,
            label,
            resources: BoundedVec::new("resources", vec![]).map_err(contract)?,
        },
        ToolCategory::Web => ToolPresentation::Web {
            label,
            summary: None,
        },
        ToolCategory::Mcp => ToolPresentation::Mcp {
            server_label: SafeText::new("server", "MCP").map_err(contract)?,
            tool_label: label.clone(),
            label,
        },
        ToolCategory::Subagent => ToolPresentation::Subagent {
            label,
            activity_id: None,
        },
        ToolCategory::Other => ToolPresentation::Generic {
            label,
            summary: None,
        },
    })
}
