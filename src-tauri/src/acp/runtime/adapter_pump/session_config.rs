use super::*;
use crate::acp::session_config::ConfigIntent;

pub(crate) fn write_config<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    intent: &ConfigIntent,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    let egress = adapter.configure(intent).map_err(|_| HostError::Intent)?;
    let outcome = transport.write(egress.frame());
    let resolution = match outcome {
        FrameWriteOutcome::Complete => EgressResolution::Written,
        FrameWriteOutcome::ZeroByteFailure => EgressResolution::WriteZeroBytes,
        FrameWriteOutcome::DeliveryUnknown { .. } => EgressResolution::DeliveryUnknown,
    };
    adapter
        .resolve_egress(egress.into_token(), resolution)
        .map_err(|_| HostError::RecoveryRequired)?;
    match outcome {
        FrameWriteOutcome::Complete => Ok(()),
        FrameWriteOutcome::ZeroByteFailure => Err(HostError::Unavailable),
        FrameWriteOutcome::DeliveryUnknown { .. } => Err(HostError::DeliveryUnknown),
    }
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(crate) fn write_session_config(
        &mut self,
        transport: &mut impl EgressTransport,
        intent: &ConfigIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.ensure_live()
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        write_config(&mut self.adapter, transport, intent)
    }
    pub(crate) fn config_pending(&self) -> bool {
        self.adapter.config_pending()
    }
}
impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> CompletedPumpParts<A, J, R> {
    pub(crate) fn write_session_config(
        &mut self,
        transport: &mut impl EgressTransport,
        intent: &ConfigIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        write_config(&mut self.adapter, transport, intent)
    }
    pub(crate) fn config_pending(&self) -> bool {
        self.adapter.config_pending()
    }
}
