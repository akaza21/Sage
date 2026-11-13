use std::ffi::OsString;

use crate::acp::limits::LIMITS;

use super::FixtureError;
use super::generated_cases::{FROZEN_CASES, FrozenOwner, FrozenPhase, JOURNEY_CASES};

const CHARACTERIZATION_CASES: [(&str, &str); 10] = [
    (
        "characterization-browser-bootstrap",
        r#"[{"op":"awaitClient","kind":"codexInitialize"},{"op":"emitKnownFrame","shape":"codexInitializeResult"},{"op":"awaitClient","kind":"codexStart"},{"op":"emitKnownFrame","shape":"codexThreadResult"},{"op":"awaitClient","kind":"codexModelList"},{"op":"emitKnownFrame","shape":"codexModelListResult"},{"op":"awaitClient","kind":"codexGoalGet"},{"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"emitKnownFrame","shape":"codexDelta"},{"op":"emitKnownFrame","shape":"codexCompleted"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"awaitClient","kind":"codexInterrupt"},{"op":"emitKnownFrame","shape":"codexAck"},{"op":"emitKnownFrame","shape":"codexInterrupted"},{"op":"awaitClient","kind":"codexUnsubscribe"},{"op":"emitKnownFrame","shape":"codexAck"}]"#,
    ),
    (
        "characterization-turn-success",
        r#"[{"op":"awaitClient","kind":"initialize"},{"op":"emitKnownFrame","shape":"initializeSuccess","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"newSession"},{"op":"emitKnownFrame","shape":"newSessionSuccess","externalSessionId":"fixture-persisted-session-1","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"prompt"},{"op":"emitKnownFrame","shape":"textStart","delimiter":"lf","split":"none"},{"op":"emitKnownFrame","shape":"textDelta","delimiter":"lf","split":"none"},{"op":"emitKnownFrame","shape":"textComplete","delimiter":"lf","split":"none"},{"op":"emitKnownFrame","shape":"turnComplete","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"close"}]"#,
    ),
    (
        "characterization-turn-cancelled",
        r#"[{"op":"awaitClient","kind":"initialize"},{"op":"emitKnownFrame","shape":"initializeSuccess","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"newSession"},{"op":"emitKnownFrame","shape":"newSessionSuccess","externalSessionId":"fixture-persisted-session-1","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"prompt"},{"op":"awaitClient","kind":"cancel"},{"op":"emitKnownFrame","shape":"turnCancelled","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"close"}]"#,
    ),
    (
        "characterization-cold-cycle",
        r#"[{"op":"awaitClient","kind":"initialize"},{"op":"emitKnownFrame","shape":"initializeSuccess","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"newSession"},{"op":"emitKnownFrame","shape":"newSessionSuccess","externalSessionId":"fixture-persisted-session-1","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"prompt"},{"op":"emitKnownFrame","shape":"textStart","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"cancel"},{"op":"emitKnownFrame","shape":"turnCancelled","delimiter":"lf","split":"none"},{"op":"awaitClient","kind":"close"}]"#,
    ),
    (
        "codex-journey-turn-success",
        r#"[{"op":"awaitClient","kind":"codexInitialize"},{"op":"emitKnownFrame","shape":"codexInitializeResult"},{"op":"awaitClient","kind":"codexStart"},{"op":"emitKnownFrame","shape":"codexThreadResult"},{"op":"awaitClient","kind":"codexModelList"},{"op":"emitKnownFrame","shape":"codexModelListResult"},{"op":"awaitClient","kind":"codexGoalGet"},{"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"emitKnownFrame","shape":"codexDelta"},{"op":"emitKnownFrame","shape":"codexCompleted"},{"op":"awaitClient","kind":"codexUnsubscribe"},{"op":"emitKnownFrame","shape":"codexAck"}]"#,
    ),
    (
        "characterization-two-turn-success",
        r#"[{"op":"awaitClient","kind":"codexInitialize"},{"op":"emitKnownFrame","shape":"codexInitializeResult"},{"op":"awaitClient","kind":"codexStart"},{"op":"emitKnownFrame","shape":"codexThreadResult"},{"op":"awaitClient","kind":"codexModelList"},{"op":"emitKnownFrame","shape":"codexModelListResult"},{"op":"awaitClient","kind":"codexGoalGet"},{"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"emitKnownFrame","shape":"codexDelta"},{"op":"emitKnownFrame","shape":"codexCompleted"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"emitKnownFrame","shape":"codexDelta"},{"op":"emitKnownFrame","shape":"codexCompleted"},{"op":"awaitClient","kind":"codexUnsubscribe"},{"op":"emitKnownFrame","shape":"codexAck"}]"#,
    ),
    (
        "characterization-cancel-reuse",
        r#"[{"op":"awaitClient","kind":"codexInitialize"},{"op":"emitKnownFrame","shape":"codexInitializeResult"},{"op":"awaitClient","kind":"codexStart"},{"op":"emitKnownFrame","shape":"codexThreadResult"},{"op":"awaitClient","kind":"codexModelList"},{"op":"emitKnownFrame","shape":"codexModelListResult"},{"op":"awaitClient","kind":"codexGoalGet"},{"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"awaitClient","kind":"codexInterrupt"},{"op":"emitKnownFrame","shape":"codexAck"},{"op":"emitKnownFrame","shape":"codexInterrupted"},{"op":"awaitClient","kind":"codexUnsubscribe"},{"op":"emitKnownFrame","shape":"codexAck"}]"#,
    ),
    (
        "codex-item-completed-then-exit",
        r#"[{"op":"awaitClient","kind":"codexInitialize"},{"op":"emitKnownFrame","shape":"codexInitializeResult"},{"op":"awaitClient","kind":"codexStart"},{"op":"emitKnownFrame","shape":"codexThreadResult"},{"op":"awaitClient","kind":"codexModelList"},{"op":"emitKnownFrame","shape":"codexModelListResult"},{"op":"awaitClient","kind":"codexGoalGet"},{"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"emitKnownFrame","shape":"codexItemCompleted"},{"op":"exit","code":17}]"#,
    ),
    (
        "codex-partial-turn-at-shutdown",
        r#"[{"op":"awaitClient","kind":"codexInitialize"},{"op":"emitKnownFrame","shape":"codexInitializeResult"},{"op":"awaitClient","kind":"codexStart"},{"op":"emitKnownFrame","shape":"codexThreadResult"},{"op":"awaitClient","kind":"codexModelList"},{"op":"emitKnownFrame","shape":"codexModelListResult"},{"op":"awaitClient","kind":"codexGoalGet"},{"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"emitKnownFrame","shape":"codexDelta"},{"op":"awaitClient","kind":"codexSteer"}]"#,
    ),
    (
        "characterization-resume-turn-success",
        r#"[{"op":"awaitClient","kind":"codexInitialize"},{"op":"emitKnownFrame","shape":"codexInitializeResult"},{"op":"awaitClient","kind":"codexResume"},{"op":"emitKnownFrame","shape":"codexThreadResult"},{"op":"awaitClient","kind":"codexModelList"},{"op":"emitKnownFrame","shape":"codexModelListResult"},{"op":"awaitClient","kind":"codexGoalGet"},{"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"},{"op":"awaitClient","kind":"codexTurnStart"},{"op":"emitKnownFrame","shape":"codexTurnResult"},{"op":"emitKnownFrame","shape":"codexStarted"},{"op":"emitKnownFrame","shape":"codexDelta"},{"op":"emitKnownFrame","shape":"codexCompleted"},{"op":"awaitClient","kind":"codexUnsubscribe"},{"op":"emitKnownFrame","shape":"codexAck"}]"#,
    ),
];

