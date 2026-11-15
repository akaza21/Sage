use serde_json::{Map, Value};

use super::tool_input::command_exit_code;
use super::{StandardAcpTranslator, TranslationError};
use sage_acp_protocol::{
    contract::SafeText,
    intent::{ToolCategory, ToolExecution},
};
use sage_core::limits::LIMITS;

const OUTPUT_TAIL_BYTES: usize = 16_384;

/// ACP updates are partial: a later frame that omits the exit code or terminal output keeps the
/// earlier report instead of erasing it.
#[derive(Debug, Clone, Default)]
pub(super) struct CommandOutcome {
    exit_code: Option<i32>,
    tail: Option<SafeText<16384>>,
    truncated: bool,
}

impl StandardAcpTranslator {
    pub(super) fn reports_new_exit_code(
        &self,
        id: &str,
        category: ToolCategory,
        update: &Map<String, Value>,
    ) -> bool {
        category == ToolCategory::Execute
            && command_exit_code(update).is_some_and(|code| {
                self.command_outcomes
                    .get(id)
                    .and_then(|outcome| outcome.exit_code)
                    != Some(code)
            })
    }

    /// Only terminal-block output contributes to the tail; text content is already the item's
    /// output summary and is never repeated there. Retained tails share one per-turn byte budget;
    /// past it the earlier tail stays.
    pub(super) fn record_command_outcome(
        &mut self,
        id: &str,
        category: ToolCategory,
        update: &Map<String, Value>,
        terminal_output: Option<&str>,
    ) -> Result<(), TranslationError> {
        let exit_code = (category == ToolCategory::Execute)
            .then(|| command_exit_code(update))
            .flatten();
        let tail = terminal_output
            .filter(|output| !output.is_empty())
            .map(output_tail)
            .transpose()?;
        if exit_code.is_none() && tail.is_none() {
            return Ok(());
        }
        let mut outcome = self.command_outcomes.get(id).cloned().unwrap_or_default();
        if exit_code.is_some() {
            outcome.exit_code = exit_code;
        }
        if let Some((tail, truncated)) = tail {
            let previous = outcome.tail.as_ref().map_or(0, |tail| tail.as_str().len());
            let total = self
                .command_tail_bytes
                .saturating_sub(previous)
                .saturating_add(tail.as_str().len());
            if total <= LIMITS.wire.item_bytes_per_turn as usize {
                self.command_tail_bytes = total;
                outcome.tail = Some(tail);
                outcome.truncated = truncated;
            }
        }
        self.command_outcomes.insert(id.to_owned(), outcome);
        Ok(())
    }

    pub(super) fn tool_execution(
        &self,
        id: &str,
        category: ToolCategory,
    ) -> Result<ToolExecution, TranslationError> {
        let outcome = self.command_outcomes.get(id);
        Ok(ToolExecution {
            task_lifecycle: (category == ToolCategory::Subagent
                && self.profile.task_tool_lifecycle)
                .then_some(true),
            exit_code: outcome.and_then(|outcome| outcome.exit_code),
            output_tail: outcome.and_then(|outcome| outcome.tail.clone()),
            output_truncated: outcome.is_some_and(|outcome| outcome.truncated),
            ..Default::default()
        })
    }
}

fn output_tail(output: &str) -> Result<(SafeText<16384>, bool), TranslationError> {
    let mut start = output.len().saturating_sub(OUTPUT_TAIL_BYTES);
    while !output.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    Ok((
        SafeText::new("toolOutputTail", &output[start..])?,
        start != 0,
    ))
}
