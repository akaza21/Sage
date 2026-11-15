#![cfg_attr(
    not(windows),
    allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
)]

use crate::acp::{
    browser::permission::{
        BrowserAuthority, BrowserPermissionChoice, BrowserPermissionDecider,
        BrowserPermissionPrompt,
    },
    ipc::ProjectionChangedV1,
    runtime::{JournalLane, TurnNotice},
    store::{RequestStateCommand, StoreMutation},
};
use sage_acp_protocol::{
    contract::{
        RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
        SageTurnId,
    },
    lifecycle::ServerRequestState,
};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct BrowserPermissionOwner {
    pub(crate) project_id: SageProjectId,
    pub(crate) chat_id: SageChatId,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) turn_id: SageTurnId,
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) binding_digest: String,
}

pub(super) struct PendingBrowserPermission {
    pub(super) owner: BrowserPermissionOwner,
    pub(super) sender: std::sync::mpsc::SyncSender<BrowserPermissionChoice>,
    pub(super) live: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
    /// A developer-mode card, which only `AllowDeveloperMode` or `Deny`
    /// answers.
    pub(super) elevated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserPermissionDecisionResponse {
    source: &'static str,
    project_id: SageProjectId,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    turn_id: SageTurnId,
    runtime_epoch: RuntimeEpoch,
    request_id: SageServerRequestId,
    decision: &'static str,
    state: &'static str,
}

/// The browser gate still performs the final authority recheck before
/// invoking a capability.
pub(crate) struct ServiceBrowserPermissionDecider<'a> {
    pub(crate) service: &'a super::AcpService,
    pub(crate) owner: BrowserPermissionOwner,
    pub(crate) authority: BrowserAuthority,
    pub(crate) attachment: Option<crate::acp::browser::attachment::AttachmentLease>,
}

impl BrowserPermissionDecider for ServiceBrowserPermissionDecider<'_> {
    fn decide(&mut self, prompt: &BrowserPermissionPrompt) -> BrowserPermissionChoice {
        let Some(live) = self.authority.liveness_for(prompt) else {
            return BrowserPermissionChoice::Deny;
        };
        let attachment = self.attachment.clone();
        self.service
            .request_browser_permission(self.owner.clone(), prompt, move || {
                live() && attachment.as_ref().is_none_or(|lease| lease.is_live())
            })
    }
}

impl super::AcpService {
    pub(crate) fn browser_owner_witness_matches(&self, owner: &BrowserPermissionOwner) -> bool {
        self.lanes().any(|lane| {
            lane.scheduler.active().is_some_and(|active| {
                active.tree.project_id == owner.project_id
                    && active.tree.chat_id == owner.chat_id
                    && active.tree.session_id == owner.session_id
                    && active.tree.runtime_epoch == owner.runtime_epoch
                    && active.turn_id.as_ref() == Some(&owner.turn_id)
                    && lane.runtime.for_owner(&active.tree).is_ok()
            })
        })
    }

    pub(crate) fn browser_owner_precheck_matches(&self, owner: &BrowserPermissionOwner) -> bool {
        self.current_browser_owner(&owner.project_id, &owner.chat_id, &owner.session_id)
            .is_some_and(|current| {
                current.turn_id == owner.turn_id
                    && current.runtime_epoch == owner.runtime_epoch
                    && current.binding_digest == owner.binding_digest
            })
    }

    pub(crate) fn current_browser_owner(
        &self,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
        session_id: &SageAgentSessionId,
    ) -> Option<BrowserPermissionOwner> {
        use rusqlite::OptionalExtension;
        let active = self
            .lanes()
            .filter_map(|lane| lane.scheduler.active())
            .find(|active| {
                &active.tree.project_id == project_id
                    && &active.tree.chat_id == chat_id
                    && &active.tree.session_id == session_id
            });
        let turn_id = active.as_ref().and_then(|active| active.turn_id.as_ref());
        let (turn_id, epoch, digest): (String, i64, String) = self.database.connection().query_row(
            "SELECT t.id,t.runtime_epoch,s.binding_digest FROM turns t
             JOIN agent_sessions s ON s.id=t.session_id
             JOIN session_runtime_epochs e ON e.session_id=t.session_id AND e.runtime_epoch=t.runtime_epoch
             WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3
               AND s.state IN ('ready','active') AND e.state='running'
               AND (?4 IS NULL OR t.id=?4)
               AND t.state NOT IN ('queued','cancelling')
               AND t.state NOT IN ('completed','interrupted','cancelled','failed','deliveryUnknown')
             ORDER BY t.ordinal DESC LIMIT 1",
            rusqlite::params![project_id.as_str(), chat_id.as_str(), session_id.as_str(), turn_id.map(|id| id.as_str())],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().ok()??;
        Some(BrowserPermissionOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            turn_id: SageTurnId::parse(turn_id).ok()?,
            runtime_epoch: RuntimeEpoch::new(u64::try_from(epoch).ok()?).ok()?,
            binding_digest: digest,
        })
    }

