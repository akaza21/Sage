use std::collections::{HashMap, VecDeque};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::acp::browser::{BrowserScope, permission::BrowserAuthority};
use crate::acp::contract::{SageChatId, SageProjectId};
use crate::{browser::BrowserState, state::AppState};
use tauri::State;

const MAX_ATTACHMENTS: usize = 8;
/// Authorities a settled navigation took out of a lease that Stop and Take
/// over still end, newest last. A wait outlives at most a few navigations.
const MAX_RETIRED: usize = 4;
/// Tabs that Agent-driven tabs opened and that joined, per session. They
/// never count against `openTab`'s cap and never become the default tab.
pub(crate) const MAX_POPUP_TABS_PER_SESSION: usize = 2;
pub(crate) const MAX_AGENT_TABS_PER_SESSION: usize = 4;
mod agent;
mod control;
#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod detach_tests;
mod developer;
mod popup;
mod resources;
pub(crate) use control::AgentControl;
#[cfg(windows)]
pub(crate) use developer::DeveloperCommand;
pub(crate) use popup::AgentAction;
#[cfg(windows)]
pub(crate) use resources::DeveloperScope;
pub(crate) use resources::DiagnosticScope;

#[derive(Clone)]
pub(crate) struct AttachmentLease {
    pub(crate) tab_label: String,
    pub(crate) tab_number: u64,
    pub(crate) url: String,
    pub(crate) tab_revision: u64,
    active: Arc<AtomicBool>,
    retiring: Arc<AtomicBool>,
    sweeping: Arc<AtomicBool>,
    user_control: Arc<AtomicBool>,
    authority: Arc<Mutex<Option<(String, BrowserAuthority)>>>,
    /// Authorities a settled navigation took out of the slot. Their permits
    /// count as invalidated by navigation, not stopped, so an in-flight wait
    /// keeps probing until Stop or Take over stops them too.
    retired: Arc<Mutex<VecDeque<BrowserAuthority>>>,
    /// The Agent session that last held this lease's authority, which a tab
    /// this one opens joins.
    last_session: Arc<Mutex<Option<String>>>,
    /// The tab that opened this one, for a popup that joined its opener's
    /// attachment: Take over, Hand back and Stop on the opener cover it.
    opener: Option<String>,
    /// The developer-mode grant (ADR-0009b): the session and origin the
    /// user approved, in memory only (`developer.rs`).
    developer: Arc<developer::DeveloperAccess>,
    resources: Arc<resources::Resources>,
}

pub(crate) struct AgentSweepClaim {
    sweeping: Arc<AtomicBool>,
}

impl Drop for AgentSweepClaim {
    fn drop(&mut self) {
        self.sweeping.store(false, Ordering::Release);
    }
}

impl AttachmentLease {
    pub(crate) fn is_live(&self) -> bool {
        self.active.load(Ordering::Acquire) && !self.retiring.load(Ordering::Acquire)
    }

