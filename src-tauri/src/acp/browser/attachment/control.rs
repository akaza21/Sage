//! Take over and hand back: the user's pause of Agent control on one tab.
//!
//! Taking over stops every authority on the tab, so a pending permission card
//! declines and an in-flight operation ends `denied` (or `outcomeUnknown` once
//! issued). The attachments stay, but they admit no authority until the user
//! hands the tab back. Handing back admits a fresh authority with no origin
//! grant, so the Agent's next call asks again. A new user attachment starts
//! without user control: attaching is itself the new grant.

use super::*;

impl AttachmentLease {
    pub(crate) fn user_controls(&self) -> bool {
        self.user_control.load(Ordering::Acquire)
    }

    /// Flips under the authority slot lock so `authority_for` cannot admit an
    /// authority between the flag and the stop.
    pub(super) fn set_user_control(&self, on: bool) {
        let Ok(mut slot) = self.authority.lock() else {
            self.user_control.store(on, Ordering::Release);
            if on {
                self.active.store(false, Ordering::Release);
            }
            return;
        };
        self.user_control.store(on, Ordering::Release);
        if on {
            if let Some((_, authority)) = slot.take() {
                authority.stop();
            }
            self.stop_retired();
        }
    }
}

/// Who may drive a tab, for the chrome's Stop, Take over and Hand back.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentControl {
    pub(crate) project_id: SageProjectId,
    pub(crate) chat_id: SageChatId,
    pub(crate) user_control: bool,
}

impl BrowserAttachments {
    /// The live Agent attachment on `tab_label`, the user's own first, so
    /// the chrome shows its controls for as long as an Agent is attached,
    /// not only while it acts.
    pub(crate) fn agent_control(&self, tab_label: &str) -> Option<AgentControl> {
        let found = |project: &SageProjectId, chat: &SageChatId, lease: &AttachmentLease| {
            (lease.tab_label == tab_label && lease.is_live()).then(|| AgentControl {
                project_id: project.clone(),
                chat_id: chat.clone(),
                user_control: lease.user_controls(),
            })
        };
        if let Some(control) = self.entries.lock().ok().and_then(|entries| {
            entries
                .iter()
                .find_map(|((project, chat), lease)| found(project, chat, lease))
        }) {
            return Some(control);
        }
        self.agent_entries.lock().ok().and_then(|entries| {
            entries
                .iter()
                .find_map(|((project, chat, _, _), lease)| found(project, chat, lease))
        })
    }

