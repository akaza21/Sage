use super::{ConfigKind, ConfigMutation, ConfigValue, SessionConfig};

/// Returns `None` when the session does not report the mode or option, or the option has an unsupported kind.
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

pub(crate) fn changes(old: &SessionConfig, new: &SessionConfig) -> Vec<ConfigMutation> {
    values(new)
        .into_iter()
        .filter(|value| current_value(old, value).as_ref() != Some(value))
        .collect()
}

pub(crate) fn same_target(a: &ConfigMutation, b: &ConfigMutation) -> bool {
    match (a, b) {
        (ConfigMutation::Mode(_), ConfigMutation::Mode(_)) => true,
        (
            ConfigMutation::Option { config_id: a, .. },
            ConfigMutation::Option { config_id: b, .. },
        ) => a == b,
        _ => false,
    }
}
