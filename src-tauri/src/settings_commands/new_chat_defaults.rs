//! The new chat defaults commands. Saving checks the harness against the
//! discovered catalog; reading reports what discovery says about each stored
//! value now, so a value that stops working later is reported and kept rather
//! than rewritten. A model is judged against the models discovery lists for
//! the harness. Most harnesses only offer models, and every harness only
//! offers efforts, through a live session's configuration, so those values
//! report `unknown` here and are checked when a new chat starts.

use serde::Serialize;
use tauri::State;

use crate::{
    domain::HarnessInstallState,
    error::{AppError, AppResult},
    harnesses::HarnessCatalog,
    settings_store::{NewChatDefaults, SettingsStore},
    state::AppState,
};

/// What harness discovery says about the stored default harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DefaultHarnessStatus {
    Available,
    /// Discovery has not finished, so the harness cannot be judged yet.
    Detecting,
    Unavailable,
    NotInstalled,
    /// The harness is no longer in Sage's catalog.
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

/// What discovery says about a stored default model or effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DefaultOptionStatus {
    /// Discovery lists it for the default harness.
    Available,
    /// Discovery lists the harness's options and this one is not among them.
    Unavailable,
    /// Nothing discovered can judge it yet: the harness is not available, or
    /// it offers the option only through a live session.
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
    if harness_status(catalog, harness_id) != DefaultHarnessStatus::Available {
        return DefaultOptionStatus::Unknown;
    }
    match catalog.models(harness_id) {
        Ok(models) if !models.is_empty() => {
            if models.iter().any(|model| model.id == model_id) {
                DefaultOptionStatus::Available
            } else {
                DefaultOptionStatus::Unavailable
            }
        }
        _ => DefaultOptionStatus::Unknown,
    }
}

fn state_of(catalog: &HarnessCatalog, defaults: NewChatDefaults) -> NewChatDefaultsState {
    let harness_id = defaults.harness_id.as_deref();
    let harness_status = harness_id.map(|harness_id| harness_status(catalog, harness_id));
    let model_status = harness_id
        .zip(defaults.model_id.as_deref())
        .map(|(harness_id, model_id)| model_status(catalog, harness_id, model_id));
    // No discovery source lists efforts; the live session is the only judge.
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

/// Refuses a harness that discovery knows cannot run. While discovery is
/// still running the harness cannot be judged, so it is kept and reported as
/// detecting until discovery settles.
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
