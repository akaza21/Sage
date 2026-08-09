use std::ffi::OsString;

use crate::acp::limits::LIMITS;

use super::FixtureError;
use super::generated_cases::{FROZEN_CASES, FrozenOwner, FrozenPhase};

const CHARACTERIZATION_CASES: [(&str, &str); 3] = [
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
];

pub(super) struct FixtureInvocation {
    pub(super) case_id: &'static str,
    pub(super) steps_json: &'static str,
    pub(super) seed: u32,
}

impl FixtureInvocation {
    pub(super) fn parse(
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<Self, FixtureError> {
        let mut values = arguments.into_iter();
        let flag_case = values.next().ok_or(FixtureError::InvalidArguments)?;
        let case = values.next().ok_or(FixtureError::InvalidArguments)?;
        let flag_seed = values.next().ok_or(FixtureError::InvalidArguments)?;
        let seed = values.next().ok_or(FixtureError::InvalidArguments)?;
        if values.next().is_some() || flag_case != "--case" || flag_seed != "--seed" {
            return Err(FixtureError::InvalidArguments);
        }
        let case = case.to_str().ok_or(FixtureError::InvalidArguments)?;
        if let Some((case_id, steps_json)) = CHARACTERIZATION_CASES
            .iter()
            .find(|(case_id, _)| *case_id == case)
        {
            return Self::validated(case_id, steps_json, seed);
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
        })
    }
}
