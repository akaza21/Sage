//! Tabs an Agent-driven tab opens. A popup, a `target=_blank` link or a
//! `window.open` from a tab the Agent drives joins that tab's attachment:
//! it becomes an Agent tab of the same chat and session, with presence and
//! Take over controls, under the same authority and revocation rules, and it
//! starts under user control when the user holds the opener. Only children
//! the browser recorded for a live Agent-attached opener join, at most
//! `MAX_AGENT_TABS_PER_SESSION` per session; a tab the user opens never does.

use super::*;

/// Children waiting for their first settled page, by child label.
const MAX_POPUP_CHILDREN: usize = 16;

impl BrowserAttachments {
    /// Records `child` as opened by the Agent-driven `opener` and joins it
    /// now when its page has already settled. Returns whether it joined.
    pub(crate) fn adopt_popup(
        &self,
        opener: &str,
        child: &str,
        settled: Option<(&str, u64)>,
    ) -> bool {
        if opener == child || self.opener_lease(opener).is_none() {
            return false;
        }
        if let Ok(mut children) = self.popup_children.lock() {
            if children.len() >= MAX_POPUP_CHILDREN && !children.contains_key(child) {
                return false;
            }
            children.insert(child.to_owned(), opener.to_owned());
        }
        settled.is_some_and(|(url, revision)| self.join_popup(child, url, revision))
    }

    /// Joins a recorded child to its opener's attachment once the child
    /// shows a settled page. Returns whether it joined.
    pub(super) fn join_popup(&self, child: &str, url: &str, revision: u64) -> bool {
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
        let Some((project, chat, session, opener_lease)) = self.opener_lease(&opener) else {
            forget();
            return false;
        };
        let joined = self
            .attach_agent(
                project.clone(),
                chat.clone(),
                &session,
                child,
                url,
                revision,
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
    }

    /// The live Agent attachment on `opener` and the session that drives it.
    fn opener_lease(
        &self,
        opener: &str,
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
        registry
    }

    #[test]
    fn a_tab_the_agent_driven_tab_opens_joins_its_chat_and_session() {
        let registry = driven_opener();
        let (project, chat) = ids();
        assert!(!registry.adopt_popup("browser-7", "browser-9", None));
        assert_eq!(registry.agent_control("browser-9"), None);
        registry.navigation_settled("browser-9", "https://example.com/next", 3);
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
    fn a_child_opened_while_the_user_holds_the_opener_starts_taken_over() {
        let registry = driven_opener();
        assert!(registry.take_over("browser-7"));
        assert!(registry.adopt_popup(
            "browser-7",
            "browser-9",
            Some(("https://example.com/next", 3))
        ));
        let control = registry.agent_control("browser-9").unwrap();
        assert!(control.user_control);
        let (project, chat) = ids();
        assert!(registry.chat_under_user_control(&project, &chat));
    }

    #[test]
    fn no_scope_widening_for_unattached_or_unused_openers_or_stopped_ones() {
        // An opener with no attachment: the user's own browsing.
        let registry = BrowserAttachments::default();
        assert!(!registry.adopt_popup("browser-7", "browser-9", Some(("https://a.example/", 1))));
        assert_eq!(registry.agent_control("browser-9"), None);

        // Attached, but no Agent session has used it yet.
        let (project, chat) = ids();
        registry
            .attach(project, chat, "browser-7", "https://example.com/", 1)
            .unwrap();
        assert!(!registry.adopt_popup("browser-7", "browser-9", Some(("https://a.example/", 1))));
        assert_eq!(registry.agent_control("browser-9"), None);

        // Stop on the opener drops a child still waiting for its page.
        let registry = driven_opener();
        registry.adopt_popup("browser-7", "browser-9", None);
        registry.revoke_tab("browser-7");
        registry.navigation_settled("browser-9", "https://example.com/next", 3);
        assert_eq!(registry.agent_control("browser-9"), None);
    }

    #[test]
    fn joined_children_are_capped_per_session() {
        let registry = driven_opener();
        let joined = (10..20)
            .filter(|number| {
                registry.adopt_popup(
                    "browser-7",
                    &format!("browser-{number}"),
                    Some(("https://example.com/", 1)),
                )
            })
            .count();
        assert_eq!(joined, MAX_AGENT_TABS_PER_SESSION);
    }
}
