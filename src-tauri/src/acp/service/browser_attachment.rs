#![cfg_attr(
    not(windows),
    allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
)]

use crate::acp::{
    browser::attachment::AttachmentLease,
    contract::{SageChatId, SageProjectId},
    service::browser_permission::BrowserPermissionOwner,
};
#[cfg(windows)]
use tauri::Manager;

#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
enum SweepResult {
    Closed,
    LifecycleOwned,
    Exhausted,
}

#[cfg(any(windows, test))]
async fn sweep_agent_tab<F, Future, Tracked>(
    mut close: F,
    tracked: Tracked,
    retry_delay: std::time::Duration,
    max_attempts: usize,
) -> SweepResult
where
    F: FnMut() -> Future,
    Future: std::future::Future<Output = Result<(), String>>,
    Tracked: Fn() -> bool,
{
    for attempt in 1..=max_attempts {
        match close().await {
            Ok(()) => return SweepResult::Closed,
            Err(_) if !tracked() => return SweepResult::Closed,
            Err(error)
                if error == "browser tab unavailable"
                    || error.starts_with(
                        "Browser is still closing; restart Sage before browsing again",
                    ) =>
            {
                return SweepResult::LifecycleOwned;
            }
            Err(_) if attempt == max_attempts => return SweepResult::Exhausted,
            Err(_) => {}
        }
        tokio::time::sleep(retry_delay.saturating_mul(attempt as u32)).await;
    }
    SweepResult::Exhausted
}

impl super::AcpService {
    fn clear_browser_highlight(&self, label: &str) {
        #[cfg(windows)]
        {
            let app = self.browser_app.lock().ok().and_then(|slot| slot.clone());
            if let Some(webview) = app.and_then(|app| {
                app.state::<crate::browser::BrowserState>()
                    .webview_for_tab(label)
            }) {
                std::thread::spawn(move || {
                    crate::browser::devtools::clear_agent_highlight(&webview);
                });
            }
        }
        #[cfg(not(windows))]
        let _ = label;
    }

