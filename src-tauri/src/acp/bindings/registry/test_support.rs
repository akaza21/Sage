use std::sync::atomic::{AtomicBool, Ordering};

use crate::acp::bindings::{codex::CodexRuntimeFactory, runtime::RuntimeFactory};

use super::*;

static TEST_FACTORY_CONSTRUCTED: AtomicBool = AtomicBool::new(false);
static TEST_TURN_DISPATCHED: AtomicBool = AtomicBool::new(false);
static TEST_CANCEL_DISPATCHED: AtomicBool = AtomicBool::new(false);
static TEST_CLOSE_DISPATCHED: AtomicBool = AtomicBool::new(false);

pub(crate) const TEST_BINDING_ID: &str = "registry-test-app-server";

pub(crate) fn take_test_factory_construction_for_test() -> bool {
    TEST_FACTORY_CONSTRUCTED.swap(false, Ordering::AcqRel)
}
pub(crate) fn take_test_turn_dispatch_for_test() -> bool {
    TEST_TURN_DISPATCHED.swap(false, Ordering::AcqRel)
}
pub(crate) fn take_test_cancel_dispatch_for_test() -> bool {
    TEST_CANCEL_DISPATCHED.swap(false, Ordering::AcqRel)
}
pub(crate) fn take_test_close_dispatch_for_test() -> bool {
    TEST_CLOSE_DISPATCHED.swap(false, Ordering::AcqRel)
}

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    _mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::Codex(binding), BindingProjectRoot::Codex(root)) = (binding, root) else {
        unreachable!("test binding uses Codex authority")
    };
    Box::new(TestRuntimeFactory(
        CodexRuntimeFactory::from_admitted_binding(binding, root),
    ))
}

pub(super) fn turn_dispatch(
    _: &crate::acp::service::AcpService,
    _: crate::acp::runtime::HostPromptIntent,
) -> Result<
    crate::acp::runtime::HostOutcome,
    crate::acp::prompt_send::PromptSendError<crate::acp::service::CodexSessionError>,
> {
    TEST_TURN_DISPATCHED.store(true, Ordering::Release);
    Err(crate::acp::prompt_send::PromptSendError::unknown(
        crate::acp::service::CodexSessionError::Intent,
    ))
}

pub(super) fn cancel_dispatch(
    _: &crate::acp::service::AcpService,
    _: crate::acp::runtime::HostCancelIntent,
) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
    TEST_CANCEL_DISPATCHED.store(true, Ordering::Release);
    Err(crate::acp::service::CodexSessionError::Intent)
}

pub(super) fn close_dispatch(
    _: &crate::acp::service::AcpService,
    _: crate::acp::store::CloseSessionCommand,
) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
    TEST_CLOSE_DISPATCHED.store(true, Ordering::Release);
    Err(crate::acp::service::CodexSessionError::Intent)
}

struct TestRuntimeFactory(CodexRuntimeFactory);

impl RuntimeFactory for TestRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: sage_acp_protocol::contract::RuntimeEpoch,
        session_id: sage_acp_protocol::contract::SageAgentSessionId,
    ) -> Result<
        crate::acp::bindings::runtime::NativeRuntime,
        crate::acp::bindings::adapter::AdapterRuntimeError,
    > {
        TEST_FACTORY_CONSTRUCTED.store(true, Ordering::Release);
        Box::new(self.0).construct(runtime_epoch, session_id)
    }
}
