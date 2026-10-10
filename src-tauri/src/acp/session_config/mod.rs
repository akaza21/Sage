//! Bounded ACP v1 session configuration. This state is never journaled.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::contract::{BoundedVec, RequiredText, SafeText};
use super::limits::LIMITS;

mod kind;
mod overlay;
pub(crate) use kind::ConfigKind;
pub(crate) use overlay::{
    ConfigChannel, ConfigIntent, ConfigMutation, ConfigRevision, NoticeSink, SessionConfigOverlay,
};

pub(crate) type ConfigId = RequiredText<1024>;
type Label = RequiredText<4096>;
type Description = SafeText<16384>;
pub(crate) type ConfigOptions = BoundedVec<ConfigOption, 128>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Mode {
    pub id: ConfigId,
    pub name: Label,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Description>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Modes {
    pub current_mode_id: ConfigId,
    pub available_modes: BoundedVec<Mode, 128>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Choice {
    pub value: ConfigId,
    pub name: Label,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Description>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChoiceGroup {
    pub group: ConfigId,
    pub name: Label,
    pub options: BoundedVec<Choice, 128>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Choices {
    Flat(BoundedVec<Choice, 128>),
    Grouped(BoundedVec<ChoiceGroup, 128>),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConfigOption {
    pub id: ConfigId,
    pub name: Label,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Description>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<ConfigId>,
    #[serde(flatten)]
    pub kind: ConfigKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionConfig {
    #[serde(default)]
    pub modes: Option<Modes>,
    #[serde(default)]
    pub config_options: ConfigOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_model: Option<ConfirmedModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConfirmedModel {
    pub id: RequiredText<512>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<RequiredText<512>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum ConfigValue {
    Select { value: ConfigId },
    Boolean { value: bool },
}
impl<'de> Deserialize<'de> for ConfigValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            #[serde(rename = "type")]
            kind: Option<String>,
            value: Value,
        }
        let input = Input::deserialize(d)?;
        match (input.kind.as_deref(), input.value) {
            (Some("boolean"), Value::Bool(value)) => Ok(Self::Boolean { value }),
            (Some("boolean"), _) => Err(serde::de::Error::custom(
                "expected boolean value for boolean config",
            )),
            (_, Value::String(value)) => ConfigId::new("value", value)
                .map(|value| Self::Select { value })
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "expected string value or typed boolean value",
            )),
        }
    }
}
impl ConfigValue {
    pub(crate) fn wire(&self) -> Value {
        match self {
            Self::Select { value } => serde_json::json!({"value":value}),
            Self::Boolean { value } => serde_json::json!({"type":"boolean","value":value}),
        }
    }
}

impl SessionConfig {
    pub(crate) fn from_wire(value: &Value, boolean_supported: bool) -> Result<Self, String> {
        // Parse away tolerated extensions before charging the canonical admitted payload.
        let value = strip_meta(value);
        let config = serde_json::json!({
            "modes":value.get("modes"),
            "configOptions":value.get("configOptions").cloned().unwrap_or_else(|| serde_json::json!([]))
        });
        validate_shapes(&config)?;
        let mut result: Self = serde_json::from_value(config).map_err(|e| e.to_string())?;
        if !boolean_supported {
            result.demote_unnegotiated_booleans()?;
        }
        result.validate()?;
        Ok(result)
    }

    fn demote_unnegotiated_booleans(&mut self) -> Result<(), String> {
        let options = self
            .config_options
            .as_slice()
            .iter()
            .map(|option| {
                Ok(ConfigOption {
                    kind: option.kind.demoted()?,
                    ..option.clone()
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.config_options =
            ConfigOptions::new("configOptions", options).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len()
            > LIMITS.wire.models_or_config_bytes as usize
        {
            return Err("session config byte limit exceeded".into());
        }
        if let Some(modes) = &self.modes {
            unique(
                modes
                    .available_modes
                    .as_slice()
                    .iter()
                    .map(|v| v.id.as_str()),
            )?;
            self.validate_mode(modes.current_mode_id.as_str())?;
        }
        unique(self.config_options.as_slice().iter().map(|v| v.id.as_str()))?;
        let mut entries = self.config_options.as_slice().len()
            + usize::from(self.confirmed_model.is_some())
            + self
                .modes
                .as_ref()
                .map_or(0, |m| m.available_modes.as_slice().len());
        for option in self.config_options.as_slice() {
            match &option.kind {
                ConfigKind::Boolean { .. } | ConfigKind::Unsupported { .. } => {}
                ConfigKind::Select {
                    current_value,
                    options,
                } => {
                    let values = options.values()?;
                    entries += values.len();
                    if let Choices::Grouped(groups) = options {
                        entries += groups.as_slice().len();
                    }
                    unique(values.iter().copied())?;
                    if !values.contains(&current_value.as_str()) {
                        return Err("select currentValue not advertised".into());
                    }
                }
            }
        }
        if entries > LIMITS.wire.models_or_config_entries as usize {
            return Err("session config entry limit exceeded".into());
        }
        Ok(())
    }

    pub(crate) fn validate_mode(&self, mode: &str) -> Result<(), String> {
        self.modes
            .as_ref()
            .filter(|m| {
                m.available_modes
                    .as_slice()
                    .iter()
                    .any(|v| v.id.as_str() == mode)
            })
            .map(|_| ())
            .ok_or_else(|| "session mode not advertised".into())
    }

    pub(crate) fn set_current_mode(&mut self, mode: ConfigId) -> Result<(), String> {
        self.validate_mode(mode.as_str())?;
        let mut candidate = self.clone();
        if let Some(modes) = &mut candidate.modes {
            modes.current_mode_id = mode;
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn validate_value(&self, id: &str, value: &ConfigValue) -> Result<(), String> {
        let option = self
            .config_options
            .as_slice()
            .iter()
            .find(|o| o.id.as_str() == id)
            .ok_or_else(|| "session config ID not advertised".to_owned())?;
        match (&option.kind, value) {
            (ConfigKind::Boolean { .. }, ConfigValue::Boolean { .. }) => Ok(()),
            (ConfigKind::Select { options, .. }, ConfigValue::Select { value })
                if options.values()?.contains(&value.as_str()) =>
            {
                Ok(())
            }
            _ => Err("session config value not advertised".into()),
        }
    }
}
impl Choices {
    fn values(&self) -> Result<Vec<&str>, String> {
        match self {
            Self::Flat(values) => Ok(values.as_slice().iter().map(|v| v.value.as_str()).collect()),
            Self::Grouped(groups) => {
                unique(groups.as_slice().iter().map(|v| v.group.as_str()))?;
                Ok(groups
                    .as_slice()
                    .iter()
                    .flat_map(|g| g.options.as_slice())
                    .map(|v| v.value.as_str())
                    .collect())
            }
        }
    }
}
fn unique<'a>(values: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err("duplicate session config ID".into());
        }
    }
    Ok(())
}
fn strip_meta(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| k.as_str() != "_meta")
                .map(|(k, v)| (k.clone(), strip_meta(v)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(strip_meta).collect()),
        _ => value.clone(),
    }
}
fn validate_shapes(value: &Value) -> Result<(), String> {
    if let Some(options) = value["configOptions"].as_array() {
        for option in options {
            if option["type"] != "select" {
                continue;
            }
            let values = option["options"]
                .as_array()
                .ok_or("select options must be an array")?;
            let grouped = values.first().is_some_and(|v| v.get("group").is_some());
            for choice in values {
                if grouped != choice.get("group").is_some()
                    || (grouped && choice.get("value").is_some())
                    || (!grouped && choice.get("options").is_some())
                {
                    return Err("mixed or malformed config choice groups".into());
                }
                if grouped {
                    let children = choice["options"]
                        .as_array()
                        .ok_or("group options must be an array")?;
                    if children
                        .iter()
                        .any(|v| v.get("group").is_some() || v.get("options").is_some())
                    {
                        return Err("nested config groups are unsupported".into());
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
