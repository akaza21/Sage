use serde::{Deserialize, Serialize};

use super::{SafeResourceLocation, SafeText};

/// Execution facts a tool call carries beside its category and label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolExecution {
    pub cwd: Option<SafeResourceLocation>,
    pub exit_code: Option<i32>,
    pub output_tail: Option<SafeText<16384>>,
    pub output_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_lifecycle: Option<bool>,
    /// MCP server identity as the harness names it; never a display name
    /// Sage invents. Boxed so tool payloads stay compact in adapter events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<Box<SafeText<128>>>,
    /// Latest status line a running tool reported, replaced on each report
    /// and absent once the tool is terminal. Capped at
    /// [`TOOL_PROGRESS_BYTES`]; boxed like `mcp_server`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Box<SafeText<TOOL_PROGRESS_BYTES>>>,
}

/// Byte cap of a running tool's status line.
pub const TOOL_PROGRESS_BYTES: usize = 512;

#[cfg(test)]
mod tests {
    use super::ToolExecution;

    #[test]
    fn legacy_execution_rows_decode_without_optional_facts_and_omit_them_on_write() {
        let legacy: ToolExecution =
            serde_json::from_str(r#"{"exitCode":0,"outputTruncated":false}"#).expect("legacy");
        assert_eq!(legacy.mcp_server, None);
        let written = serde_json::to_value(&legacy).expect("encode");
        assert!(written.get("mcpServer").is_none());
        assert_eq!(legacy.progress, None);
        assert!(written.get("progress").is_none());
    }
}
