use super::*;
use crate::acp::{
    goal::{
        GoalAction, GoalNoticeSink, GoalState, NativeRpcIntent, NativeRpcReply, NativeRpcRequest,
    },
    scheduler::RuntimeOwner,
};

impl AcpService {
    #[cfg(test)]
    pub(crate) fn goal_channel_for_test(&self) -> std::sync::Arc<crate::acp::goal::GoalChannel> {
        std::sync::Arc::clone(&self.lane_at(0).runtime.goal)
    }
    #[cfg(test)]
    pub(crate) fn install_goal_test_runtime(
        &self,
        owner: RuntimeOwner,
        host: std::sync::Arc<crate::acp::runtime::RuntimeHost>,
        goal: GoalState,
    ) {
        let lane = self.lane_at(0);
        lane.runtime.reserve_start(owner.clone()).unwrap();
        lane.runtime.goal.update(&owner, goal);
        lane.runtime.finish_start(&owner, host).unwrap();
        lane.runtime.publish_start(&owner).unwrap();
    }
    pub(crate) fn mutate_existing_goal(
        &self,
        owner: RuntimeOwner,
        action: GoalAction,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let lane = self
            .lane_for(&owner)
            .ok_or(crate::acp::runtime::HostError::Intent)?;
        let host = lane
            .runtime
            .for_owner(&owner)
            .map_err(|_| crate::acp::runtime::HostError::Intent)?;
        if !matches!(
            lane.runtime.goal.snapshot(&owner),
            Some(GoalState::Goal { .. })
        ) {
            return Err(crate::acp::runtime::HostError::Intent);
        }
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::Goal(action),
        });
        if matches!(
            result,
            Err(crate::acp::runtime::HostError::Unavailable
                | crate::acp::runtime::HostError::Closed
                | crate::acp::runtime::HostError::Deadline
                | crate::acp::runtime::HostError::DeliveryUnknown
                | crate::acp::runtime::HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        goal_reply(result)
    }
    pub(crate) fn set_goal_notice_sink(&self, sink: GoalNoticeSink) {
        for lane in self.lanes() {
            lane.runtime.goal.set_sink(sink.clone());
        }
    }

    pub(super) fn compose_goal(&self, page: &mut AcpChatPage) {
        page.goal = GoalState::Unsupported;
        let Some(session) = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| Some(&session.id) == page.current_session_id.as_ref())
        else {
            return;
        };
        if !crate::acp::bindings::codex::supports_native_0146_protocol(
            session.capabilities.protocol_version.as_str(),
        ) {
            return;
        }
        let Some(runtime_epoch) = session.runtime_epoch else {
            page.goal = GoalState::Offline;
            return;
        };
        let owner = RuntimeOwner {
            project_id: page.project_id.clone(),
            chat_id: page.chat_id.clone(),
            session_id: session.id.clone(),
            runtime_epoch,
        };
        page.goal = match self.lane_for(&owner) {
            Some(lane) if lane.runtime.for_owner(&owner).is_ok() => lane
                .runtime
                .goal
                .snapshot(&owner)
                .unwrap_or(GoalState::Loading),
            _ => GoalState::Offline,
        };
    }

    pub(super) fn fit_goal_to_page_budget(&self, page: &mut AcpChatPage) -> Result<(), StoreError> {
        fit_goal_to_page_budget(
            page,
            crate::acp::limits::LIMITS.durable.history_page_bytes as usize,
        )
    }
}

fn goal_reply(
    result: Result<NativeRpcReply, crate::acp::runtime::HostError>,
) -> Result<(), crate::acp::runtime::HostError> {
    match result {
        Ok(NativeRpcReply::GoalApplied) => Ok(()),
        Ok(
            NativeRpcReply::ChildTranscript(_)
            | NativeRpcReply::TooLarge
            | NativeRpcReply::Rejected
            | NativeRpcReply::Forked(_)
            | NativeRpcReply::Released
            | NativeRpcReply::UsageRead,
        ) => Err(crate::acp::runtime::HostError::RecoveryRequired),
        Err(error) => Err(error),
    }
}

fn fit_goal_to_page_budget(page: &mut AcpChatPage, limit: usize) -> Result<(), StoreError> {
    let bytes = |page: &AcpChatPage| {
        serde_json::to_vec(page)
            .map(|value| value.len())
            .map_err(|_| StoreError::Corrupt("goal page serialization"))
    };
    if bytes(page)? <= limit {
        return Ok(());
    }
    // Goal is ephemeral decoration; the bounded durable transcript must remain
    // loadable even when its page leaves no room for a large native Goal.
    page.goal = GoalState::Unavailable;
    if bytes(page)? <= limit {
        Ok(())
    } else {
        Err(StoreError::Capacity {
            resource: "historyPage",
            limit: crate::acp::limits::LIMITS.durable.history_page_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        contract::{BoundedVec, ProjectionRevision, SafeText},
        goal::{GoalSnapshot, GoalStatus},
    };

    #[test]
    fn goal_reply_accepts_only_the_goal_applied_variant() {
        assert_eq!(goal_reply(Ok(NativeRpcReply::GoalApplied)), Ok(()));
        assert_eq!(
            goal_reply(Err(crate::acp::runtime::HostError::GoalMutationRejected)),
            Err(crate::acp::runtime::HostError::GoalMutationRejected)
        );
    }

    #[test]
    fn max_goal_degrades_when_history_page_has_no_spare_bytes() {
        let mut page = AcpChatPage {
            session_config: None,
            restored_session_config: None,
            session_config_notice: None,
            goal: GoalState::Goal {
                snapshot: GoalSnapshot {
                    objective: "x".repeat(4_000),
                    status: GoalStatus::Active,
                    token_budget: None,
                    tokens_used: 1,
                    time_used_seconds: 1,
                    created_at: None,
                    updated_at: None,
                },
            },
            usage: false,
            project_id: SageProjectId::parse("project-goal").unwrap(),
            chat_id: SageChatId::parse("chat-goal").unwrap(),
            title: SafeText::new("chatTitle", "History").unwrap(),
            projection_revision: ProjectionRevision::ZERO,
            current_session_id: None,
            sessions: BoundedVec::default(),
            turns: BoundedVec::default(),
            next_cursor: None,
            forked_from: None,
            forked_from_turn_id: None,
            child_projection_epoch: 0,
            session_notices: Vec::new(),
        };
        let existing_turns = page.turns.clone();
        let full_goal = page.goal.clone();
        page.goal = GoalState::Unavailable;
        let near_bound = serde_json::to_vec(&page).unwrap().len() + 1;
        page.goal = full_goal;
        assert!(serde_json::to_vec(&page).unwrap().len() > near_bound);
        fit_goal_to_page_budget(&mut page, near_bound).unwrap();
        assert!(matches!(page.goal, GoalState::Unavailable));
        assert_eq!(page.turns, existing_turns);
        assert!(serde_json::to_vec(&page).unwrap().len() <= near_bound);
    }
}