    pub(crate) fn same_attachment(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.active, &other.active)
    }

    pub(crate) fn authority_for(&self, scope: &BrowserScope) -> Option<BrowserAuthority> {
        if scope.tab_id != self.tab_number {
            return None;
        }
        let mut slot = self.authority.lock().ok()?;
        if !self.is_live() || self.user_controls() {
            return None;
        }
        if let Some((session, authority)) = slot.as_ref() {
            if session == &scope.session_id {
                return Some(authority.clone());
            }
            authority.stop();
            self.release_resources();
            self.end_developer_mode();
        }
        let authority =
            BrowserAuthority::attached(scope.clone(), &self.url, self.tab_revision).ok()?;
        *slot = Some((scope.session_id.clone(), authority.clone()));
        if let Ok(mut last) = self.last_session.lock() {
            *last = Some(scope.session_id.clone());
        }
        Some(authority)
    }

    fn last_session(&self) -> Option<String> {
        self.last_session.lock().ok()?.clone()
    }

    fn stop(&self) {
        self.stop_retired();
        let Ok(slot) = self.authority.lock() else {
            self.active.store(false, Ordering::Release);
            self.release_resources();
            self.end_developer_mode();
            return;
        };
        self.active.store(false, Ordering::Release);
        if let Some((_, authority)) = slot.as_ref() {
            authority.stop();
        }
        self.release_resources();
        self.end_developer_mode();
    }

    /// Stops the authorities navigations took out of the slot, which ends
    /// any dispatch still running under them.
    fn stop_retired(&self) {
        if let Ok(mut retired) = self.retired.lock() {
            retired.drain(..).for_each(|authority| authority.stop());
        }
    }

    /// Takes the slot's authority out after a settled navigation, keeping it
    /// where Stop and Take over still reach it.
    fn retire_slot_authority(&self) {
        let Ok(mut slot) = self.authority.lock() else {
            return;
        };
        if let Some((_, authority)) = slot.take() {
            authority.navigation_started();
            if let Ok(mut retired) = self.retired.lock() {
                if retired.len() == MAX_RETIRED {
                    retired.pop_front();
                }
                retired.push_back(authority);
            }
        }
    }

    fn retire(&self) {
        self.retiring.store(true, Ordering::Release);
        self.stop_retired();
        if let Ok(slot) = self.authority.lock()
            && let Some((_, authority)) = slot.as_ref()
        {
            authority.stop();
        }
        self.release_resources();
        self.end_developer_mode();
    }

    fn navigation_started(&self) {
        if let Ok(slot) = self.authority.lock()
            && let Some((_, authority)) = slot.as_ref()
        {
            authority.navigation_started();
        }
    }
}

#[derive(Default)]
pub(crate) struct BrowserAttachments {
    developer_registry: Arc<developer::DeveloperRegistry>,
    entries: Mutex<HashMap<(SageProjectId, SageChatId), AttachmentLease>>,
    agent_entries: Mutex<HashMap<(SageProjectId, SageChatId, String, u64), AttachmentLease>>,
    /// Tabs an Agent-driven tab opened, by child label, until they join.
    popup_children: Mutex<HashMap<String, String>>,
    /// Counts hand backs per chat, so a paused call resumes only after one
    /// in its own chat, not after Stop, a detach or another chat's hand back.
    hand_backs: Mutex<HashMap<(SageProjectId, SageChatId), u64>>,
    /// Agent actions in flight on, or recently ended on, each tab: only a
    /// popup that opens around one joins (`popup.rs`).
    agent_actions: Mutex<HashMap<String, popup::AgentActivity>>,
}

