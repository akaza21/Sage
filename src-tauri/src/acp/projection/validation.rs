use super::*;

impl AcpChatPage {
    pub fn validate_page_bounds(&self) -> Result<(), ContractError> {
        let mut items = 0_usize;
        let mut activities = 0_usize;
        if self.current_session_id.as_ref().is_some_and(|current| {
            !self
                .sessions
                .as_slice()
                .iter()
                .any(|session| &session.id == current)
        }) {
            return Err(ContractError::ForbiddenContent {
                field: "currentSessionId",
            });
        }
        if self
            .session_surface
            .as_ref()
            .is_some_and(|surface| self.current_session_id.as_ref() != Some(&surface.session_id))
        {
            return Err(ContractError::ForbiddenContent {
                field: "sessionSurface.sessionId",
            });
        }
        for turn in self.turns.as_slice() {
            if !self
                .sessions
                .as_slice()
                .iter()
                .any(|session| session.id == turn.session_id)
            {
                return Err(ContractError::ForbiddenContent {
                    field: "turn.sessionId",
                });
            }
            for item in turn.items.as_slice() {
                if item.project_id != self.project_id
                    || item.chat_id != self.chat_id
                    || item.session_id != turn.session_id
                    || item.turn_id != turn.id
                    || item.content.requires_tool_state() != item.tool_state.is_some()
                {
                    return Err(ContractError::ForbiddenContent {
                        field: "itemOwnership",
                    });
                }
            }
            for activity in turn.activities.as_slice() {
                if activity.turn_id != turn.id {
                    return Err(ContractError::ForbiddenContent {
                        field: "activityOwnership",
                    });
                }
            }
            items = items.checked_add(turn.items.as_slice().len()).ok_or(
                ContractError::TooManyEntries {
                    field: "historyPageItems",
                    actual: usize::MAX,
                    maximum: 256,
                },
            )?;
            activities = activities
                .checked_add(turn.activities.as_slice().len())
                .ok_or(ContractError::TooManyEntries {
                    field: "activityNodesPerPage",
                    actual: usize::MAX,
                    maximum: 128,
                })?;
        }
        if items > 256 {
            return Err(ContractError::TooManyEntries {
                field: "historyPageItems",
                actual: items,
                maximum: 256,
            });
        }
        if activities > 128 {
            return Err(ContractError::TooManyEntries {
                field: "activityNodesPerPage",
                actual: activities,
                maximum: 128,
            });
        }
        Ok(())
    }
}
