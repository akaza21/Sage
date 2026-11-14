//! A replay case is one scripted session, so it cannot also serve a separate
//! discovery run. Its discovery answers come from the case itself (see
//! `fixture::native_case_discovery_reply`).
use std::{collections::VecDeque, sync::Arc, time::Duration};

use serde_json::Value;

use crate::acp::{
    bindings::codex::{AdmittedCodexBinding, CodexProjectRootAuthority},
    contract::RuntimeEpoch,
    transport::{Frame, FrameWriteOutcome, IncrementalFrameDecoder, OutboundFrame},
};

use super::{
    super::{
        CodexDiscoveryProtocol,
        runner::{
            CodexDiscoveryDeadline, CodexDiscoveryRunError, DiscoveryTransport,
            DiscoveryZeroProcess, RevalidationCheckpoint, run_discovery_protocol,
        },
    },
    AdmittedDiscovery,
};

struct ReplayTransport<'a> {
    case: &'a str,
    binding: &'a AdmittedCodexBinding,
    replies: VecDeque<Frame>,
    runtime_epoch: u64,
}

impl DiscoveryTransport for ReplayTransport<'_> {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        let Ok(request) = serde_json::from_slice::<Value>(frame.payload()) else {
            return FrameWriteOutcome::ZeroByteFailure;
        };
        if request.get("id").is_none() {
            return FrameWriteOutcome::Complete;
        }
        match crate::acp::fixture::native_case_discovery_reply(self.case, &request)
            .and_then(|reply| frame_of(&reply))
        {
            Some(reply) => {
                self.replies.push_back(reply);
                FrameWriteOutcome::Complete
            }
            None => FrameWriteOutcome::ZeroByteFailure,
        }
    }

    fn read_frame(&mut self, _: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        Ok(self.replies.pop_front())
    }

    fn revalidate(&self, _: RevalidationCheckpoint) -> Result<(), CodexDiscoveryRunError> {
        self.binding
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)
    }

    fn close_stdin(&mut self) {}

    fn terminate_zero(
        &mut self,
        _: bool,
        _: CodexDiscoveryDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        Ok(DiscoveryZeroProcess::fixture(self.runtime_epoch))
    }
}

fn frame_of(value: &Value) -> Option<Frame> {
    let mut bytes = serde_json::to_vec(value).ok()?;
    bytes.push(b'\n');
    IncrementalFrameDecoder::new().push_chunk(&bytes).ok()?
}

pub(super) fn discover(
    binding: Arc<AdmittedCodexBinding>,
    project_root: CodexProjectRootAuthority,
    runtime_epoch: RuntimeEpoch,
    deadline: CodexDiscoveryDeadline,
) -> Result<AdmittedDiscovery, CodexDiscoveryRunError> {
    let case = binding
        .desktop_fixture_case()
        .ok_or(CodexDiscoveryRunError::BindingUnavailable)?;
    let mut protocol = CodexDiscoveryProtocol::from_admitted_binding(&binding, runtime_epoch);
    let mut transport = ReplayTransport {
        case,
        binding: &binding,
        replies: VecDeque::new(),
        runtime_epoch: runtime_epoch.get(),
    };
    let content =
        run_discovery_protocol(&mut protocol, &mut transport, runtime_epoch.get(), deadline)?;
    Ok(AdmittedDiscovery {
        content,
        binding,
        project_root: project_root.into_project_root(),
        runtime_epoch,
    })
}