    pub(crate) fn permission_request_kind(
        &self,
        owner: &BrowserPermissionOwner,
        request_id: &SageServerRequestId,
    ) -> Result<String, crate::acp::runtime::HostError> {
        use rusqlite::OptionalExtension;
        self.database.connection().query_row(
            "SELECT kind FROM server_requests WHERE project_id=?1 AND chat_id=?2 AND session_id=?3
             AND turn_id=?4 AND runtime_epoch=?5 AND id=?6",
            rusqlite::params![owner.project_id.as_str(), owner.chat_id.as_str(), owner.session_id.as_str(),
                owner.turn_id.as_str(), owner.runtime_epoch.get() as i64, request_id.as_str()],
            |row| row.get::<_, String>(0),
        ).optional().map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?
            .ok_or(crate::acp::runtime::HostError::Intent)
    }

    pub(crate) fn resolve_browser_permission_decision(
        &self,
        owner: BrowserPermissionOwner,
        request_id: SageServerRequestId,
        choice: BrowserPermissionChoice,
    ) -> Result<BrowserPermissionDecisionResponse, crate::acp::runtime::HostError> {
        use crate::acp::runtime::HostError;
        if !is_browser_request_kind(&self.permission_request_kind(&owner, &request_id)?) {
            return Err(HostError::Intent);
        }
        let pending = {
            let mut requests = self
                .browser_permissions
                .lock()
                .map_err(|_| HostError::RecoveryRequired)?;
            let Some(pending) = requests.get(&request_id) else {
                return Err(HostError::Intent);
            };
            if pending.owner.project_id != owner.project_id
                || pending.owner.chat_id != owner.chat_id
                || pending.owner.session_id != owner.session_id
                || pending.owner.turn_id != owner.turn_id
                || pending.owner.runtime_epoch != owner.runtime_epoch
                || !choice_fits(pending.elevated, choice)
            {
                return Err(HostError::Intent);
            }
            requests.remove(&request_id).ok_or(HostError::Intent)?
        };
        if !(pending.live)() || !self.browser_turn_live(&pending.owner) {
            self.cancel_browser_permission(&pending.owner, &request_id);
            return Err(HostError::Intent);
        }
        self.transition_browser_permission(&pending.owner, &request_id, choice)?;
        let _ = pending.sender.send(choice);
        Ok(BrowserPermissionDecisionResponse {
            source: "browser",
            project_id: owner.project_id,
            chat_id: owner.chat_id,
            session_id: owner.session_id,
            turn_id: owner.turn_id,
            runtime_epoch: owner.runtime_epoch,
            request_id,
            decision: match choice {
                BrowserPermissionChoice::AllowOnce => "allowOnce",
                BrowserPermissionChoice::AllowForOriginSession => "allowForOriginSession",
                BrowserPermissionChoice::AllowDeveloperMode => "allowDeveloperMode",
                BrowserPermissionChoice::Deny => "deny",
            },
            state: match choice {
                BrowserPermissionChoice::Deny => "resolvedDeclined",
                _ => "resolvedAllowed",
            },
        })
    }

