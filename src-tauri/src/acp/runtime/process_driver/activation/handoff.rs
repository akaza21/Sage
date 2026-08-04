use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{ActivationJournal, ActivationTransport},
};

use super::{ActivationDriverError, ActivationProcessDriver, State};
use crate::acp::runtime::process_driver::{
    DriverMailbox, ProcessTreeControl, StdinTransport,
    activation_parts::{
        ActivationDriverHandoff, ActivationRebindFailure, ActivationZeroProcessHandoff,
        CompletedActivationDriverParts, RecoveryActivationDriverParts,
    },
};

impl<A, J, R, T, C> ActivationProcessDriver<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: ActivationJournal,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
    C: ProcessTreeControl,
{
    pub(crate) fn completion_ready(&self, mailbox: &DriverMailbox) -> bool {
        self.state == State::Running && mailbox.is_quiescent() && self.pump.completion_ready()
    }

    pub(crate) fn try_into_idle_parts(
        self,
        mailbox: &DriverMailbox,
    ) -> ActivationDriverHandoff<CompletedActivationDriverParts<A, J, R, T, C>, Self> {
        if !self.completion_ready(mailbox) {
            return Err(Box::new(ActivationRebindFailure {
                driver: self,
                error: ActivationDriverError::NotReusable,
            }));
        }
        let Self {
            pump,
            transport,
            control,
            owner,
            clock,
            ..
        } = self;
        match pump.try_into_idle_parts() {
            Ok(parts) => Ok(CompletedActivationDriverParts {
                parts,
                transport,
                control,
                owner,
                clock,
            }),
            Err(error) => {
                let (pump, _) = *error;
                Err(Box::new(ActivationRebindFailure {
                    driver: Self {
                        pump,
                        transport,
                        control,
                        owner,
                        clock,
                        state: State::Running,
                        first_delivery_unknown: None,
                    },
                    error: ActivationDriverError::NotReusable,
                }))
            }
        }
    }

    pub(crate) fn try_into_recovery_parts(
        self,
    ) -> ActivationDriverHandoff<RecoveryActivationDriverParts<A, J, R, T, C>, Self> {
        if self.state != State::ProjectionRecovery {
            return Err(Box::new(ActivationRebindFailure {
                driver: self,
                error: ActivationDriverError::NotReusable,
            }));
        }
        let Self {
            pump,
            transport,
            control,
            owner,
            clock,
            first_delivery_unknown,
            ..
        } = self;
        match pump.try_into_recovery_parts() {
            Ok(parts) => Ok(RecoveryActivationDriverParts {
                parts,
                transport,
                control,
                owner,
                clock,
                first_delivery_unknown,
            }),
            Err(error) => {
                let (pump, _) = *error;
                Err(Box::new(ActivationRebindFailure {
                    driver: Self {
                        pump,
                        transport,
                        control,
                        owner,
                        clock,
                        state: State::ProjectionRecovery,
                        first_delivery_unknown,
                    },
                    error: ActivationDriverError::NotReusable,
                }))
            }
        }
    }

    pub(crate) fn take_zero_process_handoff(
        self,
    ) -> ActivationDriverHandoff<ActivationZeroProcessHandoff<A, J, R, C::ZeroProof>, Self> {
        let cause = match self.state {
            State::Teardown(cause) => cause,
            _ => {
                return Err(Box::new(ActivationRebindFailure {
                    driver: self,
                    error: ActivationDriverError::Tree,
                }));
            }
        };
        let Self {
            pump,
            control,
            owner,
            first_delivery_unknown,
            transport,
            clock,
            ..
        } = self;
        let (exit, proof) = match control.wait_zero() {
            Ok(value) => value,
            Err(_) => {
                return Err(Box::new(ActivationRebindFailure {
                    driver: Self {
                        pump,
                        transport,
                        control,
                        owner,
                        clock,
                        state: State::Teardown(cause),
                        first_delivery_unknown,
                    },
                    error: ActivationDriverError::Tree,
                }));
            }
        };
        match pump.try_into_teardown_parts() {
            Ok(parts) => Ok(ActivationZeroProcessHandoff {
                parts,
                proof,
                exit,
                cause,
                owner,
                first_delivery_unknown,
            }),
            Err(error) => {
                let (pump, _) = *error;
                Err(Box::new(ActivationRebindFailure {
                    driver: Self {
                        pump,
                        transport,
                        control,
                        owner,
                        clock,
                        state: State::Teardown(cause),
                        first_delivery_unknown,
                    },
                    error: ActivationDriverError::NotReusable,
                }))
            }
        }
    }
}
