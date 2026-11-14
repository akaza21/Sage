use std::io;

use crate::{
    acp::{
        characterization_observation::CharacterizationObservationEvent,
        process::{SpawnTimingReceipt, monotonic_tick},
    },
    persistence::WalConvergenceReceipt,
};

use super::super::types::CharacterizationCase;
use super::schema::{Latency, MAX_SAFE_INTEGER, MonotonicClock, WalConvergence, checked_duration};

#[derive(Default)]
pub(super) struct ObservationLedger {
    initialize_write_started: Option<u64>,
    ready_committed: Option<u64>,
    prompt_write_completed: Option<u64>,
    first_normalized_item_accepted: Option<u64>,
    cancel_received: Option<u64>,
    cancel_write_completed: Option<u64>,
    cancel_terminal_committed: Option<u64>,
    close_received: Option<u64>,
    job_zero: Option<u64>,
    close_committed: Option<u64>,
    wal_converged: Option<(u64, WalConvergenceReceipt)>,
    invalid: bool,
}

pub(super) struct ValidatedObservation {
    pub clock: MonotonicClock,
    pub wal: WalConvergence,
    pub latency: Latency,
}

impl ObservationLedger {
    pub(super) fn invalidate(&mut self) {
        self.invalid = true;
    }

    pub(super) fn record(&mut self, event: CharacterizationObservationEvent) -> io::Result<()> {
        if self.invalid {
            return invalid("observation state");
        }
        match event {
            CharacterizationObservationEvent::InitializeWriteStarted(tick) => {
                set_once(&mut self.initialize_write_started, tick)
            }
            CharacterizationObservationEvent::ReadyCommitted => set_now(&mut self.ready_committed),
            CharacterizationObservationEvent::PromptWriteCompleted(tick) => {
                set_once(&mut self.prompt_write_completed, tick)
            }
            CharacterizationObservationEvent::FirstNormalizedItemAccepted => {
                set_now(&mut self.first_normalized_item_accepted)
            }
            CharacterizationObservationEvent::CancelReceived(tick) => {
                set_once(&mut self.cancel_received, tick)
            }
            CharacterizationObservationEvent::CancelWriteCompleted(tick) => {
                set_once(&mut self.cancel_write_completed, tick)
            }
            CharacterizationObservationEvent::CancelTerminalCommitted => {
                set_now(&mut self.cancel_terminal_committed)
            }
            CharacterizationObservationEvent::CloseReceived => set_now(&mut self.close_received),
            CharacterizationObservationEvent::JobZero => set_now(&mut self.job_zero),
            CharacterizationObservationEvent::CloseCommitted => set_now(&mut self.close_committed),
            CharacterizationObservationEvent::Invalid => invalid("invalid observation"),
        }
    }

    pub(super) fn record_wal(&mut self, receipt: WalConvergenceReceipt) -> io::Result<()> {
        let tick = current_tick()?;
        if self.invalid || self.wal_converged.is_some() {
            return invalid("duplicate WAL observation");
        }
        self.wal_converged = Some((tick, receipt));
        Ok(())
    }

