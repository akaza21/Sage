use super::ServerRequestState;

#[test]
fn pending_permission_states_transition_directly_to_cancelled() {
    for pending in [ServerRequestState::Received, ServerRequestState::Asked] {
        assert!(pending.can_transition_to(ServerRequestState::Cancelled));
    }
    assert!(ServerRequestState::Cancelled.is_terminal());
}

#[test]
fn committed_or_terminal_permission_states_cannot_be_reclassified_as_cancelled() {
    for state in [
        ServerRequestState::AllowPending,
        ServerRequestState::AllowCommitted,
        ServerRequestState::DeclinePending,
        ServerRequestState::DeclineCommitted,
        ServerRequestState::ResponseQueued,
        ServerRequestState::ResolvedAllowed,
        ServerRequestState::ResolvedDeclined,
        ServerRequestState::Cancelled,
    ] {
        assert!(!state.can_transition_to(ServerRequestState::Cancelled));
    }
}