    #[cfg(windows)]
    pub(crate) fn set_browser_app(&self, app: tauri::AppHandle) {
        if let Ok(mut slot) = self.browser_app.lock() {
            *slot = Some(app);
        }
    }
    fn browser_chat_is_live(&self, project: &SageProjectId, chat: &SageChatId) -> bool {
        self.database
            .connection()
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM chat_tabs t
                   JOIN projects p ON p.id=t.project_id
                   JOIN app_state a ON a.singleton=1 AND a.active_project_id=t.project_id
                   WHERE t.project_id=?1 AND t.chat_id=?2
                 )",
                rusqlite::params![project.as_str(), chat.as_str()],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
    }

    pub(crate) fn attach_browser_tab(
        &self,
        project: SageProjectId,
        chat: SageChatId,
        label: &str,
        url: &str,
        revision: u64,
    ) -> Result<(), ()> {
        if !self.browser_chat_is_live(&project, &chat) {
            return Err(());
        }
        let previous = self
            .browser_attachments
            .resolve(&project, &chat)
            .map(|lease| lease.tab_label);
        self.browser_attachments
            .attach(project, chat, label, url, revision)?;
        if let Some(previous) = previous {
            self.clear_browser_highlight(&previous);
        }
        Ok(())
    }

    pub(crate) fn detach_browser_tab(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> Result<(), ()> {
        if let Some(label) = self.browser_attachments.detach(project, chat) {
            self.clear_browser_highlight(&label);
        }
        Ok(())
    }

    pub(crate) fn browser_attachment(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> Option<AttachmentLease> {
        if !self.browser_chat_is_live(project, chat) {
            let _ = self.detach_browser_tab(project, chat);
            return None;
        }
        self.browser_attachments.resolve(project, chat)
    }

    pub(crate) fn agent_browser_attachment(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
        tab_number: u64,
    ) -> Option<AttachmentLease> {
        if !self.browser_chat_is_live(project, chat) {
            return None;
        }
        self.browser_attachments
            .agent_resolve(project, chat, session, tab_number)
    }

    pub(crate) fn latest_agent_browser_attachment(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
    ) -> Option<AttachmentLease> {
        if !self.browser_chat_is_live(project, chat) {
            return None;
        }
        self.browser_attachments
            .agent_latest(project, chat, session)
    }

    pub(crate) fn agent_browser_count(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
    ) -> usize {
        self.browser_attachments.agent_count(project, chat, session)
    }

    pub(crate) fn attach_agent_browser_tab(
        &self,
        owner: &BrowserPermissionOwner,
        label: &str,
        url: &str,
        revision: u64,
    ) -> Result<(), ()> {
        if !self.browser_chat_is_live(&owner.project_id, &owner.chat_id)
            || !self
                .current_browser_owner(&owner.project_id, &owner.chat_id, &owner.session_id)
                .is_some_and(|current| {
                    current.turn_id == owner.turn_id
                        && current.runtime_epoch == owner.runtime_epoch
                        && current.binding_digest == owner.binding_digest
                })
        {
            return Err(());
        }
        self.browser_attachments.attach_agent(
            owner.project_id.clone(),
            owner.chat_id.clone(),
            owner.session_id.as_str(),
            label,
            url,
            revision,
        )
    }

    pub(crate) fn detach_agent_browser_tab(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
        tab_number: u64,
    ) {
        self.browser_attachments
            .agent_detach(project, chat, session, tab_number);
    }

    pub(crate) fn retire_agent_browser_tab(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
        tab_number: u64,
    ) {
        self.browser_attachments
            .agent_retire(project, chat, session, tab_number);
    }

    pub(crate) fn revoke_browser_chat(&self, project: &SageProjectId, chat: &SageChatId) {
        let _ = self.detach_browser_tab(project, chat);
        #[cfg(windows)]
        if let Some(app) = self.browser_app.lock().ok().and_then(|slot| slot.clone()) {
            for label in self
                .browser_attachments
                .agent_labels_for_chat(project, chat)
            {
                let Some(claim) = self.browser_attachments.agent_claim_sweep(&label) else {
                    continue;
                };
                self.browser_attachments.agent_retire_label(&label);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _claim = claim;
                    let result = sweep_agent_tab(
                        || {
                            let app = app.clone();
                            let label = label.clone();
                            async move {
                                let state = app.state::<crate::browser::BrowserState>();
                                crate::browser::lifecycle::browser_close(app.clone(), state, label)
                                    .await
                            }
                        },
                        || {
                            app.state::<crate::state::AppState>()
                                .acp
                                .browser_attachments
                                .agent_has_label(&label)
                        },
                        std::time::Duration::from_secs(2),
                        3,
                    )
                    .await;
                    if result == SweepResult::Exhausted {
                        app.state::<crate::browser::BrowserState>()
                            .quarantine_failed_close(&label);
                        eprintln!("agent browser cleanup requires app restart");
                    }
                    app.state::<crate::state::AppState>()
                        .acp
                        .revoke_browser_tab(&label);
                });
            }
            return;
        }
        self.browser_attachments.revoke_agent_chat(project, chat);
    }

    pub(crate) fn revoke_browser_other_projects(&self, active_project: &SageProjectId) {
        for (project, chat) in self.browser_attachments.agent_chats() {
            if &project != active_project {
                self.revoke_browser_chat(&project, &chat);
            }
        }
        for label in self
            .browser_attachments
            .revoke_other_projects(active_project)
        {
            self.clear_browser_highlight(&label);
        }
    }

    pub(crate) fn revoke_browser_tab(&self, label: &str) {
        self.browser_attachments.revoke_tab(label);
        self.clear_browser_highlight(label);
    }

    pub(crate) fn take_over_browser_tab(&self, label: &str) -> bool {
        let taken = self.browser_attachments.take_over(label);
        if taken {
            self.clear_browser_highlight(label);
        }
        taken
    }

    pub(crate) fn hand_back_browser_tab(&self, label: &str) -> bool {
        self.browser_attachments.hand_back(label)
    }

    pub(crate) fn browser_hand_back_generation(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> u64 {
        self.browser_attachments.hand_back_generation(project, chat)
    }

    /// The popups that joined `opener`, which follow it on Take over.
    pub(crate) fn browser_joined_popups(&self, opener: &str) -> Vec<String> {
        self.browser_attachments.joined_popups(opener)
    }

    /// Marks an Agent click, key or navigation running on `label`; a popup
    /// joins only around one.
    pub(crate) fn begin_browser_agent_action(
        &self,
        label: &str,
    ) -> crate::acp::browser::attachment::AgentAction<'_> {
        self.browser_attachments.begin_agent_action(label)
    }

    /// Joins a tab that the Agent-driven `opener` opened to the opener's
    /// Agent attachment, now or at its first settled page.
    pub(crate) fn adopt_browser_popup(
        &self,
        opener: &str,
        child: &str,
        settled: Option<(&str, u64)>,
    ) {
        self.browser_attachments
            .adopt_popup(opener, child, settled, &|project, chat, session| {
                self.browser_turn_running(project, chat, session)
            });
    }

    /// Whether the chat is live and `session` has a current owner whose
    /// turn runs: the same checks `attach_agent_browser_tab` makes, for a
    /// popup that joins without an owner of its own.
    fn browser_turn_running(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
    ) -> bool {
        self.browser_chat_is_live(project, chat)
            && crate::acp::contract::SageAgentSessionId::parse(session).is_ok_and(|session| {
                self.current_browser_owner(project, chat, &session)
                    .is_some_and(|owner| {
                        self.browser_owner_precheck_matches(&owner)
                            && self.browser_owner_witness_matches(&owner)
                    })
            })
    }

    /// Ends every developer-mode grant, as the Settings switch going off
    /// does, and returns the tabs that held one.
    pub(crate) fn end_browser_developer_mode(&self) -> Vec<String> {
        self.browser_attachments.end_developer_mode()
    }

    pub(crate) fn browser_agent_control(
        &self,
        label: &str,
    ) -> Option<crate::acp::browser::attachment::AgentControl> {
        self.browser_attachments.agent_control(label)
    }

    /// Whether the user took over any tab this chat's Agent may drive; the
    /// Agent then opens no other tab either.
    pub(crate) fn browser_chat_under_user_control(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> bool {
        self.browser_attachments
            .chat_under_user_control(project, chat)
    }

    pub(crate) fn browser_navigation_started(&self, label: &str) {
        self.browser_attachments.navigation_started(label);
    }

    pub(crate) fn browser_navigation_settled(&self, label: &str, url: &str, revision: u64) {
        self.browser_attachments
            .navigation_settled(label, url, revision);
        self.browser_attachments
            .join_popup(label, url, revision, &|project, chat, session| {
                self.browser_turn_running(project, chat, session)
            });
    }
}