    pub(super) fn validate(
        &self,
        case: CharacterizationCase,
        timing: SpawnTimingReceipt,
    ) -> io::Result<ValidatedObservation> {
        if self.invalid {
            return invalid("observation state");
        }
        let initialize = required(self.initialize_write_started, "initialize")?;
        let ready = required(self.ready_committed, "ready")?;
        let prompt = required(self.prompt_write_completed, "prompt")?;
        let close = required(self.close_received, "close")?;
        let zero = required(self.job_zero, "job zero")?;
        let close_committed = required(self.close_committed, "close committed")?;
        let (wal_tick, wal_receipt) = self.wal_converged.ok_or_else(|| data("WAL convergence"))?;
        let cancel = (
            self.cancel_received,
            self.cancel_write_completed,
            self.cancel_terminal_committed,
        );
        validate_case(case, self.first_normalized_item_accepted, cancel)?;
        let all_ticks = [
            timing.spawn_started_tick(),
            timing.process_ready_tick(),
            initialize,
            ready,
            prompt,
            self.first_normalized_item_accepted.unwrap_or(prompt),
            cancel.0.unwrap_or(prompt),
            cancel.1.unwrap_or(prompt),
            cancel.2.unwrap_or(prompt),
            close,
            zero,
            close_committed,
            wal_tick,
        ];
        if all_ticks.iter().any(|tick| *tick > MAX_SAFE_INTEGER)
            || timing.process_ready_tick() > initialize
            || initialize >= ready
            || ready > prompt
            || close >= zero
            || zero > close_committed
            || close_committed > wal_tick
            || wal_receipt.checkpoint_target_bytes()
                != sage_core::limits::LIMITS
                    .migration
                    .wal_checkpoint_target_bytes
            || wal_receipt.wal_bytes() >= wal_receipt.checkpoint_target_bytes()
        {
            return invalid("observation order");
        }
        if let Some(first) = self.first_normalized_item_accepted
            && (prompt >= first || first > close)
        {
            return invalid("first output order");
        }
        if let (Some(received), Some(written), Some(terminal)) = cancel
            && (prompt > received
                || prompt > written
                || received >= written
                || written > terminal
                || terminal > close
                || (case == CharacterizationCase::ColdCycle
                    && self
                        .first_normalized_item_accepted
                        .is_none_or(|first| first > received)))
        {
            return invalid("cancel order");
        }
        let frequency = timing.frequency_hz();
        Ok(ValidatedObservation {
            clock: MonotonicClock {
                frequency_hz: frequency,
                spawn_started_tick: timing.spawn_started_tick(),
                process_ready_tick: timing.process_ready_tick(),
                initialize_write_started_tick: initialize,
                ready_committed_tick: ready,
                prompt_write_completed_tick: prompt,
                first_normalized_item_accepted_tick: self.first_normalized_item_accepted,
                cancel_received_tick: cancel.0,
                cancel_write_completed_tick: cancel.1,
                cancel_terminal_committed_tick: cancel.2,
                close_received_tick: close,
                job_zero_tick: zero,
                close_committed_tick: close_committed,
                wal_converged_tick: wal_tick,
            },
            wal: WalConvergence {
                wal_bytes: wal_receipt.wal_bytes(),
                checkpoint_target_bytes: wal_receipt.checkpoint_target_bytes(),
                checkpointed: wal_receipt.checkpointed(),
            },
            latency: Latency {
                spawn_to_process_ready: checked_duration(
                    timing.spawn_started_tick(),
                    timing.process_ready_tick(),
                    frequency,
                )?,
                initialize: checked_duration(initialize, ready, frequency)?,
                first_normalized_output: self
                    .first_normalized_item_accepted
                    .map(|first| checked_duration(prompt, first, frequency))
                    .transpose()?,
                cancel_dispatch: cancel
                    .0
                    .zip(cancel.1)
                    .map(|(received, written)| checked_duration(received, written, frequency))
                    .transpose()?,
                cancel_terminal: cancel
                    .0
                    .zip(cancel.2)
                    .map(|(received, terminal)| checked_duration(received, terminal, frequency))
                    .transpose()?,
                native_process_convergence: checked_duration(close, zero, frequency)?,
            },
        })
    }
}

pub(super) fn validate_safe_clock(timing: SpawnTimingReceipt) -> io::Result<()> {
    (timing.frequency_hz() > 0
        && timing.frequency_hz() <= MAX_SAFE_INTEGER
        && timing.spawn_started_tick() <= MAX_SAFE_INTEGER
        && timing.process_ready_tick() <= MAX_SAFE_INTEGER
        && timing.spawn_started_tick() < timing.process_ready_tick())
    .then_some(())
    .ok_or_else(|| data("timing range"))
}

fn validate_case(
    case: CharacterizationCase,
    first: Option<u64>,
    cancel: (Option<u64>, Option<u64>, Option<u64>),
) -> io::Result<()> {
    match case {
        CharacterizationCase::TurnSuccess if first.is_none() || cancel != (None, None, None) => {
            invalid("turn-success observations")
        }
        CharacterizationCase::TurnCancelled
            if first.is_some()
                || cancel.0.is_none()
                || cancel.1.is_none()
                || cancel.2.is_none() =>
        {
            invalid("turn-cancelled observations")
        }
        CharacterizationCase::ColdCycle
            if first.is_none()
                || cancel.0.is_none()
                || cancel.1.is_none()
                || cancel.2.is_none() =>
        {
            invalid("cold-cycle observations")
        }
        _ => Ok(()),
    }
}

