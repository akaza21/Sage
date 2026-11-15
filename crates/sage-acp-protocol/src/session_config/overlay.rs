use super::{ConfigId, ConfigValue, SessionConfig};
use crate::{
    contract::{ProjectionRevision, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ConfigRevision(ProjectionRevision);
impl ConfigRevision {
    #[cfg(any(test, feature = "test-support"))]
    pub fn get(self) -> u64 {
        self.0.get()
    }
    pub fn first() -> Self {
        Self(ProjectionRevision::new(1).expect("one"))
    }
    pub fn next(self) -> Result<Self, String> {
        self.0.next().map(Self).map_err(|e| e.to_string())
    }
}
impl<'de> Deserialize<'de> for ConfigRevision {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = ProjectionRevision::deserialize(d)?;
        if value == ProjectionRevision::ZERO {
            return Err(serde::de::Error::custom("config revision must be positive"));
        }
        Ok(Self(value))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigOverlay {
    pub sage_agent_session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub config_revision: ConfigRevision,
    #[serde(skip_serializing_if = "is_false")]
    pub removed: bool,
    #[serde(flatten)]
    pub config: SessionConfig,
}
fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigChangedV1 {
    pub schema_version: u8,
    pub sage_project_id: SageProjectId,
    pub sage_chat_id: SageChatId,
    pub sage_agent_session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub config_revision: ConfigRevision,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigMutation {
    Mode(ConfigId),
    Option {
        config_id: ConfigId,
        value: ConfigValue,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIntent {
    pub owner: RuntimeOwner,
    pub expected_config_revision: ConfigRevision,
    pub mutation: ConfigMutation,
}
impl ConfigIntent {
    pub fn actual_bytes(&self) -> usize {
        self.owner.project_id.as_str().len()
            + self.owner.chat_id.as_str().len()
            + self.owner.session_id.as_str().len()
            + 16
            + match &self.mutation {
                ConfigMutation::Mode(id) => id.as_str().len(),
                ConfigMutation::Option { config_id, value } => {
                    config_id.as_str().len()
                        + match value {
                            ConfigValue::Select { value } => value.as_str().len(),
                            ConfigValue::Boolean { .. } => 1,
                        }
                }
            }
    }
}
