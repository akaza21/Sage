use super::*;

impl BrowserAttachments {
    pub(crate) fn attach_agent(
        &self,
        project: SageProjectId,
        chat: SageChatId,
        session: &str,
        tab_label: &str,
        url: &str,
        revision: u64,
    ) -> Result<(), ()> {
        self.attach_agent_tab(project, chat, session, tab_label, url, revision, None)
    }

    /// Attaches an Agent tab; `opener` names the Agent-driven tab a joined
    /// popup came from, which caps it separately from `openTab`'s tabs.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn attach_agent_tab(
        &self,
        project: SageProjectId,
        chat: SageChatId,
        session: &str,
        tab_label: &str,
        url: &str,
        revision: u64,
        opener: Option<(&str, &AttachmentLease)>,
    ) -> Result<(), ()> {
        let cap = if opener.is_some() {
            MAX_POPUP_TABS_PER_SESSION
        } else {
            MAX_AGENT_TABS_PER_SESSION
        };
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
        let mut entries = self.agent_entries.lock().map_err(|_| ())?;
        if entries
            .iter()
            .filter(|((p, c, s, _), lease)| {
                p == &project
                    && c == &chat
                    && s == session
                    && lease.opener.is_some() == opener.is_some()
            })
            .count()
            >= cap
            // Under this lock, so a detach of the opener that stopped it
            // first either refuses this join or removes the child after.
            || opener.is_some_and(|(_, lease)| !lease.is_live())
            || entries.keys().any(|(_, _, _, id)| *id == number)
        {
            return Err(());
        }
        entries.insert(
            (project, chat, session.to_owned(), number),
            AttachmentLease {
                tab_label: tab_label.to_owned(),
                tab_number: number,
                url: url.to_owned(),
                tab_revision: revision,
                active: Arc::new(AtomicBool::new(true)),
                retiring: Arc::new(AtomicBool::new(false)),
                sweeping: Arc::new(AtomicBool::new(false)),
                user_control: Arc::new(AtomicBool::new(false)),
                authority: Arc::new(Mutex::new(None)),
                retired: Arc::default(),
                last_session: Arc::default(),
                opener: opener.map(|(label, _)| label.to_owned()),
            },
        );
        Ok(())
    }

    pub(crate) fn agent_resolve(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
        tab_number: u64,
    ) -> Option<AttachmentLease> {
        self.agent_entries
            .lock()
            .ok()?
            .get(&(
                project.clone(),
                chat.clone(),
                session.to_owned(),
                tab_number,
            ))
            .filter(|lease| lease.is_live())
            .cloned()
    }

    pub(crate) fn agent_detach(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
        tab_number: u64,
    ) {
        let lease = self.agent_entries.lock().ok().and_then(|mut entries| {
            entries.remove(&(
                project.clone(),
                chat.clone(),
                session.to_owned(),
                tab_number,
            ))
        });
        if let Some(lease) = lease {
            lease.stop();
            self.drop_joined_popups(&lease.tab_label);
        }
    }

    pub(crate) fn agent_retire(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
        tab_number: u64,
    ) {
        if let Ok(entries) = self.agent_entries.lock()
            && let Some(lease) = entries.get(&(
                project.clone(),
                chat.clone(),
                session.to_owned(),
                tab_number,
            ))
        {
            lease.retire();
        }
    }

    pub(crate) fn agent_latest(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
    ) -> Option<AttachmentLease> {
        self.agent_entries
            .lock()
            .ok()?
            .iter()
            // A joined popup is never the implicit default tab.
            .filter(|((p, c, s, _), lease)| {
                p == project
                    && c == chat
                    && s == session
                    && lease.is_live()
                    && lease.opener.is_none()
            })
            .max_by_key(|((_, _, _, number), _)| *number)
            .map(|(_, lease)| lease.clone())
    }

    pub(crate) fn agent_count(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &str,
    ) -> usize {
        self.agent_entries
            .lock()
            .map(|entries| {
                entries
                    .iter()
                    .filter(|((p, c, s, _), lease)| {
                        p == project && c == chat && s == session && lease.opener.is_none()
                    })
                    .count()
            })
            .unwrap_or(MAX_AGENT_TABS_PER_SESSION)
    }

    pub(crate) fn agent_labels_for_chat(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> Vec<String> {
        self.agent_entries
            .lock()
            .map(|entries| {
                entries
                    .iter()
                    .filter(|((p, c, _, _), _)| p == project && c == chat)
                    .map(|(_, lease)| lease.tab_label.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn agent_has_label(&self, label: &str) -> bool {
        self.agent_entries
            .lock()
            .is_ok_and(|entries| entries.values().any(|lease| lease.tab_label == label))
    }

    pub(crate) fn agent_claim_sweep(&self, label: &str) -> Option<AgentSweepClaim> {
        let entries = self.agent_entries.lock().ok()?;
        let lease = entries.values().find(|lease| lease.tab_label == label)?;
        lease
            .sweeping
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(AgentSweepClaim {
            sweeping: Arc::clone(&lease.sweeping),
        })
    }

    pub(crate) fn agent_retire_label(&self, label: &str) {
        if let Ok(entries) = self.agent_entries.lock()
            && let Some(lease) = entries.values().find(|lease| lease.tab_label == label)
        {
            lease.retire();
        }
    }

    pub(crate) fn agent_all_labels(&self) -> Vec<String> {
        self.agent_entries
            .lock()
            .map(|entries| {
                entries
                    .values()
                    .map(|lease| lease.tab_label.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn agent_chats(&self) -> Vec<(SageProjectId, SageChatId)> {
        self.agent_entries
            .lock()
            .map(|entries| {
                let mut chats = std::collections::HashSet::new();
                for (project, chat, _, _) in entries.keys() {
                    chats.insert((project.clone(), chat.clone()));
                }
                chats.into_iter().collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn revoke_agent_chat(&self, project: &SageProjectId, chat: &SageChatId) {
        if let Ok(mut entries) = self.agent_entries.lock() {
            entries.retain(|(p, c, _, _), lease| {
                if p == project && c == chat {
                    lease.stop();
                    false
                } else {
                    true
                }
            });
        }
    }
}
