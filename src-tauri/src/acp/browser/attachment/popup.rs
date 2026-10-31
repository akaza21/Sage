//! Tabs an Agent-driven tab opens. A popup, a `target=_blank` link or a
//! `window.open` from a tab the Agent drives joins that tab's attachment:
//! it becomes an Agent tab of the same chat and session, with presence and
//! Take over controls, under the same authority and revocation rules, and it
//! starts under user control when the user holds the opener. Only children
//! the browser recorded for a live Agent-attached opener join, and only
//! while that session's turn runs (`turn_running`); a tab the user opens
//! never does. A joined tab is capped apart from `openTab`'s tabs, at
//! `MAX_POPUP_TABS_PER_SESSION`, is never the implicit default tab, and
//! follows its opener on Take over, Hand back and Stop.

use super::*;
use std::time::{Duration, Instant};

/// How long after an Agent action on the opener a popup may still join.
pub(crate) const AGENT_ACTION_WINDOW: Duration = Duration::from_secs(2);

/// Agent actions on one tab: how many run now, and when the last ended.
#[derive(Default)]
pub(crate) struct AgentActivity {
    in_flight: u32,
    ended: Option<Instant>,
}

/// An Agent click, key or navigation running on a tab; dropping it ends it.
pub(crate) struct AgentAction<'a> {
    registry: &'a BrowserAttachments,
    label: String,
}

impl Drop for AgentAction<'_> {
    fn drop(&mut self) {
        if let Ok(mut actions) = self.registry.agent_actions.lock()
            && let Some(activity) = actions.get_mut(&self.label)
        {
            activity.in_flight = activity.in_flight.saturating_sub(1);
            activity.ended = Some(Instant::now());
        }
    }
}

/// Children waiting for their first settled page, by child label.
pub(crate) const MAX_POPUP_CHILDREN: usize = 16;

/// Whether `session` of the chat has a current owner whose turn runs.
pub(crate) type TurnRunning<'a> = &'a dyn Fn(&SageProjectId, &SageChatId, &str) -> bool;

