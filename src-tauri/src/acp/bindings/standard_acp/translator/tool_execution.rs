use super::{StandardAcpTranslator, TranslationError};
use crate::acp::{
    contract::SafeText,
    intent::{ToolCategory, ToolExecution},
};

const OUTPUT_TAIL_BYTES: usize = 16_384;

impl StandardAcpTranslator {
    /// `terminal_output` is output only a terminal content block carried; output
    /// the tool's text content carried is already the item's output summary and
    /// is not repeated in the tail.
    pub(super) fn tool_execution(
        &self,
        id: &str,
        category: ToolCategory,
        terminal_output: Option<&str>,
    ) -> Result<ToolExecution, TranslationError> {
        let mut execution = ToolExecution {
            task_lifecycle: (category == ToolCategory::Subagent
                && self.profile.task_tool_lifecycle)
                .then_some(true),
            exit_code: self.tool_exit_codes.get(id).copied(),
            ..Default::default()
        };
        if let Some(output) = terminal_output.filter(|value| !value.is_empty()) {
            let mut start = output.len().saturating_sub(OUTPUT_TAIL_BYTES);
            while !output.is_char_boundary(start) {
                start = start.saturating_add(1);
            }
            execution.output_tail = Some(SafeText::new("toolOutputTail", &output[start..])?);
            execution.output_truncated = start != 0;
        }
        Ok(execution)
    }
}
