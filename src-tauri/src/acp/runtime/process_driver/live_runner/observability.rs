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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ZeroProcessDisposition {
    Park,
    Recover,
    Retain,
}

pub(super) fn zero_process_disposition(
    exit: TreeExit,
    observability: ShutdownObservability,
    already_closed: bool,
    durable_completed: bool,
    durable_cancelled: bool,
) -> ZeroProcessDisposition {
    let durable_terminal = already_closed || durable_completed || durable_cancelled;
    if exit == TreeExit::Cancelled && !durable_terminal {
        ZeroProcessDisposition::Retain
    } else if durable_completed && matches!(observability, ShutdownObservability::Intentional) {
        ZeroProcessDisposition::Park
    } else if !already_closed
        && (!durable_terminal || matches!(observability, ShutdownObservability::Failure))
    {
        ZeroProcessDisposition::Recover
    } else {
        ZeroProcessDisposition::Retain
    }
}

pub(super) fn zero_process_terminalized(
    disposition: ZeroProcessDisposition,
    durable_terminal: bool,
    recovered_after_exit: bool,
) -> bool {
    match disposition {
        ZeroProcessDisposition::Recover => recovered_after_exit,
        ZeroProcessDisposition::Park | ZeroProcessDisposition::Retain => durable_terminal,
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

    #[test]
    fn an_intentional_close_after_a_completed_turn_parks_the_runtime_for_reuse() {
        assert_eq!(
            zero_process_disposition(
                TreeExit::Cancelled,
                ShutdownObservability::Intentional,
                false,
                true,
                false,
            ),
            ZeroProcessDisposition::Park
        );
    }

    #[test]
    fn a_cancelled_tree_without_the_runner_cursor_waits_for_the_cancel_service() {
        assert_eq!(
            zero_process_disposition(
                TreeExit::Cancelled,
                ShutdownObservability::Failure,
                false,
                false,
                false,
            ),
            ZeroProcessDisposition::Retain
        );
    }

    #[test]
    fn an_unsolicited_exit_during_a_turn_recovers_the_runtime() {
        assert_eq!(
            zero_process_disposition(
                TreeExit::ProtocolFailure,
                ShutdownObservability::Failure,
                false,
                false,
                false,
            ),
            ZeroProcessDisposition::Recover
        );
    }

    #[test]
    fn an_unsolicited_exit_after_a_completed_turn_recovers_instead_of_parking() {
        assert_eq!(
            zero_process_disposition(
                TreeExit::ProtocolFailure,
                ShutdownObservability::Failure,
                false,
                true,
                false,
            ),
            ZeroProcessDisposition::Recover
        );
    }

    #[test]
    fn an_unsolicited_exit_after_a_cancelled_turn_recovers_the_runtime() {
        assert_eq!(
            zero_process_disposition(
                TreeExit::ProtocolFailure,
                ShutdownObservability::Failure,
                false,
                false,
                true,
            ),
            ZeroProcessDisposition::Recover
        );
    }

    #[test]
    fn an_explicitly_closed_session_leaves_recovery_to_the_close_path() {
        assert_eq!(
            zero_process_disposition(
                TreeExit::ProtocolFailure,
                ShutdownObservability::Failure,
                true,
                false,
                false,
            ),
            ZeroProcessDisposition::Retain
        );
    }

    #[test]
    fn a_failed_recovery_after_an_unsolicited_exit_is_not_a_settled_runner() {
        assert!(!zero_process_terminalized(
            ZeroProcessDisposition::Recover,
            true,
            false
        ));
    }

    #[test]
    fn a_successful_recovery_after_an_unsolicited_exit_settles_the_runner() {
        assert!(zero_process_terminalized(
            ZeroProcessDisposition::Recover,
            true,
            true
        ));
    }

    #[test]
    fn a_failed_recovery_during_a_turn_is_not_a_settled_runner() {
        assert!(!zero_process_terminalized(
            ZeroProcessDisposition::Recover,
            false,
            false
        ));
    }

    #[test]
    fn an_intentional_park_settles_on_its_durable_cursor() {
        assert!(zero_process_terminalized(
            ZeroProcessDisposition::Park,
            true,
            false
        ));
    }

    #[test]
    fn a_retained_close_settles_on_its_durable_cursor() {
        assert!(zero_process_terminalized(
            ZeroProcessDisposition::Retain,
            true,
            false
        ));
    }
}