impl BrowserAttachments {
    pub(crate) fn attach(
        &self,
        project: SageProjectId,
        chat: SageChatId,
        tab_label: &str,
        url: &str,
        tab_revision: u64,
    ) -> Result<(), ()> {
        let number = tab_label
            .strip_prefix("browser-")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|number| *number > 0)
            .ok_or(())?;
        let parsed = url::Url::parse(url).map_err(|_| ())?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(());
        }
        if self.agent_entries.lock().map_err(|_| ())?.iter().any(
            |((owner_project, owner_chat, _, _), lease)| {
                lease.tab_label == tab_label && (owner_project != &project || owner_chat != &chat)
            },
        ) {
            return Err(());
        }
        let mut entries = self.entries.lock().map_err(|_| ())?;
        let key = (project, chat);
        if !entries.contains_key(&key) && entries.len() >= MAX_ATTACHMENTS {
            return Err(());
        }
        if entries
            .iter()
            .any(|(other, lease)| other != &key && lease.tab_label == tab_label)
        {
            return Err(());
        }
        if let Some(previous) = entries.remove(&key) {
            previous.stop_or_hand_off();
            // Keep replacement unpublished until its old family is gone.
            // Registry lock order is user entries, then Agent entries.
            self.drop_joined_popups(&previous.tab_label);
        }
        entries.insert(
            key,
            AttachmentLease {
                tab_label: tab_label.to_owned(),
                tab_number: number,
                url: url.to_owned(),
                tab_revision,
                active: Arc::new(AtomicBool::new(true)),
                retiring: Arc::new(AtomicBool::new(false)),
                sweeping: Arc::new(AtomicBool::new(false)),
                user_control: Arc::new(AtomicBool::new(false)),
                authority: Arc::new(Mutex::new(None)),
                retired: Arc::default(),
                last_session: Arc::default(),
                opener: None,
                developer: Arc::new(developer::DeveloperAccess::new(
                    self.developer_registry.clone(),
                    tab_label,
                )),
                resources: Arc::default(),
            },
        );
        drop(entries);
        // Attaching is the user's new grant, so it also ends user control of
        // tabs this chat's Agent opened under the same label.
        if let Ok(agent_entries) = self.agent_entries.lock() {
            for lease in agent_entries
                .values()
                .filter(|lease| lease.tab_label == tab_label)
            {
                lease.set_user_control(false);
            }
        }
        Ok(())
    }

    pub(crate) fn resolve(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> Option<AttachmentLease> {
        let entries = self.entries.lock().ok()?;
        entries
            .get(&(project.clone(), chat.clone()))
            .filter(|lease| lease.is_live())
            .cloned()
    }

    pub(crate) fn detach(&self, project: &SageProjectId, chat: &SageChatId) -> Option<String> {
        let lease = self.entries.lock().ok().and_then(|mut entries| {
            let key = (project.clone(), chat.clone());
            // Keep the grant discoverable until its authority is stopped.
            entries.get(&key)?.stop_or_hand_off();
            entries.remove(&key)
        })?;
        self.drop_joined_popups(&lease.tab_label);
        Some(lease.tab_label)
    }

    pub(crate) fn revoke_tab(&self, tab_label: &str) {
        self.forget_popups(tab_label);
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|_, lease| {
                if lease.tab_label == tab_label {
                    lease.stop();
                    false
                } else {
                    true
                }
            });
        }
        // Stop on an opener also stops the popups that joined it.
        if let Ok(mut entries) = self.agent_entries.lock() {
            entries.retain(|_, lease| {
                if lease.tab_label == tab_label || lease.opener.as_deref() == Some(tab_label) {
                    lease.stop();
                    false
                } else {
                    true
                }
            });
        }
    }

    pub(crate) fn navigation_started(&self, tab_label: &str) {
        if let Ok(entries) = self.entries.lock() {
            for lease in entries
                .values()
                .filter(|lease| lease.tab_label == tab_label)
            {
                lease.navigation_started();
            }
        }
        if let Ok(entries) = self.agent_entries.lock() {
            for lease in entries
                .values()
                .filter(|lease| lease.tab_label == tab_label)
            {
                lease.navigation_started();
            }
        }
    }

    pub(crate) fn navigation_settled(&self, tab_label: &str, url: &str, revision: u64) {
        if let Ok(mut entries) = self.entries.lock() {
            for lease in entries
                .values_mut()
                .filter(|lease| lease.tab_label == tab_label)
            {
                lease.retire_slot_authority();
                lease.end_developer_mode_unless_on(url);
                lease.url = url.to_owned();
                lease.tab_revision = revision;
            }
        }
        if let Ok(mut entries) = self.agent_entries.lock() {
            for lease in entries
                .values_mut()
                .filter(|lease| lease.tab_label == tab_label)
            {
                lease.retire_slot_authority();
                lease.end_developer_mode_unless_on(url);
                lease.url = url.to_owned();
                lease.tab_revision = revision;
            }
        }
    }

    pub(crate) fn revoke_other_projects(&self, active_project: &SageProjectId) -> Vec<String> {
        let mut labels = Vec::new();
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|(project, _), lease| {
                if project == active_project {
                    true
                } else {
                    lease.stop();
                    labels.push(lease.tab_label.clone());
                    false
                }
            });
        }
        labels
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserAttachmentStatus {
    tab_id: String,
    url: String,
}

#[tauri::command]
pub(crate) fn attach_browser_tab(
    app: State<'_, AppState>,
    browser: State<'_, BrowserState>,
    project_id: SageProjectId,
    chat_id: SageChatId,
    tab_id: String,
) -> Result<BrowserAttachmentStatus, String> {
    let (url, revision) = browser.settled_tab(&tab_id)?;
    app.acp
        .attach_browser_tab(project_id, chat_id, &tab_id, &url, revision)
        .map_err(|_| "browser tab attachment unavailable".to_owned())?;
    Ok(BrowserAttachmentStatus { tab_id, url })
}

