#![cfg_attr(
    not(windows),
    allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
)]

use std::collections::HashMap;

use tauri::{AppHandle, Emitter, Manager};

use crate::{
    acp::{
        browser::{
            BrowserAction, BrowserCommand, BrowserFailure, BrowserRequest, BrowserResponse,
            BrowserResult,
            mcp::{BrowserMcpError, BrowserOperationObserver},
            redact_console,
        },
        limits::BROWSER_LIMITS,
        projection::BrowserOperationProjection,
        scheduler::RuntimeOwner,
        store::browser_operations,
    },
    persistence::now_millis,
    state::AppState,
};

mod open_tab;

pub(super) struct ServiceBrowserOperationObserver {
    pub(super) app: AppHandle,
    pub(super) owner: RuntimeOwner,
    current: Option<BrowserOperationProjection>,
    labels: ObservedClickLabels,
}

impl ServiceBrowserOperationObserver {
    pub(super) fn new(app: AppHandle, owner: RuntimeOwner) -> Self {
        Self {
            app,
            owner,
            current: None,
            labels: ObservedClickLabels::default(),
        }
    }

    fn origin(&self, request: &BrowserRequest) -> String {
        if let BrowserCommand::OpenTab { url } | BrowserCommand::Navigate { url } = &request.command
        {
            return url::Url::parse(url)
                .map(|url| url.origin().ascii_serialization())
                .unwrap_or_else(|_| "unavailable".into());
        }
        let app = self.app.state::<AppState>();
        let label = format!("browser-{}", request.scope.tab_id);
        let Some(lease) = app
            .acp
            .browser_attachment(&self.owner.project_id, &self.owner.chat_id)
            .filter(|lease| lease.tab_label == label)
        else {
            return "unavailable".into();
        };
        url::Url::parse(&lease.url)
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_else(|_| "unavailable".into())
    }

    fn emit(&self, record: &BrowserOperationProjection) {
        let _ = self.app.emit_to("main", "browser:agent-operation", record);
    }
}

impl BrowserOperationObserver for ServiceBrowserOperationObserver {
    fn started(&mut self, request: &BrowserRequest) -> Result<(), BrowserMcpError> {
        if matches!(request.command, BrowserCommand::OpenTab { .. }) {
            return Ok(());
        }
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
        let record = BrowserOperationProjection {
            project_id: owner.project_id,
            chat_id: owner.chat_id,
            session_id: owner.session_id,
            turn_id: owner.turn_id,
            tab_id: format!("browser-{}", request.scope.tab_id),
            operation_id: uuid::Uuid::new_v4().to_string(),
            operation: operation(&request.command).into(),
            phase: "started".into(),
            origin: self.origin(request),
            summary: self.labels.summary(&request.command),
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
        drop(connection);
        self.emit(&record);
        self.current = Some(record);
        Ok(())
    }

    fn finished(
        &mut self,
        request: &BrowserRequest,
        outcome: &Result<BrowserResponse, BrowserFailure>,
    ) -> Result<(), BrowserMcpError> {
        if matches!(request.command, BrowserCommand::OpenTab { .. }) {
            return self.finished_open_tab(request, outcome);
        }
        let mut record = self.current.take().ok_or(BrowserMcpError::OutcomeUnknown)?;
        record.updated_at = now_millis().max(record.created_at);
        let (phase, result) = outcome_summary(outcome);
        if let Ok(response) = outcome {
            self.labels.remember(&response.result);
        }
        record.phase = phase.into();
        record.result_summary = Some(result);
        let app_state = self.app.state::<AppState>();
        let mut connection = app_state
            .acp
            .database
            .write_connection()
            .map_err(|_| BrowserMcpError::OutcomeUnknown)?;
        browser_operations::finish(&mut connection, &record)
            .map_err(|_| BrowserMcpError::OutcomeUnknown)?;
        drop(connection);
        self.emit(&record);
        Ok(())
    }
}

#[derive(Default)]
struct ObservedClickLabels {
    by_handle: HashMap<String, String>,
}

impl ObservedClickLabels {
    fn remember(&mut self, result: &BrowserResult) {
        match result {
            BrowserResult::SnapshotPage { nodes, .. } => {
                self.by_handle.clear();
                for node in nodes.iter().take(BROWSER_LIMITS.snapshot_nodes) {
                    if !matches!(node.role.as_str(), "button" | "link") {
                        continue;
                    }
                    let label = redact_console(&node.name);
                    if label.is_empty()
                        || label.len() > 80
                        || label.chars().any(|ch| {
                            ch.is_control()
                                || ch == '"'
                                || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                        })
                    {
                        continue;
                    }
                    self.by_handle.insert(node.handle.token().to_owned(), label);
                }
            }
            BrowserResult::Navigate { .. } => self.by_handle.clear(),
            _ => {}
        }
    }

    fn summary(&self, command: &BrowserCommand) -> String {
        if let BrowserCommand::Act {
            action: BrowserAction::Click { handle },
        } = command
            && let Some(label) = self.by_handle.get(handle.token())
        {
            return format!("Click \"{label}\"");
        }
        summary(command)
    }
}

fn operation(command: &BrowserCommand) -> &'static str {
    match command {
        BrowserCommand::OpenTab { .. } => "openTab",
        BrowserCommand::Navigate { .. } => "navigate",
        BrowserCommand::SnapshotPage {} => "snapshotPage",
        BrowserCommand::Act { .. } => "act",
        BrowserCommand::WaitFor { .. } => "waitFor",
        BrowserCommand::Screenshot { .. } => "screenshot",
        BrowserCommand::ReadConsole { .. } => "readConsole",
        BrowserCommand::ReadNetwork { .. } => "readNetwork",
    }
}