fn set_now(target: &mut Option<u64>) -> io::Result<()> {
    set_once(target, current_tick()?)
}

fn set_once(target: &mut Option<u64>, tick: u64) -> io::Result<()> {
    if tick > MAX_SAFE_INTEGER || target.replace(tick).is_some() {
        return invalid("duplicate observation");
    }
    Ok(())
}

fn current_tick() -> io::Result<u64> {
    monotonic_tick().map_err(|_| data("monotonic clock"))
}

fn required(value: Option<u64>, label: &str) -> io::Result<u64> {
    value.ok_or_else(|| data(label))
}

fn invalid<T>(message: &str) -> io::Result<T> {
    Err(data(message))
}

fn data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_three_case_shapes_validate_against_the_same_native_clock() {
        for case in [
            CharacterizationCase::TurnSuccess,
            CharacterizationCase::TurnCancelled,
            CharacterizationCase::ColdCycle,
        ] {
            let receipt = ledger(case).validate(case, timing()).unwrap();
            assert_eq!(receipt.latency.spawn_to_process_ready, 10);
            assert_eq!(receipt.latency.initialize, 10);
            assert_eq!(
                receipt.latency.first_normalized_output.is_some(),
                case != CharacterizationCase::TurnCancelled
            );
            assert_eq!(
                receipt.latency.cancel_dispatch.is_some(),
                case != CharacterizationCase::TurnSuccess
            );
        }
    }

    #[test]
    fn cancel_cannot_precede_the_completed_prompt_write() {
        let mut ledger = ledger(CharacterizationCase::TurnCancelled);
        ledger.cancel_received = Some(49);
        assert!(
            ledger
                .validate(CharacterizationCase::TurnCancelled, timing())
                .is_err()
        );
    }

    #[test]
    fn case_specific_output_and_cancel_anchors_fail_closed() {
        let mut success = ledger(CharacterizationCase::TurnSuccess);
        success.cancel_received = Some(70);
        assert!(
            success
                .validate(CharacterizationCase::TurnSuccess, timing())
                .is_err()
        );
        let mut cancelled = ledger(CharacterizationCase::TurnCancelled);
        cancelled.first_normalized_item_accepted = Some(60);
        assert!(
            cancelled
                .validate(CharacterizationCase::TurnCancelled, timing())
                .is_err()
        );
    }

    #[test]
    fn duplicate_or_unsafe_observation_poisons_the_ledger() {
        let mut duplicate = ledger(CharacterizationCase::TurnSuccess);
        assert!(
            duplicate
                .record(CharacterizationObservationEvent::PromptWriteCompleted(70))
                .is_err()
        );
        duplicate.invalidate();
        assert!(
            duplicate
                .validate(CharacterizationCase::TurnSuccess, timing())
                .is_err()
        );
        let mut unsafe_tick = ledger(CharacterizationCase::TurnSuccess);
        unsafe_tick.wal_converged = Some((MAX_SAFE_INTEGER + 1, wal()));
        assert!(
            unsafe_tick
                .validate(CharacterizationCase::TurnSuccess, timing())
                .is_err()
        );
    }

    fn ledger(case: CharacterizationCase) -> ObservationLedger {
        let first = (case != CharacterizationCase::TurnCancelled).then_some(60);
        let cancel = (case != CharacterizationCase::TurnSuccess).then_some((70, 80, 90));
        ObservationLedger {
            initialize_write_started: Some(30),
            ready_committed: Some(40),
            prompt_write_completed: Some(50),
            first_normalized_item_accepted: first,
            cancel_received: cancel.map(|value| value.0),
            cancel_write_completed: cancel.map(|value| value.1),
            cancel_terminal_committed: cancel.map(|value| value.2),
            close_received: Some(100),
            job_zero: Some(110),
            close_committed: Some(120),
            wal_converged: Some((130, wal())),
            invalid: false,
        }
    }

    fn timing() -> SpawnTimingReceipt {
        SpawnTimingReceipt::test_only(1_000_000, 10, 20)
    }

    fn wal() -> WalConvergenceReceipt {
        WalConvergenceReceipt::test_only(
            0,
            sage_core::limits::LIMITS
                .migration
                .wal_checkpoint_target_bytes,
            true,
        )
    }
}
