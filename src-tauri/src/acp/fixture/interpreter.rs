use serde_json::Value;

use crate::acp::limits::LIMITS;

use super::cases::FixtureInvocation;
use super::{FixtureError, process_ops, wire};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StepOwner {
    Process,
    Wire,
    HostOracle,
}

pub(super) fn execute(invocation: FixtureInvocation) -> Result<i32, FixtureError> {
    let steps: Vec<Value> =
        serde_json::from_str(invocation.steps_json).map_err(|_| FixtureError::InvalidArguments)?;
    if steps.len() as u64 > LIMITS.fixture.steps_per_case {
        return Err(FixtureError::InstructionLimit);
    }
    let mut exit_code = 0_i32;
    let mut process_state = process_ops::ProcessState::new();
    let mut wire_state = wire::WireState::new();
    for step in &steps {
        let op = field_str(step, "op")?;
        match classify(op)? {
            StepOwner::Process => process_ops::execute(
                step,
                invocation.case_id,
                invocation.seed,
                &mut exit_code,
                &mut process_state,
            )?,
            StepOwner::Wire => wire::execute(step, invocation.seed, &mut wire_state)?,
            StepOwner::HostOracle => return Err(FixtureError::HostOracleRequired),
        }
    }
    Ok(exit_code)
}

pub(super) fn classify(op: &str) -> Result<StepOwner, FixtureError> {
    match op {
        "delayUntilIntentChanges" | "exit" | "spawnFixtureChildren" | "spawnTree" => {
            Ok(StepOwner::Process)
        }
        "awaitClient"
        | "closeNativeStdin"
        | "closeStdin"
        | "closeStdout"
        | "emitBoundedJson"
        | "emitCapabilityToken"
        | "emitExternalId"
        | "emitKnownFrame"
        | "emitMalformed"
        | "emitMetadata"
        | "emitModels"
        | "emitPermissionPayload"
        | "emitRpcMethod"
        | "emitSizedFrame"
        | "emitStderrLine"
        | "fillStderrRing"
        | "readPromptBytes"
        | "readRequestBytes" => Ok(StepOwner::Wire),
        "advanceDelivery"
        | "commitDecline"
        | "crashRestartEach"
        | "deleteChatLineage"
        | "deleteRuntimeEpoch"
        | "dispatchCancel"
        | "exerciseRedactionSinks"
        | "fillEgressControl"
        | "fillEgressData"
        | "fillJournalControl"
        | "fillJournalData"
        | "fillDriverData"
        | "injectEncodingFailure"
        | "injectQueueFailure"
        | "injectSqliteFailure"
        | "injectStdinFailure"
        | "insertActivity"
        | "insertAgentSession"
        | "insertChatHead"
        | "insertChatLineage"
        | "insertItem"
        | "insertResolvedRequest"
        | "insertRuntimeEpoch"
        | "insertServerRequest"
        | "insertTerminalAgentSession"
        | "insertTerminalTurn"
        | "inspectCsp"
        | "inspectPackage"
        | "launchShape"
        | "mapNativePath"
        | "migrateSession"
        | "normalizeError"
        | "normalizeItem"
        | "normalizeTool"
        | "normalizeUnsupported"
        | "openVerifiedArtifact"
        | "persistActivities"
        | "persistActivity"
        | "persistActivityProvenance"
        | "persistCapabilitySnapshot"
        | "persistCollaboration"
        | "persistItems"
        | "readActivityPage"
        | "readPage"
        | "receivePermissionRequest"
        | "reloadSession"
        | "replaceArtifactPath"
        | "reserveRpc"
        | "reserveRuntimeTasks"
        | "reserveServerRequests"
        | "restart"
        | "retainTerminalTail"
        | "runMigration"
        | "runMigrationFailpoint"
        | "sendPrompt"
        | "spawnFromVerifiedHandle"
        | "terminalizeActivity"
        | "terminalizeSession"
        | "updateActivity"
        | "updateAgentSession"
        | "updateChatHead"
        | "updateChatLineage"
        | "updateServerRequest"
        | "updateTurn" => Ok(StepOwner::HostOracle),
        _ => Err(FixtureError::UnknownStep),
    }
}

pub(super) fn field_str<'a>(step: &'a Value, field: &str) -> Result<&'a str, FixtureError> {
    step.get(field)
        .and_then(Value::as_str)
        .ok_or(FixtureError::InvalidArguments)
}

pub(super) fn field_u64(step: &Value, field: &str) -> Result<u64, FixtureError> {
    step.get(field)
        .and_then(Value::as_u64)
        .ok_or(FixtureError::InvalidArguments)
}
