//! A stored value that stops working later is reported and kept rather than
//! rewritten. Models are checked against the catalog's known models; efforts
//! exist only in a live session, so they report `unknown` and a new chat
//! checks them against its session.

use sage_core::error::{AppError, AppResult};
use serde::Serialize;
use tauri::State;

use crate::{
    domain::HarnessInstallState,
    harnesses::HarnessCatalog,
    settings_store::{NewChatDefaults, SettingsStore},
    state::AppState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DefaultHarnessStatus {
    Available,
    Detecting,
    Unavailable,
    NotInstalled,
    /// No longer in Sage's catalog.
    Unknown,
}

impl From<HarnessInstallState> for DefaultHarnessStatus {
    fn from(state: HarnessInstallState) -> Self {
        match state {
            HarnessInstallState::Available => Self::Available,
            HarnessInstallState::Detecting => Self::Detecting,
            HarnessInstallState::Unavailable => Self::Unavailable,
            HarnessInstallState::NotInstalled => Self::NotInstalled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DefaultOptionStatus {
    Available,
    Unavailable,
    /// Cannot be judged: the harness is unavailable or lists no models, or the
    /// value is an effort.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewChatDefaultsState {
    pub defaults: NewChatDefaults,
    /// Each status is `None` when that value is not stored.
    pub harness_status: Option<DefaultHarnessStatus>,
    pub model_status: Option<DefaultOptionStatus>,
    pub effort_status: Option<DefaultOptionStatus>,
}

fn harness_status(catalog: &HarnessCatalog, harness_id: &str) -> DefaultHarnessStatus {
    catalog
        .install_state_of(harness_id)
        .map_or(DefaultHarnessStatus::Unknown, Into::into)
}

fn model_status(catalog: &HarnessCatalog, harness_id: &str, model_id: &str) -> DefaultOptionStatus {
    // Runs on the main thread, so it reads only models already known.
    match catalog.known_models(harness_id) {
        Ok(models) if models.is_empty() => DefaultOptionStatus::Unknown,
        Ok(models) if models.iter().any(|model| model.id == model_id) => {
            DefaultOptionStatus::Available
        }
        Ok(_) => DefaultOptionStatus::Unavailable,
        Err(_) => DefaultOptionStatus::Unknown,
    }
}

fn state_of(catalog: &HarnessCatalog, defaults: NewChatDefaults) -> NewChatDefaultsState {
    let harness_id = defaults.harness_id.as_deref();
    let harness_status = harness_id.map(|harness_id| harness_status(catalog, harness_id));
    let model_status =
        harness_id
            .zip(defaults.model_id.as_deref())
            .map(|(harness_id, model_id)| {
                if harness_status == Some(DefaultHarnessStatus::Available) {
                    model_status(catalog, harness_id, model_id)
                } else {
                    DefaultOptionStatus::Unknown
                }
            });
    let effort_status = defaults
        .effort
        .as_ref()
        .map(|_| DefaultOptionStatus::Unknown);
    NewChatDefaultsState {
        defaults,
        harness_status,
        model_status,
        effort_status,
    }
}

pub(crate) fn read_new_chat_defaults(
    settings: &SettingsStore,
    catalog: &HarnessCatalog,
) -> AppResult<NewChatDefaultsState> {
    Ok(state_of(catalog, settings.new_chat_defaults()?))
}

/// A harness still being detected is kept and reported as detecting.
pub(crate) fn save_new_chat_defaults(
    settings: &SettingsStore,
    catalog: &HarnessCatalog,
    defaults: NewChatDefaults,
) -> AppResult<NewChatDefaultsState> {
    if let Some(harness_id) = defaults.harness_id.as_deref() {
        match harness_status(catalog, harness_id) {
            DefaultHarnessStatus::Available | DefaultHarnessStatus::Detecting => {}
            DefaultHarnessStatus::Unknown => {
                return Err(AppError::InvalidRequest(
                    "the default harness is not in Sage's harness catalog".into(),
                ));
            }
            DefaultHarnessStatus::Unavailable | DefaultHarnessStatus::NotInstalled => {
                return Err(AppError::InvalidRequest(
                    "the default harness is not available on this computer".into(),
                ));
            }
        }
    }
    Ok(state_of(catalog, settings.set_new_chat_defaults(defaults)?))
}

#[tauri::command]
pub(crate) fn get_new_chat_defaults(
    state: State<'_, AppState>,
) -> Result<NewChatDefaultsState, String> {
    read_new_chat_defaults(&state.settings, &state.harnesses).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_new_chat_defaults(
    state: State<'_, AppState>,
    defaults: NewChatDefaults,
) -> Result<NewChatDefaultsState, String> {
    save_new_chat_defaults(&state.settings, &state.harnesses, defaults)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "new_chat_defaults/tests.rs"]
mod tests;
