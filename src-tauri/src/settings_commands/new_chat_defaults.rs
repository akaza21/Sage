//! The new chat defaults commands. Saving checks the harness against the
//! discovered catalog; reading reports what discovery says about the stored
//! harness now, so a harness that was uninstalled later is reported and kept
//! rather than rewritten. Model and effort are checked against the live
//! session configuration when a chat starts, not here.

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewChatDefaultsState {
    pub defaults: NewChatDefaults,
    /// `None` when no default harness is stored.
    pub harness_status: Option<DefaultHarnessStatus>,
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

fn harness_status(catalog: &HarnessCatalog, harness_id: &str) -> DefaultHarnessStatus {
    let Some(definition) = catalog
        .definitions()
        .into_iter()
        .find(|definition| definition.id == harness_id)
    else {
        return DefaultHarnessStatus::Unknown;
    };
    [
        HarnessInstallState::Available,
        HarnessInstallState::Detecting,
        HarnessInstallState::Unavailable,
        HarnessInstallState::NotInstalled,
    ]
    .into_iter()
    .find(|state| state.as_str() == definition.install_state)
    .map_or(DefaultHarnessStatus::Unknown, Into::into)
}

fn state_of(catalog: &HarnessCatalog, defaults: NewChatDefaults) -> NewChatDefaultsState {
    let harness_status = defaults
        .harness_id
        .as_deref()
        .map(|harness_id| harness_status(catalog, harness_id));
    NewChatDefaultsState {
        defaults,
        harness_status,
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