    /// Whether a live attachment of this chat, the user's or one its Agent
    /// opened, is under user control.
    pub(crate) fn chat_under_user_control(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> bool {
        let held = |lease: &AttachmentLease| lease.is_live() && lease.user_controls();
        self.entries.lock().is_ok_and(|entries| {
            entries
                .get(&(project.clone(), chat.clone()))
                .is_some_and(held)
        }) || self.agent_entries.lock().is_ok_and(|entries| {
            entries
                .iter()
                .any(|((owner_project, owner_chat, _, _), lease)| {
                    owner_project == project && owner_chat == chat && held(lease)
                })
        })
    }

    /// Returns whether the tab had an Agent attachment to pause.
    pub(crate) fn take_over(&self, tab_label: &str) -> bool {
        self.set_user_control(tab_label, true)
    }

    /// Returns whether the tab still had an Agent attachment to resume.
    pub(crate) fn hand_back(&self, tab_label: &str) -> bool {
        let found = self.set_user_control(tab_label, false);
        if found {
            self.hand_backs.fetch_add(1, Ordering::AcqRel);
        }
        found
    }

    /// Moves on each hand back; a paused call resumes only once it moved.
    pub(crate) fn hand_back_generation(&self) -> u64 {
        self.hand_backs.load(Ordering::Acquire)
    }

    fn set_user_control(&self, tab_label: &str, on: bool) -> bool {
        let mut found = false;
        if let Ok(entries) = self.entries.lock() {
            for lease in entries
                .values()
                .filter(|lease| lease.tab_label == tab_label)
            {
                lease.set_user_control(on);
                found = true;
            }
        }
        // Popups that joined this tab follow it, so handing back the opener
        // never leaves a child holding the chat paused.
        if let Ok(entries) = self.agent_entries.lock() {
            for lease in entries.values() {
                if lease.tab_label == tab_label {
                    lease.set_user_control(on);
                    found = true;
                } else if lease.opener.as_deref() == Some(tab_label) {
                    lease.set_user_control(on);
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::browser::{
        BrowserAction, BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest,
        BrowserResult, ElementHandle,
        permission::{
            BrowserCapabilityDispatch, BrowserDispatchPermit, BrowserPermissionChoice,
            BrowserPermissionGate, BrowserPermissionPrompt, PendingBrowserResult,
        },
    };
    use std::{cell::Cell, rc::Rc};

    /// Issues a click under the permit; `during` runs after the issue, before
    /// the outcome comes back, as a user's click on Take over would.
    struct Port<F: FnMut()> {
        acted: Rc<Cell<bool>>,
        during: F,
    }

    impl<F: FnMut()> BrowserCapabilityDispatch for Port<F> {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            permit.with_live_lock(|| {
                permit.mark_issued();
                self.acted.set(true);
                Ok(())
            })?;
            (self.during)();
            Ok(Box::new(|| Ok(BrowserResult::Act { applied: true })))
        }
    }

    fn click(tab_id: u64) -> BrowserRequest {
        BrowserRequest {
            scope: scope(tab_id),
            command: BrowserCommand::Act {
                action: BrowserAction::Click {
                    handle: ElementHandle::from_token("opaque".into()),
                },
            },
        }
    }

    fn ids() -> (SageProjectId, SageChatId) {
        (
            SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(),
        )
    }

    fn scope(tab_id: u64) -> BrowserScope {
        BrowserScope {
            session_id: "session-1".into(),
            tab_id,
        }
    }

    fn prompt(tab_id: u64) -> BrowserPermissionPrompt {
        BrowserPermissionPrompt {
            scope: scope(tab_id),
            origin: "https://example.com".into(),
            operation: "act",
        }
    }

    #[test]
    fn take_over_stops_the_live_authority_until_the_user_hands_back() {
        let registry = BrowserAttachments::default();
        let (project, chat) = ids();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/",
                1,
            )
            .unwrap();
        let lease = registry.resolve(&project, &chat).unwrap();
        let authority = lease.authority_for(&scope(7)).unwrap();
        let pending_card_live = authority.liveness_for(&prompt(7)).unwrap();
        assert!(pending_card_live());

        assert!(registry.take_over("browser-7"));
        assert!(!pending_card_live(), "a pending permission card declines");
        assert!(authority.liveness_for(&prompt(7)).is_none());
        assert!(lease.user_controls());
        assert!(lease.authority_for(&scope(7)).is_none());
        // The attachment survives a user navigation while taken over.
        registry.navigation_settled("browser-7", "https://example.org/", 2);
        let lease = registry.resolve(&project, &chat).unwrap();
        assert!(lease.authority_for(&scope(7)).is_none());

        assert!(registry.hand_back("browser-7"));
        assert!(!lease.user_controls());
        let fresh = lease.authority_for(&scope(7)).unwrap();
        assert!(fresh.liveness_for(&prompt(7)).is_some());
        assert!(
            !pending_card_live(),
            "handing back never revives the old card"
        );
    }

    #[test]
    fn take_over_covers_agent_opened_tabs_and_needs_an_attachment() {
        let registry = BrowserAttachments::default();
        let (project, chat) = ids();
        assert!(!registry.take_over("browser-8"));
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
        let lease = registry
            .agent_resolve(&project, &chat, "session-1", 8)
            .unwrap();
        assert!(lease.authority_for(&scope(8)).is_some());
        assert!(registry.take_over("browser-8"));
        assert!(lease.authority_for(&scope(8)).is_none());
        assert!(
            registry
                .agent_resolve(&project, &chat, "session-1", 8)
                .is_some()
        );
        registry.revoke_tab("browser-8");
        assert!(!registry.hand_back("browser-8"), "Stop ends the attachment");
    }

    #[test]
    fn the_chrome_reads_who_controls_a_tab_from_rust_while_attached() {
        let registry = BrowserAttachments::default();
        let (project, chat) = ids();
        assert_eq!(registry.agent_control("browser-8"), None);
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
        let control = |user_control| {
            Some(AgentControl {
                project_id: project.clone(),
                chat_id: chat.clone(),
                user_control,
            })
        };
        assert_eq!(registry.agent_control("browser-8"), control(false));
        assert!(!registry.chat_under_user_control(&project, &chat));
        registry.take_over("browser-8");
        assert_eq!(registry.agent_control("browser-8"), control(true));
        assert!(
            registry.chat_under_user_control(&project, &chat),
            "the Agent opens no parallel tab while the user holds one"
        );
        // The user attaching the same tab is a new grant for every lease on it.
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-8",
                "https://example.com/",
                1,
            )
            .unwrap();
        assert_eq!(registry.agent_control("browser-8"), control(false));
        assert!(!registry.chat_under_user_control(&project, &chat));
        let agent = registry
            .agent_resolve(&project, &chat, "session-1", 8)
            .unwrap();
        assert!(agent.authority_for(&scope(8)).is_some());
        registry.revoke_tab("browser-8");
        assert_eq!(registry.agent_control("browser-8"), None);
    }

    #[test]
    fn a_new_user_attachment_ends_user_control() {
        let registry = BrowserAttachments::default();
        let (project, chat) = ids();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/",
                1,
            )
            .unwrap();
        assert!(registry.take_over("browser-7"));
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/",
                1,
            )
            .unwrap();
        let lease = registry.resolve(&project, &chat).unwrap();
        assert!(!lease.user_controls());
        assert!(lease.authority_for(&scope(7)).is_some());
    }