fn summary(command: &BrowserCommand) -> String {
    match command {
        BrowserCommand::OpenTab { .. } => "Open browser tab".into(),
        BrowserCommand::Navigate { .. } => "Navigate to origin".into(),
        BrowserCommand::SnapshotPage {} => "Inspect page".into(),
        BrowserCommand::Act { action } => match action {
            BrowserAction::Click { .. } => "Click element",
            BrowserAction::Fill { .. } => "Fill field",
            BrowserAction::Key { .. } => "Press key",
            BrowserAction::Scroll { .. } => "Scroll page",
        }
        .into(),
        BrowserCommand::WaitFor { .. } => "Wait for page condition".into(),
        BrowserCommand::Screenshot { .. } => "Capture screenshot".into(),
        BrowserCommand::ReadConsole { .. } => "Read console metadata".into(),
        BrowserCommand::ReadNetwork { .. } => "Read network metadata".into(),
    }
}

fn outcome_summary(outcome: &Result<BrowserResponse, BrowserFailure>) -> (&'static str, String) {
    match outcome {
        Ok(BrowserResponse {
            result: BrowserResult::Act { applied: false },
            ..
        }) => ("failed", "Action not applied".into()),
        Ok(response) => (
            "completed",
            match &response.result {
                BrowserResult::OpenTab { .. } => "Browser tab opened".into(),
                BrowserResult::Navigate { .. } => "Navigation requested".into(),
                BrowserResult::SnapshotPage { nodes, .. } => {
                    format!("{} elements observed", nodes.len())
                }
                BrowserResult::Act { applied } => if *applied {
                    "Action applied"
                } else {
                    "Action not applied"
                }
                .into(),
                BrowserResult::WaitFor { matched } => if *matched {
                    "Condition matched"
                } else {
                    "Condition not matched"
                }
                .into(),
                BrowserResult::Screenshot { width, height, .. } => {
                    format!("Screenshot captured ({width}×{height})")
                }
                BrowserResult::ReadConsole { entries, .. } => {
                    format!("{} console entries", entries.len())
                }
                BrowserResult::ReadNetwork { entries, .. } => {
                    format!("{} network entries", entries.len())
                }
            },
        ),
        Err(BrowserFailure::Denied | BrowserFailure::NoAttachment) => {
            ("denied", "Browser access denied".into())
        }
        Err(BrowserFailure::OutcomeUnknown) => (
            "outcomeUnknown",
            "Issued command outcome unknown; inspect page before retrying".into(),
        ),
        Err(_) => ("failed", "Browser operation failed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::browser::{ElementHandle, FillSecret, SnapshotNode};

    #[test]
    fn fill_value_never_enters_operation_summary() {
        let command = BrowserCommand::Act {
            action: BrowserAction::Fill {
                handle: ElementHandle::from_token("opaque".into()),
                value: FillSecret::new("private-password".into()),
            },
        };
        assert_eq!(summary(&command), "Fill field");
        assert!(!summary(&command).contains("private-password"));
        assert!(!format!("{command:?}").contains("private-password"));
        assert!(
            !serde_json::to_string(&command)
                .unwrap()
                .contains("private-password")
        );
    }

    #[test]
    fn navigation_projection_omits_untrusted_path_query_and_fragment() {
        let command = BrowserCommand::Navigate {
            url: "https://example.com/reset/short-code?password=private-password#secret".into(),
        };
        let serialized = serde_json::json!({
            "operation": operation(&command),
            "origin": "https://example.com",
            "summary": summary(&command),
        })
        .to_string();
        assert!(serialized.contains("Navigate to origin"));
        assert!(!serialized.contains("short-code"));
        assert!(!serialized.contains("private-password"));
    }

    #[test]
    fn redacted_snapshot_label_names_a_click_but_not_a_fill() {
        let handle = ElementHandle::from_token("opaque".into());
        let mut labels = ObservedClickLabels::default();
        labels.remember(&BrowserResult::SnapshotPage {
            navigation_epoch: 1,
            text: String::new(),
            nodes: vec![SnapshotNode {
                handle: handle.clone(),
                role: "button".into(),
                name: "Submit".into(),
            }],
        });
        let click = BrowserCommand::Act {
            action: BrowserAction::Click {
                handle: handle.clone(),
            },
        };
        assert_eq!(labels.summary(&click), "Click \"Submit\"");
        let fill = BrowserCommand::Act {
            action: BrowserAction::Fill {
                handle,
                value: FillSecret::new("private-password".into()),
            },
        };
        assert_eq!(labels.summary(&fill), "Fill field");
        assert!(!format!("{fill:?}").contains("private-password"));
    }

    #[test]
    fn issued_unknown_outcome_has_warning_phase() {
        let (phase, summary) = outcome_summary(&Err(BrowserFailure::OutcomeUnknown));
        assert_eq!(phase, "outcomeUnknown");
        assert!(summary.contains("inspect page"));
    }

    #[test]
    fn unapplied_action_is_a_visible_failure() {
        let response = BrowserResponse {
            scope: crate::acp::browser::BrowserScope {
                session_id: "session-one".into(),
                tab_id: 7,
            },
            result: BrowserResult::Act { applied: false },
        };
        assert_eq!(
            outcome_summary(&Ok(response)),
            ("failed", "Action not applied".into())
        );
    }
}