#[cfg(test)]
mod tests {
    use super::super::browser_shutdown::{
        close_agent_tabs_once_for_shutdown, complete_shutdown_close,
    };
    use super::*;
    use std::cell::Cell;

    #[test]
    fn non_codex_close_routes_retired_tab_through_shared_cleanup() {
        use crate::acp::{
            contract::{ClientIntentId, ProjectionRevision},
            intent::BindingId,
            store::CloseSessionCommand,
        };
        let (service, owner) =
            super::super::permission::tests::support::service_with_pending_requests(
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            );
        service
            .browser_attachments
            .attach_agent(
                owner.project_id.clone(),
                owner.chat_id.clone(),
                owner.session_id.as_str(),
                "browser-8",
                "https://example.com/",
                1,
            )
            .unwrap();
        service.browser_attachments.agent_retire(
            &owner.project_id,
            &owner.chat_id,
            owner.session_id.as_str(),
            8,
        );
        assert!(
            service
                .agent_browser_attachment(
                    &owner.project_id,
                    &owner.chat_id,
                    owner.session_id.as_str(),
                    8,
                )
                .is_none()
        );
        let binding = BindingId::new("bindingId", "claude-agent-acp").unwrap();
        let _ = service.dispatch_session_close(
            &binding,
            CloseSessionCommand {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                expected_projection_revision: ProjectionRevision::ZERO,
                client_intent_id: ClientIntentId::new(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                now_millis: crate::persistence::now_millis(),
            },
        );
        assert!(!service.browser_attachments.agent_has_label("browser-8"));
        service.shutdown_blocking().unwrap();
    }

    #[tokio::test]
    async fn chat_end_sweep_retries_a_tracked_tab_until_close_succeeds() {
        let attempts = Cell::new(0);
        let tracked = Cell::new(true);
        let tab_live = Cell::new(true);
        let result = sweep_agent_tab(
            || async {
                attempts.set(attempts.get() + 1);
                if attempts.get() == 1 {
                    return Err("native close failed".to_owned());
                }
                tab_live.set(false);
                tracked.set(false);
                Ok(())
            },
            || tracked.get(),
            std::time::Duration::ZERO,
            3,
        )
        .await;
        assert_eq!(result, SweepResult::Closed);
        assert_eq!(attempts.get(), 2);
        assert!(!tracked.get());
        assert!(!tab_live.get());
    }

    #[tokio::test]
    async fn uncertain_close_transfers_cleanup_to_lifecycle_without_retrying() {
        let attempts = Cell::new(0);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            sweep_agent_tab(
                || async {
                    attempts.set(attempts.get() + 1);
                    Err("Browser is still closing; restart Sage before browsing again".into())
                },
                || true,
                std::time::Duration::ZERO,
                3,
            ),
        )
        .await;
        assert_eq!(result.unwrap(), SweepResult::LifecycleOwned);
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn repeated_definite_failure_has_a_bounded_sweep() {
        let attempts = Cell::new(0);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            sweep_agent_tab(
                || async {
                    attempts.set(attempts.get() + 1);
                    Err("native close failed".into())
                },
                || true,
                std::time::Duration::ZERO,
                3,
            ),
        )
        .await;
        assert_eq!(result.unwrap(), SweepResult::Exhausted);
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn concurrent_revoke_does_not_drop_lease_during_first_failed_close() {
        let registry = crate::acp::browser::attachment::BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        registry
            .attach_agent(
                project.clone(),
                chat.clone(),
                "session-1",
                "browser-8",
                "https://example.com/",
                1,
            )
            .unwrap();
        registry.agent_retire(&project, &chat, "session-1", 8);
        let attempts = Cell::new(0);
        let first = async {
            let _claim = registry.agent_claim_sweep("browser-8").unwrap();
            sweep_agent_tab(
                || async {
                    attempts.set(attempts.get() + 1);
                    if attempts.get() == 1 {
                        Err("definite close failure".into())
                    } else {
                        Ok(())
                    }
                },
                || registry.agent_has_label("browser-8"),
                std::time::Duration::from_millis(5),
                2,
            )
            .await
        };
        let second = async {
            assert_eq!(attempts.get(), 1);
            assert!(registry.agent_claim_sweep("browser-8").is_none());
            assert!(registry.agent_has_label("browser-8"));
        };
        let (result, ()) = tokio::join!(first, second);
        assert_eq!(result, SweepResult::Closed);
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn shutdown_waits_for_one_close_attempt_without_exceeding_budget() {
        let attempted = Cell::new(0);
        let started = std::time::Instant::now();
        let deadline = started + std::time::Duration::from_millis(200);
        close_agent_tabs_once_for_shutdown(
            ["browser-8", "browser-9"],
            deadline,
            |_| Some(()),
            |_| {
                attempted.set(attempted.get() + 1);
                std::thread::sleep(std::time::Duration::from_millis(10));
            },
        );
        assert_eq!(attempted.get(), 2);
        assert!(std::time::Instant::now() < deadline);
    }

    #[test]
    fn failed_shutdown_close_retains_retiring_lease_for_retry() {
        let registry = crate::acp::browser::attachment::BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        registry
            .attach_agent(
                project.clone(),
                chat.clone(),
                "session-1",
                "browser-8",
                "https://example.com/",
                1,
            )
            .unwrap();
        registry.agent_retire(&project, &chat, "session-1", 8);
        let quarantined = Cell::new(false);
        complete_shutdown_close(
            || Err("native close failed".to_owned()),
            || quarantined.set(true),
            || registry.revoke_tab("browser-8"),
        );
        assert!(quarantined.get());
        assert!(registry.agent_has_label("browser-8"));
    }
}