    #[test]
    fn take_over_while_the_card_waits_denies_without_acting() {
        let registry = Rc::new(BrowserAttachments::default());
        let (project, chat) = ids();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/",
                1,
            )
            .unwrap();
        let lease = registry.resolve(&project, &chat).unwrap();
        let acted = Rc::new(Cell::new(false));
        let user = registry.clone();
        let mut gate = BrowserPermissionGate::new(
            Port {
                acted: acted.clone(),
                during: || {},
            },
            move |_: &BrowserPermissionPrompt| {
                user.take_over("browser-7");
                BrowserPermissionChoice::AllowOnce
            },
            lease.authority_for(&scope(7)).unwrap(),
        );
        assert_eq!(gate.perform(&click(7)), Err(BrowserFailure::Denied));
        assert!(!acted.get());
    }

    /// Keeps the dispatch permit, as a wait in flight does, so the test can
    /// ask whether it is stopped after the gate returns.
    struct Keeps(Rc<std::cell::RefCell<Option<BrowserDispatchPermit>>>);

    impl BrowserCapabilityDispatch for Keeps {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            permit: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            *self.0.borrow_mut() = Some(permit);
            Ok(Box::new(|| Ok(BrowserResult::Act { applied: true })))
        }
    }

    fn stopped(permit: &BrowserDispatchPermit) -> bool {
        !permit.is_live() && !permit.was_invalidated_by_navigation()
    }

    #[test]
    fn take_over_and_stop_end_a_dispatch_a_navigation_already_took_out() {
        for take_over in [true, false] {
            let registry = BrowserAttachments::default();
            let (project, chat) = ids();
            registry
                .attach(
                    project.clone(),
                    chat.clone(),
                    "browser-7",
                    "https://example.com/",
                    1,
                )
                .unwrap();
            let lease = registry.resolve(&project, &chat).unwrap();
            let kept = Rc::new(std::cell::RefCell::new(None));
            let mut gate = BrowserPermissionGate::new(
                Keeps(kept.clone()),
                |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowOnce,
                lease.authority_for(&scope(7)).unwrap(),
            );
            let _ = gate.perform(&click(7));
            let permit = kept.borrow_mut().take().unwrap();
            registry.navigation_settled("browser-7", "https://example.com/next", 2);
            assert!(
                !stopped(&permit),
                "a navigation alone lets a wait keep probing"
            );
            if take_over {
                assert!(registry.take_over("browser-7"));
            } else {
                registry.revoke_tab("browser-7");
            }
            assert!(stopped(&permit), "take over={take_over}");
        }
    }

    #[test]
    fn take_over_after_issue_reports_an_unknown_outcome() {
        let registry = Rc::new(BrowserAttachments::default());
        let (project, chat) = ids();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/",
                1,
            )
            .unwrap();
        let lease = registry.resolve(&project, &chat).unwrap();
        let acted = Rc::new(Cell::new(false));
        let user = registry.clone();
        let mut gate = BrowserPermissionGate::new(
            Port {
                acted: acted.clone(),
                during: move || {
                    user.take_over("browser-7");
                },
            },
            |_: &BrowserPermissionPrompt| BrowserPermissionChoice::AllowForOriginSession,
            lease.authority_for(&scope(7)).unwrap(),
        );
        assert_eq!(gate.perform(&click(7)), Err(BrowserFailure::OutcomeUnknown));
        assert!(acted.get());
        registry.hand_back("browser-7");
        let fresh = lease.authority_for(&scope(7)).unwrap();
        let prompted = Rc::new(Cell::new(false));
        let asked = prompted.clone();
        let mut again = BrowserPermissionGate::new(
            Port {
                acted: Rc::new(Cell::new(false)),
                during: || {},
            },
            move |_: &BrowserPermissionPrompt| {
                asked.set(true);
                BrowserPermissionChoice::AllowOnce
            },
            fresh,
        );
        assert!(again.perform(&click(7)).is_ok());
        assert!(prompted.get(), "the origin grant did not survive take over");
    }
}
