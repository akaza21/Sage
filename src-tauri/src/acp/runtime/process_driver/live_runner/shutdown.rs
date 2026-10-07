use std::time::{Duration, Instant};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{TreeControl, TreeExit},
    runtime::{BoundedThread, ProjectIntentAuthority},
};

use super::{
    DriverMailbox, FirstTurnWorkerContext, FirstTurnWorkerState, RetainedRunnerRecovery,
    RunnerExit, StdinTransport,
    observability::{
        ShutdownObservability, ZeroProcessDisposition, shutdown_observability,
        zero_process_disposition, zero_process_terminalized,
    },
};
use crate::acp::runtime::process_driver::WorkerReply;

pub(super) fn finish<A, R, T, Auth>(
    exit: RunnerExit<A, R, T>,
    mailbox: &DriverMailbox,
    control: &TreeControl,
    reader: &mut BoundedThread,
    watcher: &mut BoundedThread,
    _cleanup_deadline: Instant,
    context: &FirstTurnWorkerContext<Auth>,
) -> (Option<RetainedRunnerRecovery>, bool)
where
    A: HarnessAdapter,
    R: ResourceMapper + Send + 'static,
    T: StdinTransport + Send + 'static,
    A: Send + 'static,
    Auth: ProjectIntentAuthority,
{
    let already_closed = matches!(exit.state, FirstTurnWorkerState::Closed);
    let mut exit = exit;
    let durable_completed = exit.state.has_durable_completed_cursor();
    let durable_cancelled = exit.state.has_durable_cancelled_cursor();
    let durable_terminal = already_closed || durable_completed || durable_cancelled;
    let mut observability = shutdown_observability(exit.reason, durable_terminal, true);
    let mut termination_succeeded = already_closed;
    if !already_closed {
        match observability {
            ShutdownObservability::Intentional => tracing::info!(
                shutdown_reason = ?exit.reason,
                "ACP runner requesting termination after finalized turn"
            ),
            ShutdownObservability::Failure => {
                let stderr_summary = control.stderr_summary().ok();
                let stderr = control.stderr_diagnostic().ok();
                let native_child_exit_code = control.native_exit_code_if_exited().ok().flatten();
                tracing::warn!(
                    recovery_checkpoint = "acp_runner.protocol_failure.pre_termination",
                    protocol_failure_reason = ?exit.reason,
                    native_child_exit_code,
                    jsonrpc_method = ?stderr.as_ref().and_then(|diagnostic| diagnostic.method.as_deref()),
                    jsonrpc_error_code = ?stderr.as_ref().and_then(|diagnostic| diagnostic.error_code.as_deref()),
                    jsonrpc_error_message = ?stderr.as_ref().and_then(|diagnostic| diagnostic.error_message.as_deref()),
                    stderr_summary = ?stderr_summary,
                    "ACP runner diagnostic captured before termination"
                );
            }
        }
        exit.state.close_stdin_after_durable_completed();
        match control.request_termination(exit.reason) {
            Ok(()) => termination_succeeded = true,
            Err(error) => {
                observability = ShutdownObservability::Failure;
                let stderr_summary = control.stderr_summary().ok();
                tracing::warn!(
                    termination_error = ?error,
                    stderr_summary = ?stderr_summary,
                    "ACP runner termination request failed"
                );
            }
        }
    }
    mailbox.close();
    if let Some(envelope) = exit.shutdown.take() {
        envelope.respond(Ok(WorkerReply::Unit));
    }
    let disposition = zero_process_disposition(
        exit.reason,
        observability,
        already_closed,
        durable_completed,
        durable_cancelled,
    );
    let wait_deadline = Instant::now() + cleanup_budget();
    let mut recovered_after_exit = false;
    let converged = match control.wait_zero_before(wait_deadline) {
        Ok((root_exit, token)) => {
            match observability {
                ShutdownObservability::Intentional => tracing::info!(
                    root_exit = ?root_exit,
                    sage_requested_termination = !matches!(root_exit, TreeExit::Exited { .. }),
                    "ACP runner reached process-tree zero after requested clean termination"
                ),
                ShutdownObservability::Failure => {
                    let stderr = control.stderr_summary().ok();
                    tracing::warn!(
                        root_exit = ?root_exit,
                        sage_requested_termination = !matches!(root_exit, TreeExit::Exited { .. }),
                        stderr_summary = ?stderr,
                        "ACP runner reached process-tree zero"
                    );
                }
            }
            match disposition {
                ZeroProcessDisposition::Park => {
                    tauri::async_runtime::block_on(exit.state.park_after_zero(token));
                }
                ZeroProcessDisposition::Recover => {
                    if let Some(sessions) = context.session_recovery.as_ref()
                        && let Ok(proof) = crate::acp::runtime::ZeroProcessProof::from_supervisor(
                            context.owner.clone(),
                            &token,
                        )
                    {
                        recovered_after_exit = sessions
                            .recover_runtime_after_zero_process_retained(
                                context.owner.clone(),
                                proof,
                            )
                            .is_ok();
                    }
                }
                ZeroProcessDisposition::Retain => {}
            }
            true
        }
        Err(error) => {
            let stderr = control.stderr_summary().ok();
            tracing::warn!(
                wait_status = ?error,
                stderr_summary = ?stderr,
                "ACP runner could not prove process-tree zero"
            );
            false
        }
    };
    let remaining = wait_deadline.saturating_duration_since(Instant::now());
    let reader_done = reader.wait(remaining).is_ok();
    let watcher_done = watcher.wait(remaining).is_ok();
    let clean_shutdown =
        zero_process_terminalized(disposition, durable_terminal, recovered_after_exit)
            && termination_succeeded
            && converged
            && reader_done
            && watcher_done;
    (
        (!clean_shutdown).then(|| Box::new(exit.state) as RetainedRunnerRecovery),
        recovered_after_exit,
    )
}

pub(super) fn cleanup_budget() -> Duration {
    Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close)
}