#[tauri::command]
pub(crate) fn detach_browser_tab(
    app: State<'_, AppState>,
    project_id: SageProjectId,
    chat_id: SageChatId,
) -> Result<(), String> {
    app.acp
        .detach_browser_tab(&project_id, &chat_id)
        .map_err(|_| "browser tab attachment unavailable".to_owned())
}

#[tauri::command]
pub(crate) fn stop_browser_tab(
    app: State<'_, AppState>,
    browser: State<'_, BrowserState>,
    tab_id: String,
) -> Result<(), String> {
    if browser.webview_for_tab(&tab_id).is_none() {
        return Err("browser tab unavailable".to_owned());
    }
    app.acp.revoke_browser_tab(&tab_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn browser_attachment_status(
    app: State<'_, AppState>,
    browser: State<'_, BrowserState>,
    project_id: SageProjectId,
    chat_id: SageChatId,
) -> Option<BrowserAttachmentStatus> {
    let lease = app.acp.browser_attachment(&project_id, &chat_id)?;
    let (url, revision) = browser.settled_tab(&lease.tab_label).ok()?;
    if url != lease.url || revision != lease.tab_revision {
        return None;
    }
    Some(BrowserAttachmentStatus {
        tab_id: lease.tab_label,
        url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_opened_tabs_are_session_bound_and_capped_without_replacing_user_attachment() {
        let registry = BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/",
                1,
            )
            .unwrap();
        for number in 8..12 {
            assert!(
                registry
                    .attach_agent(
                        project.clone(),
                        chat.clone(),
                        "session-1",
                        &format!("browser-{number}"),
                        "https://example.com/",
                        1
                    )
                    .is_ok()
            );
        }
        assert!(
            registry
                .attach_agent(
                    project.clone(),
                    chat.clone(),
                    "session-1",
                    "browser-12",
                    "https://example.com/",
                    1
                )
                .is_err()
        );
        assert_eq!(registry.resolve(&project, &chat).unwrap().tab_number, 7);
        assert_eq!(
            registry
                .agent_resolve(&project, &chat, "session-1", 9)
                .unwrap()
                .tab_number,
            9
        );
        assert!(
            registry
                .attach(
                    project.clone(),
                    SageChatId::parse("chat-two").unwrap(),
                    "browser-8",
                    "https://example.com/",
                    1
                )
                .is_err()
        );
        assert!(
            registry
                .agent_resolve(&project, &chat, "session-2", 9)
                .is_none()
        );
        registry.revoke_tab("browser-9");
        assert!(
            registry
                .agent_resolve(&project, &chat, "session-1", 9)
                .is_none()
        );
        assert!(
            registry
                .attach_agent(
                    project,
                    chat,
                    "session-1",
                    "browser-12",
                    "https://example.com/",
                    1
                )
                .is_ok()
        );
    }

    #[test]
    fn detaching_one_agent_tab_stops_only_its_lease() {
        let registry = BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        for number in [8, 9] {
            registry
                .attach_agent(
                    project.clone(),
                    chat.clone(),
                    "session-1",
                    &format!("browser-{number}"),
                    "https://example.com/",
                    1,
                )
                .unwrap();
        }
        let detached = registry
            .agent_resolve(&project, &chat, "session-1", 8)
            .unwrap();
        let labels = registry.agent_labels_for_chat(&project, &chat);
        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&"browser-8".to_owned()));
        assert!(labels.contains(&"browser-9".to_owned()));
        registry.agent_detach(&project, &chat, "session-1", 8);
        assert!(!detached.is_live());
        assert!(
            registry
                .agent_resolve(&project, &chat, "session-1", 8)
                .is_none()
        );
        assert!(
            registry
                .agent_resolve(&project, &chat, "session-1", 9)
                .is_some()
        );
    }

    #[test]
    fn retiring_agent_tab_rejects_later_turn_lookup_but_keeps_capacity_reserved() {
        let registry = BrowserAttachments::default();
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
        assert!(
            registry
                .agent_resolve(&project, &chat, "session-1", 8)
                .is_none()
        );
        assert!(
            registry
                .agent_latest(&project, &chat, "session-1")
                .is_none()
        );
        assert_eq!(registry.agent_count(&project, &chat, "session-1"), 1);
        assert!(registry.agent_has_label("browser-8"));
    }

    #[test]
    fn no_attach_is_inert_and_a_foreign_chat_cannot_resolve_another_chats_tab() {
        let registry = BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        let foreign = SageChatId::parse("chat-two").unwrap();
        assert!(registry.resolve(&project, &chat).is_none());
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/page",
                3,
            )
            .unwrap();
        assert!(registry.resolve(&project, &foreign).is_none());
        let lease = registry.resolve(&project, &chat).unwrap();
        assert_eq!(lease.tab_number, 7);
        assert!(lease.is_live());
        let _ = registry.detach(&project, &chat);
        assert!(!lease.is_live());
        assert!(registry.resolve(&project, &chat).is_none());
    }

    #[test]
    fn switching_projects_revokes_other_projects_browser_lease() {
        let registry = BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let other = SageProjectId::parse("project-two").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com",
                1,
            )
            .unwrap();
        let lease = registry.resolve(&project, &chat).unwrap();
        registry.revoke_other_projects(&other);
        assert!(!lease.is_live());
        assert!(registry.resolve(&project, &chat).is_none());
    }

    #[test]
    fn project_switch_keeps_retiring_agent_tab_tracked_for_service_cleanup() {
        let registry = BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let other = SageProjectId::parse("project-two").unwrap();
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
        registry.revoke_other_projects(&other);
        assert!(registry.agent_has_label("browser-8"));
    }

    #[test]
    fn duplicate_sweep_cannot_claim_a_lease_until_its_owner_finishes() {
        let registry = BrowserAttachments::default();
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
        let claim = registry.agent_claim_sweep("browser-8").unwrap();
        assert!(registry.agent_claim_sweep("browser-8").is_none());
        assert!(registry.agent_has_label("browser-8"));
        drop(claim);
        let panic = std::panic::catch_unwind(|| {
            let _claim = registry.agent_claim_sweep("browser-8").unwrap();
            panic!("cancelled sweep");
        });
        assert!(panic.is_err());
        assert!(registry.agent_claim_sweep("browser-8").is_some());
    }

    #[test]
    fn navigation_stops_authority_but_keeps_attached_tab() {
        let registry = BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/page",
                3,
            )
            .unwrap();
        let lease = registry.resolve(&project, &chat).unwrap();
        let authority = lease
            .authority_for(&BrowserScope {
                session_id: "session-1".into(),
                tab_id: 7,
            })
            .unwrap();
        registry.navigation_started("browser-7");
        assert!(registry.resolve(&project, &chat).is_some());
        assert!(
            authority
                .liveness_for(&crate::acp::browser::permission::BrowserPermissionPrompt {
                    scope: BrowserScope {
                        session_id: "session-1".into(),
                        tab_id: 7,
                    },
                    origin: "https://example.com".into(),
                    operation: "snapshotPage",
                })
                .is_none()
        );
        registry.navigation_settled("browser-7", "https://other.example/", 5);
        let settled = registry.resolve(&project, &chat).unwrap();
        assert_eq!(settled.url, "https://other.example/");
        assert_eq!(settled.tab_revision, 5);
        assert!(settled.is_live());
    }

    #[test]
    fn tab_scoped_stop_revokes_the_current_chat_after_reattachment() {
        let registry = BrowserAttachments::default();
        let project = SageProjectId::parse("project-one").unwrap();
        let first = SageChatId::parse("chat-one").unwrap();
        let second = SageChatId::parse("chat-two").unwrap();
        registry
            .attach(
                project.clone(),
                first.clone(),
                "browser-7",
                "https://example.com",
                1,
            )
            .unwrap();
        let _ = registry.detach(&project, &first);
        registry
            .attach(
                project.clone(),
                second.clone(),
                "browser-7",
                "https://example.com",
                1,
            )
            .unwrap();
        let current = registry.resolve(&project, &second).unwrap();
        registry.revoke_tab("browser-7");
        assert!(!current.is_live());
        assert!(registry.resolve(&project, &second).is_none());
    }
}