    pub(crate) fn request_browser_permission(
        &self,
        owner: BrowserPermissionOwner,
        prompt: &BrowserPermissionPrompt,
        still_live: impl Fn() -> bool + Send + Sync + 'static,
    ) -> BrowserPermissionChoice {
        if prompt.scope.session_id != owner.session_id.as_str()
            || (prompt.scope.tab_id == 0 && prompt.operation != "openTab")
            || !matches!(
                prompt.operation,
                "openTab"
                    | "navigate"
                    | "snapshotPage"
                    | "act"
                    | "waitFor"
                    | "screenshot"
                    | "readConsole"
                    | "readNetwork"
                    | "cdp"
            )
            || !still_live()
            || !self.browser_turn_live(&owner)
        {
            return BrowserPermissionChoice::Deny;
        }
        let request_id = SageServerRequestId::new();
        let Some(intent) = local_intent(&owner, prompt, &request_id) else {
            return BrowserPermissionChoice::Deny;
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let live: std::sync::Arc<dyn Fn() -> bool + Send + Sync> = std::sync::Arc::new(still_live);
        {
            let Ok(mut pending) = self.browser_permissions.lock() else {
                return BrowserPermissionChoice::Deny;
            };
            if pending.len() >= 16 {
                return BrowserPermissionChoice::Deny;
            }
            pending.insert(
                request_id.clone(),
                PendingBrowserPermission {
                    owner: owner.clone(),
                    sender,
                    live: std::sync::Arc::clone(&live),
                    elevated: prompt.operation == "cdp",
                },
            );
        }
        let now = sage_core::time::now_millis();
        let committed = self
            .journal
            .enqueue_one(
                JournalLane::Control,
                StoreMutation::ReceivePermission {
                    intent,
                    now_millis: now,
                },
                u64::try_from(now)
                    .unwrap_or_default()
                    .saturating_add(sage_core::limits::LIMITS.deadlines_ms.ordinary_rpc),
            )
            .and_then(|ticket| ticket.blocking_committed());
        let Ok(commit) = committed else {
            self.browser_permissions
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&request_id));
            return BrowserPermissionChoice::Deny;
        };
        // The request admitted an approval notice. Projection delivery is
        // advisory: the prompt stays pending whatever routing decides.
        let _ = self.publisher.publish_committed_with(
            ProjectionChangedV1 {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                projection_revision: commit.projection_revision,
                child_projection_epoch: 0,
            },
            TurnNotice::Possible,
        );
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            if !(live)() || !self.browser_turn_live(&owner) || Instant::now() >= deadline {
                self.cancel_browser_permission(&owner, &request_id);
                return BrowserPermissionChoice::Deny;
            }
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(choice) => {
                    return if (live)() && self.browser_turn_live(&owner) {
                        choice
                    } else {
                        BrowserPermissionChoice::Deny
                    };
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return BrowserPermissionChoice::Deny;
                }
            }
        }
    }

    fn browser_turn_live(&self, owner: &BrowserPermissionOwner) -> bool {
        self.database.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM turns t JOIN agent_sessions s ON s.id=t.session_id
             JOIN session_runtime_epochs e ON e.session_id=t.session_id AND e.runtime_epoch=t.runtime_epoch
             WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3 AND t.id=?4 AND t.runtime_epoch=?5
               AND s.binding_digest=?6 AND s.state IN ('ready','active') AND e.state='running'
               AND t.state NOT IN ('completed','interrupted','cancelled','failed','deliveryUnknown'))",
            rusqlite::params![owner.project_id.as_str(),owner.chat_id.as_str(),owner.session_id.as_str(),owner.turn_id.as_str(),owner.runtime_epoch.get() as i64,&owner.binding_digest],
            |row| row.get::<_, bool>(0),
        ).unwrap_or(false)
    }

    fn cancel_browser_permission(
        &self,
        owner: &BrowserPermissionOwner,
        request_id: &SageServerRequestId,
    ) {
        self.browser_permissions
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(request_id));
        let _ =
            self.transition_browser_permission(owner, request_id, BrowserPermissionChoice::Deny);
    }

    pub(super) fn transition_browser_permission(
        &self,
        owner: &BrowserPermissionOwner,
        request_id: &SageServerRequestId,
        choice: BrowserPermissionChoice,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let states: &[ServerRequestState] = match choice {
            BrowserPermissionChoice::Deny => &[
                ServerRequestState::DeclinePending,
                ServerRequestState::DeclineCommitted,
                ServerRequestState::ResponseQueued,
                ServerRequestState::ResolvedDeclined,
            ],
            BrowserPermissionChoice::AllowOnce
            | BrowserPermissionChoice::AllowForOriginSession
            | BrowserPermissionChoice::AllowDeveloperMode => &[
                ServerRequestState::AllowPending,
                ServerRequestState::AllowCommitted,
                ServerRequestState::ResponseQueued,
                ServerRequestState::ResolvedAllowed,
            ],
        };
        for state in states {
            let now = sage_core::time::now_millis();
            self.journal
                .enqueue_one(
                    JournalLane::Control,
                    StoreMutation::RequestState(RequestStateCommand {
                        project_id: owner.project_id.clone(),
                        chat_id: owner.chat_id.clone(),
                        session_id: owner.session_id.clone(),
                        turn_id: owner.turn_id.clone(),
                        runtime_epoch: owner.runtime_epoch,
                        request_id: request_id.clone(),
                        state: *state,
                        now_millis: now,
                    }),
                    u64::try_from(now)
                        .unwrap_or_default()
                        .saturating_add(sage_core::limits::LIMITS.deadlines_ms.ordinary_rpc),
                )
                .and_then(|ticket| ticket.blocking_committed())
                .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        }
        Ok(())
    }
}

#[path = "browser_permission/intent.rs"]
mod intent;
pub(crate) use intent::is_browser_request_kind;
use intent::{choice_fits, local_intent};

#[cfg(test)]
mod tests;