impl BrowserAttachments {
    /// Marks an Agent action that may open a popup as running on `label`.
    pub(crate) fn begin_agent_action(&self, label: &str) -> AgentAction<'_> {
        if let Ok(mut actions) = self.agent_actions.lock() {
            let activity = actions.entry(label.to_owned()).or_default();
            activity.in_flight = activity.in_flight.saturating_add(1);
        }
        AgentAction {
            registry: self,
            label: label.to_owned(),
        }
    }

    /// Whether an Agent action runs on `label` or ended at most
    /// `AGENT_ACTION_WINDOW` before `now`. A link the user clicks later in
    /// the turn opens a tab that stays the user's.
    pub(crate) fn agent_acted_recently(&self, label: &str, now: Instant) -> bool {
        self.agent_actions.lock().is_ok_and(|actions| {
            actions.get(label).is_some_and(|activity| {
                activity.in_flight > 0
                    || activity.ended.is_some_and(|ended| {
                        now.saturating_duration_since(ended) <= AGENT_ACTION_WINDOW
                    })
            })
        })
    }

    /// The popups that joined `opener`.
    pub(crate) fn joined_popups(&self, opener: &str) -> Vec<String> {
        self.agent_entries.lock().map_or_else(
            |_| Vec::new(),
            |entries| {
                entries
                    .values()
                    .filter(|lease| lease.opener.as_deref() == Some(opener))
                    .map(|lease| lease.tab_label.clone())
                    .collect()
            },
        )
    }

    /// Stops the popups that joined `opener` and forgets those still
    /// waiting, once the opener's attachment ended.
    pub(super) fn drop_joined_popups(&self, opener: &str) {
        self.forget_popups(opener);
        if let Ok(mut entries) = self.agent_entries.lock() {
            entries.retain(|_, lease| {
                if lease.opener.as_deref() == Some(opener) {
                    lease.stop();
                    false
                } else {
                    true
                }
            });
        }
    }

    /// Records `child` as opened by the Agent-driven `opener` and joins it
    /// now when its page has already settled. Returns whether it joined.
    pub(crate) fn adopt_popup(
        &self,
        opener: &str,
        child: &str,
        settled: Option<(&str, u64)>,
        turn_running: TurnRunning<'_>,
    ) -> bool {
        if opener == child
            || !self.agent_acted_recently(opener, Instant::now())
            || self.opener_lease(opener, turn_running).is_none()
        {
            return false;
        }
        if let Ok(mut children) = self.popup_children.lock() {
            if children.len() >= MAX_POPUP_CHILDREN && !children.contains_key(child) {
                return false;
            }
            children.insert(child.to_owned(), opener.to_owned());
        }
        settled.is_some_and(|(url, revision)| self.join_popup(child, url, revision, turn_running))
    }

    /// Joins a recorded child to its opener's attachment once the child
    /// shows a settled page. Returns whether it joined.
    pub(crate) fn join_popup(
        &self,
        child: &str,
        url: &str,
        revision: u64,
        turn_running: TurnRunning<'_>,
    ) -> bool {
        let Some(opener) = self
            .popup_children
            .lock()
            .ok()
            .and_then(|children| children.get(child).cloned())
        else {
            return false;
        };
        let forget = || {
            if let Ok(mut children) = self.popup_children.lock() {
                children.remove(child);
            }
        };
        if self.agent_control(child).is_some() {
            forget();
            return false;
        }
        let Some((project, chat, session, opener_lease)) = self.opener_lease(&opener, turn_running)
        else {
            forget();
            return false;
        };
        // A popup of a joined popup follows the first opener.
        let root = opener_lease.opener.clone().unwrap_or(opener);
        // Re-checked under the Agent tabs' lock (`attach_agent_tab`): a
        // detach of the opener that raced this join removes the child.
        let joined = self
            .attach_agent_tab(
                project.clone(),
                chat.clone(),
                &session,
                child,
                url,
                revision,
                Some((&root, &opener_lease)),
            )
            .is_ok();
        if joined
            && opener_lease.user_controls()
            && let Some(number) = child
                .strip_prefix("browser-")
                .and_then(|value| value.parse::<u64>().ok())
            && let Some(lease) = self.agent_resolve(&project, &chat, &session, number)
        {
            lease.set_user_control(true);
        }
        // A navigation that failed admission keeps the child waiting for
        // its next settled page.
        if joined
            || url::Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
        {
            forget();
        }
        joined
    }

    /// Drops children of or as `label`, which stopped or closed.
    pub(super) fn forget_popups(&self, label: &str) {
        if let Ok(mut children) = self.popup_children.lock() {
            children.retain(|child, opener| child != label && opener != label);
        }
        if let Ok(mut actions) = self.agent_actions.lock()
            && actions
                .get(label)
                .is_some_and(|activity| activity.in_flight == 0)
        {
            actions.remove(label);
        }
    }

    /// The live Agent attachment on `opener` and the session that drives it.
    fn opener_lease(
        &self,
        opener: &str,
        turn_running: TurnRunning<'_>,
    ) -> Option<(SageProjectId, SageChatId, String, AttachmentLease)> {
        let user = self.entries.lock().ok().and_then(|entries| {
            entries.iter().find_map(|((project, chat), lease)| {
                (lease.tab_label == opener && lease.is_live()).then(|| {
                    lease
                        .last_session()
                        .map(|session| (project.clone(), chat.clone(), session, lease.clone()))
                })?
            })
        });
        user.or_else(|| {
            self.agent_entries.lock().ok().and_then(|entries| {
                entries
                    .iter()
                    .find_map(|((project, chat, session, _), lease)| {
                        (lease.tab_label == opener && lease.is_live()).then(|| {
                            (
                                project.clone(),
                                chat.clone(),
                                session.clone(),
                                lease.clone(),
                            )
                        })
                    })
            })
        })
        .filter(|(project, chat, session, _)| turn_running(project, chat, session))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn running(_: &SageProjectId, _: &SageChatId, _: &str) -> bool {
        true
    }

    fn ended(_: &SageProjectId, _: &SageChatId, _: &str) -> bool {
        false
    }

    const PAGE: Option<(&str, u64)> = Some(("https://example.com/next", 3));

    #[test]
    fn recovered_chat_can_reattach_a_joined_tab_and_detach_the_old_family() {
        let registry = driven_opener();
        let (project, chat) = ids();
        assert!(registry.adopt_popup("browser-7", "browser-9", PAGE, &running));
        assert!(registry.adopt_popup("browser-7", "browser-10", PAGE, &running));
        let old_child = registry
            .agent_resolve(&project, &chat, "session-1", 9)
            .unwrap();
        assert!(registry.take_over("browser-7"));
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-9",
                "https://example.com/next",
                3,
            )
            .unwrap();
        assert!(!old_child.is_live());
        assert!(registry.agent_control("browser-10").is_none());
        assert!(!registry.chat_under_user_control(&project, &chat));
        let fresh = registry.resolve(&project, &chat).unwrap();
        assert!(
            fresh
                .authority_for(&BrowserScope {
                    session_id: "session-recovered".into(),
                    tab_id: 9
                })
                .is_some()
        );
    }

    #[test]
    fn replacing_the_same_opener_rejects_stale_joins_and_keeps_new_children() {
        let registry = driven_opener();
        let (project, chat) = ids();
        let old_opener = registry.resolve(&project, &chat).unwrap();
        assert!(registry.adopt_popup("browser-7", "browser-9", PAGE, &running));
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-7",
                "https://example.com/",
                1,
            )
            .unwrap();
        assert!(!old_opener.is_live());
        assert!(registry.agent_control("browser-9").is_none());
        // A join that captured the old opener before replacement cannot land.
        assert!(
            registry
                .attach_agent_tab(
                    project.clone(),
                    chat.clone(),
                    "session-1",
                    "browser-10",
                    "https://example.com/next",
                    3,
                    Some(("browser-7", &old_opener))
                )
                .is_err()
        );
        let fresh = registry.resolve(&project, &chat).unwrap();
        assert!(fresh.authority_for(&scope(7)).is_some());
        drop(registry.begin_agent_action("browser-7"));
        assert!(registry.adopt_popup("browser-7", "browser-11", PAGE, &running));
        assert!(
            registry
                .agent_resolve(&project, &chat, "session-1", 11)
                .is_some()
        );
    }

    #[test]
    fn failed_join_keeps_the_child_plain_and_the_opener_permission_live() {
        let registry = driven_opener();
        let (project, chat) = ids();
        let opener = registry.resolve(&project, &chat).unwrap();
        let authority = opener.authority_for(&scope(7)).unwrap();
        let prompt = crate::acp::browser::permission::BrowserPermissionPrompt {
            scope: scope(7),
            origin: "https://example.com".into(),
            operation: "snapshotPage",
        };
        let live = authority.liveness_for(&prompt).unwrap();
        assert!(registry.adopt_popup("browser-7", "browser-9", PAGE, &running));
        assert!(registry.adopt_popup("browser-7", "browser-10", PAGE, &running));
        assert!(!registry.adopt_popup("browser-7", "browser-11", PAGE, &running));
        assert_eq!(registry.agent_control("browser-11"), None);
        assert!(live());
        assert!(opener.is_live());
        assert!(
            registry
                .resolve(&project, &chat)
                .unwrap()
                .same_attachment(&opener)
        );
    }

    /// The user attached `browser-7` and the Agent has used it.
    fn driven_opener() -> BrowserAttachments {
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
        assert!(lease.authority_for(&scope(7)).is_some());
        // The Agent just clicked on it.
        drop(registry.begin_agent_action("browser-7"));
        registry
    }

    #[test]
    fn a_tab_the_agent_driven_tab_opens_joins_its_chat_and_session() {
        let registry = driven_opener();
        let (project, chat) = ids();
        assert!(!registry.adopt_popup("browser-7", "browser-9", None, &running));
        assert_eq!(registry.agent_control("browser-9"), None);
        registry.navigation_settled("browser-9", "https://example.com/next", 3);
        assert_eq!(
            registry.agent_control("browser-9"),
            None,
            "joins only via join_popup"
        );
        assert!(registry.join_popup("browser-9", "https://example.com/next", 3, &running));
        assert_eq!(
            registry.agent_control("browser-9"),
            Some(AgentControl {
                project_id: project.clone(),
                chat_id: chat.clone(),
                user_control: false,
            })
        );
        let child = registry
            .agent_resolve(&project, &chat, "session-1", 9)
            .unwrap();
        assert!(child.authority_for(&scope(9)).is_some());
        // Stop on the child ends its attachment like any Agent tab.
        registry.revoke_tab("browser-9");
        assert_eq!(registry.agent_control("browser-9"), None);
    }

    #[test]
    fn no_join_once_the_turn_or_session_ended() {
        let registry = driven_opener();
        assert!(!registry.adopt_popup("browser-7", "browser-9", PAGE, &ended));
        assert_eq!(registry.agent_control("browser-9"), None);

        // Recorded while the turn ran, settled after it ended.
        let registry = driven_opener();
        assert!(!registry.adopt_popup("browser-7", "browser-9", None, &running));
        assert!(!registry.join_popup("browser-9", "https://example.com/next", 3, &ended));
        assert_eq!(registry.agent_control("browser-9"), None);
        assert!(
            !registry.join_popup("browser-9", "https://example.com/next", 4, &running),
            "a refused child is forgotten"
        );
    }

    #[test]
    fn a_joined_popup_is_never_the_default_tab_nor_counts_against_open_tab() {
        let registry = driven_opener();
        let (project, chat) = ids();
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
        drop(registry.begin_agent_action("browser-8"));
        assert!(registry.adopt_popup("browser-8", "browser-12", PAGE, &running));
        assert!(registry.agent_control("browser-12").is_some());
        assert_eq!(
            registry
                .agent_latest(&project, &chat, "session-1")
                .map(|lease| lease.tab_number),
            Some(8)
        );
        assert_eq!(registry.agent_count(&project, &chat, "session-1"), 1);
        // openTab still has its full cap.
        for number in 20..20 + MAX_AGENT_TABS_PER_SESSION as u64 - 1 {
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
        assert_eq!(
            registry.agent_count(&project, &chat, "session-1"),
            MAX_AGENT_TABS_PER_SESSION
        );
    }

    #[test]
    fn a_child_opened_while_the_user_holds_the_opener_starts_taken_over() {
        let registry = driven_opener();
        assert!(registry.take_over("browser-7"));
        assert!(registry.adopt_popup("browser-7", "browser-9", PAGE, &running));
        let control = registry.agent_control("browser-9").unwrap();
        assert!(control.user_control);
        let (project, chat) = ids();
        assert!(registry.chat_under_user_control(&project, &chat));
    }

    #[test]
    fn hand_back_take_over_and_stop_on_the_opener_cover_its_children() {
        let registry = driven_opener();
        let (project, chat) = ids();
        assert!(registry.take_over("browser-7"));
        assert!(registry.adopt_popup("browser-7", "browser-9", PAGE, &running));
        let before = registry.hand_back_generation(&project, &chat);
        assert!(registry.hand_back("browser-7"));
        assert_eq!(registry.hand_back_generation(&project, &chat), before + 1);
        assert!(!registry.agent_control("browser-9").unwrap().user_control);
        assert!(!registry.chat_under_user_control(&project, &chat));

        assert!(registry.take_over("browser-7"));
        assert!(registry.agent_control("browser-9").unwrap().user_control);
        assert!(registry.hand_back("browser-7"));

        // Grandchildren follow the first opener.
        let child = registry
            .agent_resolve(&project, &chat, "session-1", 9)
            .unwrap();
        assert!(child.authority_for(&scope(9)).is_some());
        drop(registry.begin_agent_action("browser-9"));
        assert!(registry.adopt_popup("browser-9", "browser-10", PAGE, &running));
        registry.revoke_tab("browser-7");
        assert_eq!(registry.agent_control("browser-9"), None);
        assert_eq!(registry.agent_control("browser-10"), None);
        assert!(child.authority_for(&scope(9)).is_none());
    }

    #[test]
    fn deny_on_one_tab_while_the_user_holds_another_is_not_a_take_over() {
        let registry = driven_opener();
        let (project, chat) = ids();
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
        assert!(registry.take_over("browser-8"));
        assert!(registry.chat_under_user_control(&project, &chat));
        // The call on browser-7 was denied: browser-7 itself is not held.
        assert!(!registry.agent_control("browser-7").unwrap().user_control);
        let generation = registry.hand_back_generation(&project, &chat);
        registry.revoke_tab("browser-8");
        assert!(!registry.chat_under_user_control(&project, &chat));
        assert_eq!(
            registry.hand_back_generation(&project, &chat),
            generation,
            "Stop is not a hand back"
        );
    }

    #[test]
    fn no_scope_widening_for_unattached_or_unused_openers_or_stopped_ones() {
        // An opener with no attachment: the user's own browsing.
        let registry = BrowserAttachments::default();
        let page = Some(("https://a.example/", 1));
        assert!(!registry.adopt_popup("browser-7", "browser-9", page, &running));
        assert_eq!(registry.agent_control("browser-9"), None);

        // Attached, but no Agent session has used it yet.
        let (project, chat) = ids();
        registry
            .attach(project, chat, "browser-7", "https://example.com/", 1)
            .unwrap();
        assert!(!registry.adopt_popup("browser-7", "browser-9", page, &running));
        assert_eq!(registry.agent_control("browser-9"), None);

        // Stop on the opener drops a child still waiting for its page.
        let registry = driven_opener();
        registry.adopt_popup("browser-7", "browser-9", None, &running);
        registry.revoke_tab("browser-7");
        assert!(!registry.join_popup("browser-9", "https://example.com/next", 3, &running));
        assert_eq!(registry.agent_control("browser-9"), None);
    }

    #[test]
    fn joined_children_are_capped_per_session() {
        let registry = driven_opener();
        let joined = (10..20)
            .filter(|number| {
                registry.adopt_popup("browser-7", &format!("browser-{number}"), PAGE, &running)
            })
            .count();
        assert_eq!(joined, MAX_POPUP_TABS_PER_SESSION);
    }

    #[test]
    fn children_waiting_for_a_page_are_capped() {
        let registry = driven_opener();
        for number in 0..MAX_POPUP_CHILDREN as u64 {
            registry.adopt_popup(
                "browser-7",
                &format!("browser-{}", 100 + number),
                None,
                &running,
            );
        }
        let extra = "browser-999";
        registry.adopt_popup("browser-7", extra, None, &running);
        assert!(
            !registry.join_popup(extra, "https://example.com/", 1, &running),
            "the seventeenth waiting child was never recorded"
        );
        assert!(registry.join_popup("browser-100", "https://example.com/", 1, &running));
    }
    #[test]
    fn a_hand_back_in_another_chat_does_not_move_this_chats_generation() {
        let registry = driven_opener();
        let (project, chat) = ids();
        let other = SageChatId::parse("chat-two").unwrap();
        registry
            .attach(
                project.clone(),
                other.clone(),
                "browser-8",
                "https://example.com/",
                1,
            )
            .unwrap();
        assert!(registry.take_over("browser-8"));
        let mine = registry.hand_back_generation(&project, &chat);
        assert!(registry.hand_back("browser-8"));
        assert_eq!(registry.hand_back_generation(&project, &chat), mine);
        assert_eq!(registry.hand_back_generation(&project, &other), 1);
    }

    #[test]
    fn detaching_the_opener_stops_its_children_and_forgets_waiting_ones() {
        // The user detaches the opener they attached.
        let registry = driven_opener();
        let (project, chat) = ids();
        assert!(registry.adopt_popup("browser-7", "browser-9", PAGE, &running));
        assert!(!registry.adopt_popup("browser-7", "browser-10", None, &running));
        let child = registry
            .agent_resolve(&project, &chat, "session-1", 9)
            .unwrap();
        assert_eq!(
            registry.detach(&project, &chat).as_deref(),
            Some("browser-7")
        );
        assert!(!child.is_live());
        assert_eq!(registry.agent_control("browser-9"), None);
        assert!(!registry.join_popup("browser-10", "https://example.com/", 1, &running));
        assert_eq!(registry.agent_control("browser-10"), None);

        // An Agent tab that opened a popup is detached.
        let registry = BrowserAttachments::default();
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
        drop(registry.begin_agent_action("browser-8"));
        assert!(registry.adopt_popup("browser-8", "browser-12", PAGE, &running));
        registry.agent_detach(&project, &chat, "session-1", 8);
        assert_eq!(registry.agent_control("browser-12"), None);
    }

    #[test]
    fn a_join_racing_the_openers_detach_is_refused() {
        // The join found the opener live, then the detach stopped it before
        // the join took the Agent tabs' lock.
        let registry = driven_opener();
        let (project, chat) = ids();
        let opener = registry.resolve(&project, &chat).unwrap();
        registry.detach(&project, &chat);
        assert!(
            registry
                .attach_agent_tab(
                    project.clone(),
                    chat.clone(),
                    "session-1",
                    "browser-9",
                    "https://example.com/",
                    1,
                    Some(("browser-7", &opener)),
                )
                .is_err()
        );
        assert_eq!(registry.agent_control("browser-9"), None);
    }

    #[test]
    fn only_a_popup_around_an_agent_action_joins() {
        let registry = driven_opener();
        let now = Instant::now();
        assert!(registry.agent_acted_recently("browser-7", now));
        assert!(
            !registry.agent_acted_recently("browser-7", now + AGENT_ACTION_WINDOW * 2),
            "a link the user clicks later in the turn"
        );
        let action = registry.begin_agent_action("browser-7");
        assert!(registry.agent_acted_recently("browser-7", now + AGENT_ACTION_WINDOW * 2));
        drop(action);

        // No Agent action on the opener at all.
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
        assert!(lease.authority_for(&scope(7)).is_some());
        assert!(!registry.adopt_popup("browser-7", "browser-9", PAGE, &running));
        assert_eq!(registry.agent_control("browser-9"), None);
    }
}
