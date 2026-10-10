use super::{StandardAcpTranslator, TranslationError};
use crate::acp::{
    contract::SafeText,
    intent::{ToolCategory, ToolExecution},
};

const OUTPUT_TAIL_BYTES: usize = 16_384;

impl StandardAcpTranslator {
    pub(super) fn tool_execution(
        &self,
        category: ToolCategory,
        output: Option<&str>,
    ) -> Result<ToolExecution, TranslationError> {
        let mut execution = ToolExecution {
            task_lifecycle: (category == ToolCategory::Subagent
                && self.profile.task_tool_lifecycle)
                .then_some(true),
            ..Default::default()
        };
        if let Some(output) = output.filter(|value| !value.is_empty()) {
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