pub(super) struct FixtureInvocation {
    pub(super) case_id: &'static str,
    pub(super) steps_json: &'static str,
    pub(super) seed: u32,
    /// What follows the case and seed: the arguments the real harness would
    /// take, which only a Claude Code SDK case reads (its session flags).
    pub(super) harness_args: Vec<String>,
}

/// Harness arguments one launch may carry, and the bytes of each.
const MAX_HARNESS_ARGS: usize = 16;
const MAX_HARNESS_ARG_BYTES: usize = 256;

impl FixtureInvocation {
    pub(super) fn parse(
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<Self, FixtureError> {
        let mut values = arguments.into_iter();
        let flag_case = values.next().ok_or(FixtureError::InvalidArguments)?;
        let case = values.next().ok_or(FixtureError::InvalidArguments)?;
        let flag_seed = values.next().ok_or(FixtureError::InvalidArguments)?;
        let seed = values.next().ok_or(FixtureError::InvalidArguments)?;
        if flag_case != "--case" || flag_seed != "--seed" {
            return Err(FixtureError::InvalidArguments);
        }
        let case = case.to_str().ok_or(FixtureError::InvalidArguments)?;
        let harness_args = values
            .map(|value| {
                value
                    .into_string()
                    .ok()
                    .filter(|value| value.len() <= MAX_HARNESS_ARG_BYTES)
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(FixtureError::InvalidArguments)?;
        if harness_args.len() > MAX_HARNESS_ARGS
            || (!harness_args.is_empty() && !case.starts_with("claude-sdk-"))
        {
            return Err(FixtureError::InvalidArguments);
        }
        let mut invocation = Self::case(case, seed)?;
        invocation.harness_args = harness_args;
        Ok(invocation)
    }

    fn case(case: &str, seed: OsString) -> Result<Self, FixtureError> {
        if let Some((case_id, steps_json)) = CHARACTERIZATION_CASES
            .iter()
            .find(|(case_id, _)| *case_id == case)
        {
            return Self::validated(case_id, steps_json, seed);
        }
        if let Some(journey) = JOURNEY_CASES.iter().find(|journey| journey.id == case) {
            return Self::validated(journey.id, journey.steps_json, seed);
        }
        let frozen = FROZEN_CASES
            .iter()
            .find(|fixture_case| fixture_case.id == case)
            .ok_or(FixtureError::InvalidArguments)?;
        if frozen.owner == FrozenOwner::Host {
            return Err(FixtureError::HostOracleRequired);
        }
        if frozen.phase == FrozenPhase::Deferred {
            return Err(FixtureError::DeferredCase);
        }
        Self::validated(frozen.id, frozen.steps_json, seed)
    }

    fn validated(
        case_id: &'static str,
        steps_json: &'static str,
        seed: OsString,
    ) -> Result<Self, FixtureError> {
        if steps_json.len() as u64 > LIMITS.fixture.instruction_bytes_per_case {
            return Err(FixtureError::InstructionLimit);
        }
        let seed = seed
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or(FixtureError::InvalidArguments)?;
        Ok(Self {
            case_id,
            steps_json,
            seed,
            harness_args: Vec::new(),
        })
    }
}
