//! Where a Pi runtime's process comes from. The runtime carries its launch
//! authority itself, as every other binding's runtime does, so the app holds
//! `PiRuntime` directly and every `HarnessAdapter` method it implements is
//! the one the app calls: no wrapper can leave one out.

use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, runtime::LaunchableRuntime},
    contract::RuntimeEpoch,
    process::LaunchSpec,
};

use super::PiRuntime;

/// One launch of Pi: the installed release, or the test fixture's child.
pub(crate) trait PiLaunch: Send {
    /// The launch for this runtime epoch; a launch is used at most once.
    fn launch_spec(&mut self, epoch: RuntimeEpoch) -> Result<LaunchSpec, AdapterRuntimeError>;
}

impl PiRuntime {
    /// Gives this runtime the authority to start its Pi process.
    pub(crate) fn with_launch(mut self, launch: impl PiLaunch + 'static) -> Self {
        self.launch = Some(Box::new(launch));
        self
    }
}

impl LaunchableRuntime for PiRuntime {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        let epoch = self.runtime_epoch;
        self.launch
            .as_mut()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?
            .launch_spec(epoch)
    }
}
