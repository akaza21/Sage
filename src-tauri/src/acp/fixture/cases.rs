use std::ffi::OsString;

use crate::acp::limits::LIMITS;

use super::FixtureError;
use super::generated_cases::{FROZEN_CASES, FrozenOwner, FrozenPhase};

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
        if frozen.steps_json.len() as u64 > LIMITS.fixture.instruction_bytes_per_case {
            return Err(FixtureError::InstructionLimit);
        }
        let seed = seed
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or(FixtureError::InvalidArguments)?;
        Ok(Self {
            case_id: frozen.id,
            steps_json: frozen.steps_json,
            seed,
        })
    }
}
