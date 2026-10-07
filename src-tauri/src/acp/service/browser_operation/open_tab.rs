use super::*;

impl ServiceBrowserOperationObserver {
    pub(super) fn finished_open_tab(
        &mut self,
        request: &BrowserRequest,
        outcome: &Result<BrowserResponse, BrowserFailure>,
    ) -> Result<(), BrowserMcpError> {
        let app_state = self.app.state::<AppState>();
        let owner = app_state
            .acp
            .current_browser_owner(
                &self.owner.project_id,
                &self.owner.chat_id,
                &self.owner.session_id,
            )
            .filter(|owner| owner.runtime_epoch == self.owner.runtime_epoch)
            .ok_or(BrowserMcpError::Unavailable)?;
        let now = now_millis();
        let tab_id = match outcome {
            Ok(BrowserResponse {
                result: BrowserResult::OpenTab { tab_id },
                ..
            }) => *tab_id,
            _ => 0,
        };
        let mut record = BrowserOperationProjection {
            project_id: owner.project_id,
            chat_id: owner.chat_id,
            session_id: owner.session_id,
            turn_id: owner.turn_id,
            tab_id: format!("browser-{tab_id}"),
            operation_id: uuid::Uuid::new_v4().to_string(),
            operation: "openTab".into(),
            phase: "started".into(),
            origin: self.origin(request),
            summary: "Open browser tab".into(),
            result_summary: None,
            created_at: now,
            updated_at: now,
        };
        let mut connection = app_state
            .acp
            .database
            .write_connection()
            .map_err(|_| BrowserMcpError::Unavailable)?;
        browser_operations::start(&mut connection, self.owner.runtime_epoch, &record)
            .map_err(|_| BrowserMcpError::Unavailable)?;
        let (phase, result) = outcome_summary(outcome);
        record.phase = phase.into();
        record.result_summary = Some(result);
        browser_operations::finish(&mut connection, &record)
            .map_err(|_| BrowserMcpError::OutcomeUnknown)?;
        drop(connection);
        self.emit(&record);
        Ok(())
    }
}
