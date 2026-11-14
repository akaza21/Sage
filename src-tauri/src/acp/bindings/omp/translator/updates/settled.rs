use serde_json::{Map, Value};

use sage_acp_protocol::lifecycle::ToolState;

use super::super::{OmpTranslator, TranslationError};
use super::tool_snapshots::CommandPatch;
use crate::acp::bindings::omp::protocol::AdapterEvent;

impl OmpTranslator {
    pub(super) fn settled_tool_report(
        &mut self,
        tool_call_id: &str,
        current: ToolState,
        next: ToolState,
        update: &Map<String, Value>,
        patch: Option<&CommandPatch>,
        relocated: bool,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let reported = update
            .get("status")
            .is_some()
            .then_some(next)
            .filter(|reported| *reported != current);
        if let Some(reported) = reported {
            if !self.host_settled_tool(tool_call_id) {
                return Err(TranslationError::TerminalConflict);
            }
            if !reported.is_terminal() {
                return Ok(vec![AdapterEvent::IgnoredLate]);
            }
            self.record_settled_report(tool_call_id, reported)?;
        }
        let mutating = relocated
            || update.get("title").is_some()
            || patch.is_some_and(|patch| !patch.is_omitted());
        if mutating || reported.is_some() {
            return Ok(vec![AdapterEvent::IgnoredLate]);
        }
        Ok(vec![AdapterEvent::IgnoredDuplicate])
    }

    fn record_settled_report(
        &mut self,
        tool_call_id: &str,
        reported: ToolState,
    ) -> Result<(), TranslationError> {
        let active = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        match active.settled_reports.get(tool_call_id) {
            Some(previous) if *previous != reported => Err(TranslationError::TerminalConflict),
            Some(_) => Ok(()),
            None => {
                active
                    .settled_reports
                    .insert(tool_call_id.to_owned(), reported);
                Ok(())
            }
        }
    }

    fn host_settled_tool(&self, tool_call_id: &str) -> bool {
        self.resolved_permissions
            .values()
            .any(|resolved| resolved.tool_call_id.as_str() == tool_call_id)
    }
}
