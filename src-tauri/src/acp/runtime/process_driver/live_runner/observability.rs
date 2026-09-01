use crate::acp::process::TreeExit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShutdownObservability {
    Intentional,
    Failure,
}

pub(super) fn shutdown_observability(
    reason: TreeExit,
    durable_terminal: bool,
    termination_succeeded: bool,
) -> ShutdownObservability {
    if durable_terminal && termination_succeeded && matches!(reason, TreeExit::Cancelled) {
        ShutdownObservability::Intentional
    } else {
        ShutdownObservability::Failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalized_turn_cancelled_by_host_is_reported_as_intentional_shutdown() {
        assert_eq!(
            shutdown_observability(TreeExit::Cancelled, true, true),
            ShutdownObservability::Intentional
        );
    }

    #[test]
    fn protocol_failure_after_a_finalized_turn_remains_failure_observable() {
        assert_eq!(
            shutdown_observability(TreeExit::ProtocolFailure, true, true),
            ShutdownObservability::Failure
        );
    }

    #[test]
    fn failed_termination_request_remains_failure_observable() {
        assert_eq!(
            shutdown_observability(TreeExit::Cancelled, true, false),
            ShutdownObservability::Failure
        );
    }
}
