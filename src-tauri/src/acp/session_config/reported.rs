//! The mode and option values a session reports, as config choices.
use super::{ConfigKind, ConfigMutation, ConfigValue, SessionConfig};

/// The session's current value for the mode or option `choice` names, or
/// `None` when the session does not offer it.
pub(crate) fn current_value(
    config: &SessionConfig,
    choice: &ConfigMutation,
) -> Option<ConfigMutation> {
    match choice {
        ConfigMutation::Mode(_) => config
            .modes
            .as_ref()
            .map(|modes| ConfigMutation::Mode(modes.current_mode_id.clone())),
        ConfigMutation::Option { config_id, .. } => {
            let option = config
                .config_options
                .as_slice()
                .iter()
                .find(|option| &option.id == config_id)?;
            let value = match &option.kind {
                ConfigKind::Select { current_value, .. } => ConfigValue::Select {
                    value: current_value.clone(),
                },
                ConfigKind::Boolean { current_value } => ConfigValue::Boolean {
                    value: *current_value,
                },
                ConfigKind::Unsupported { .. } => return None,
            };
            Some(ConfigMutation::Option {
                config_id: config_id.clone(),
                value,
            })
        }
    }
}

/// Every mode and option value `config` reports.
pub(crate) fn values(config: &SessionConfig) -> Vec<ConfigMutation> {
    let mode = config
        .modes
        .as_ref()
        .map(|modes| ConfigMutation::Mode(modes.current_mode_id.clone()));
    let options = config
        .config_options
        .as_slice()
        .iter()
        .filter_map(|option| {
            current_value(
                config,
                &ConfigMutation::Option {
                    config_id: option.id.clone(),
                    value: ConfigValue::Boolean { value: false },
                },
            )
        });
    mode.into_iter().chain(options).collect()
}

/// The values `new` reports that differ from what `old` reported.
pub(crate) fn changes(old: &SessionConfig, new: &SessionConfig) -> Vec<ConfigMutation> {
    values(new)
        .into_iter()
        .filter(|value| current_value(old, value).as_ref() != Some(value))
        .collect()
}
