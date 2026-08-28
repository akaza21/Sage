#[cfg(all(test, windows))]
use std::time::Duration;
use std::time::Instant;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{LaunchError, TreeExit, ZeroProcessToken},
    runtime::{ActivationJournal, ActivationTransport},
};

use super::{ActivationDriverError, ActivationProcessDriver, State};
use crate::acp::runtime::process_driver::{
    DriverMailbox, ProcessTreeControl, StdinTransport,
    activation_parts::{
        ActivationDriverHandoff, ActivationProjectionRecoveryHandoff, ActivationRebindFailure,
        ActivationZeroProcessHandoff, CompletedActivationDriverParts,
        RecoveryActivationDriverParts,
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
        self.state == State::Running
            && mailbox.is_activation_handoff_ready()
            && self.pump.completion_ready()
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
                idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
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
        self.take_zero_process_handoff_after(ProcessTreeControl::wait_zero)
    }

    fn take_zero_process_handoff_after(
        self,
        wait_zero: impl FnOnce(&C) -> Result<(TreeExit, C::ZeroProof), LaunchError>,
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
        let (exit, proof) = match wait_zero(&control) {
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

impl<A, J, R, T> ActivationProcessDriver<A, J, R, T, crate::acp::process::TreeControl>
where
    A: HarnessAdapter,
    J: ActivationJournal,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    pub(crate) fn take_projection_recovery_before(
        self,
        deadline: Instant,
    ) -> ActivationDriverHandoff<ActivationProjectionRecoveryHandoff<A, J, R, ZeroProcessToken>, Self>
    {
        if self.state != State::ProjectionRecovery {
            return Err(Box::new(ActivationRebindFailure {
                driver: self,
                error: ActivationDriverError::NotReusable,
            }));
        }
        let Self {
            pump,
            mut transport,
            control,
            owner,
            clock,
            first_delivery_unknown,
            ..
        } = self;
        transport.close();
        let _ = control.request_termination(TreeExit::ParentShutdown);
        let (_, proof) = match control.wait_zero_before(deadline) {
            Ok(value) => value,
            Err(_) => {
                return Err(Box::new(ActivationRebindFailure {
                    driver: Self {
                        pump,
                        transport,
                        control,
                        owner,
                        clock,
                        state: State::ProjectionRecovery,
                        first_delivery_unknown,
                    },
                    error: ActivationDriverError::Tree,
                }));
            }
        };
        match pump.try_into_recovery_parts() {
            Ok(parts) => Ok(ActivationProjectionRecoveryHandoff {
                parts,
                proof,
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
                        state: State::ProjectionRecovery,
                        first_delivery_unknown,
                    },
                    error: ActivationDriverError::NotReusable,
                }))
            }
        }
    }

    pub(crate) fn take_zero_process_handoff_before(
        self,
        deadline: Instant,
    ) -> ActivationDriverHandoff<ActivationZeroProcessHandoff<A, J, R, ZeroProcessToken>, Self>
    {
        self.take_zero_process_handoff_after(|control| control.wait_zero_before(deadline))
    }

    #[cfg(all(test, windows))]
    pub(crate) fn take_zero_process_handoff_within(
        self,
        timeout: Duration,
    ) -> ActivationDriverHandoff<ActivationZeroProcessHandoff<A, J, R, ZeroProcessToken>, Self>
    {
        self.take_zero_process_handoff_after(|control| control.wait_zero_within(timeout))
    }
}
