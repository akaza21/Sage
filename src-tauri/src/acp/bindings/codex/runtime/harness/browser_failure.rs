use super::{CodexRuntime, IncomingEvent, IngressDisposition};

impl CodexRuntime {
    pub(super) fn record_browser_failure_diagnostic(
        &mut self,
        disposition: &IngressDisposition,
        frame: &[u8],
    ) {
        if self.browser_failure_warning_logged {
            return;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(frame) else {
            return;
        };
        let failure = (value["method"] == "mcpServer/startupStatus/updated"
            && value["params"]["name"] == "sage-browser"
            && value["params"]["status"] == "failed")
            .then(|| {
                let thread = value["params"]["threadId"].as_str()?;
                let error = value["params"]["error"].as_str()?;
                Some((thread, self.mcp.browser_failure_detail(error), error.len()))
            })
            .flatten();
        if let Some((thread, detail, bytes)) = failure.as_ref()
            && !self.inner.has_active_turn()
            && !self
                .pending_browser_failure_details
                .iter()
                .any(|(id, _, _)| id == thread)
        {
            if self.pending_browser_failure_details.len() == 8 {
                self.pending_browser_failure_details.remove(0);
            }
            self.pending_browser_failure_details.push((
                (*thread).to_owned(),
                detail.clone(),
                *bytes,
            ));
        }
        let projected = matches!(disposition, IngressDisposition::Events(events) if events.iter().any(|event| {
            matches!(event, IncomingEvent::Error(crate::acp::intent::NormalizedPayload::Error { code, .. }) if code.as_str() == "browserMcpUnavailable")
        }));
        if !projected {
            return;
        }
        let thread = failure.as_ref().map(|(thread, _, _)| *thread).or_else(|| {
            value["result"]["thread"]["id"]
                .as_str()
                .or_else(|| value["params"]["thread"]["id"].as_str())
                .or_else(|| value["params"]["threadId"].as_str())
        });
        let pending = thread.and_then(|thread| {
            self.pending_browser_failure_details
                .iter()
                .position(|(id, _, _)| id == thread)
                .map(|index| self.pending_browser_failure_details.swap_remove(index))
        });
        let (detail, error_bytes) = failure
            .map(|(_, detail, bytes)| (detail, bytes))
            .or_else(|| pending.map(|(_, detail, bytes)| (detail, bytes)))
            .unwrap_or_else(|| ("browser failure detail unavailable".to_owned(), 0));
        tracing::warn!(
            detail = %detail,
            error_bytes,
            "Codex browser MCP startup failed"
        );
        self.browser_failure_warning_logged = true;
        self.pending_browser_failure_details.clear();
    }
}
