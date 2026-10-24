use crate::acp::{
    contract::{BoundedVec, SafeText},
    declared_test::CommandActivity,
    intent::{COMMAND_RESOURCE_LIMIT, SafeResourceLocation, ToolCategory, ToolExecution},
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
    command_activity: CommandActivity,
    execution: ToolExecution,
) -> Result<ToolPresentation, StoreError> {
    Ok(match category {
        ToolCategory::Read => ToolPresentation::Read {
            label,
            resources: locations,
        },
        ToolCategory::Edit => ToolPresentation::Edit {
            label,
            resources: locations,
        },
        ToolCategory::Delete => ToolPresentation::Delete {
            label,
            resources: locations,
        },
        ToolCategory::Move => ToolPresentation::Move {
            label,
            resources: locations,
            original_resources: BoundedVec::new("originalResources", vec![]).map_err(contract)?,
        },
        ToolCategory::Execute => ToolPresentation::Execute {
            label,
            summary: None,
            cwd: execution.cwd,
            exit_code: execution.exit_code,
            output_tail: execution.output_tail,
            output_truncated: execution.output_truncated,
            resources: locations,
            activity: command_activity,
        },
        ToolCategory::Search => ToolPresentation::Search {
            query_summary: SafeText::new("query", label.as_str()).map_err(contract)?,
            label,
            resources: BoundedVec::new("resources", vec![]).map_err(contract)?,
            match_count: None,
            truncated: false,
        },
        ToolCategory::Fetch => ToolPresentation::Fetch {
            label,
            url: None,
            summary: None,
        },
        ToolCategory::WebSearch => ToolPresentation::WebSearch { label },
        ToolCategory::ImageView => ToolPresentation::ImageView {
            label,
            resources: locations,
        },
        ToolCategory::Think => ToolPresentation::Think {
            label,
            summary: None,
        },
        ToolCategory::SwitchMode => ToolPresentation::SwitchMode {
            label,
            mode_id: None,
            mode_label: None,
        },
        ToolCategory::Mcp => ToolPresentation::Mcp {
            server_label: SafeText::new(
                "server",
                execution
                    .mcp_server
                    .as_ref()
                    .map_or("MCP", |server| server.as_str()),
            )
            .map_err(contract)?,
            tool_label: label.clone(),
            label,
            arguments_summary: None,
            result_summary: None,
        },
        ToolCategory::Subagent => ToolPresentation::Subagent {
            label,
            activity_id: None,
            task_lifecycle: execution.task_lifecycle,
        },
        ToolCategory::Other => ToolPresentation::Other {
            label,
            raw_kind: None,
            summary: None,
        },
    })
}
