use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use url::Url;

use super::{BrowserState, BrowserTabs, MAX_BROWSER_TABS, admit_url};

const MAX_PENDING_POPUPS: usize = 8;

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub(super) use native::register_native;
mod agent;
mod opener;
mod settle;
#[cfg(all(windows, feature = "native-lane"))]
pub(super) use opener::ABANDONED;
#[cfg(windows)]
pub(super) use opener::navigation_admitted;
pub(super) use opener::{INITIAL_BLANK, emit_popup_intent, enqueue_intent, opener_has_room};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PopupIntent {
    pub(super) id: u64,
    pub(super) opener_tab_id: String,
    pub(super) url: String,
    /// Set only for a native (opener-linked) child, so the chrome can abandon it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) native_tab_id: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum PopupBegin {
    Create(String),
    Reuse(String),
}

enum PopupState {
    Pending,
    Opening,
    #[cfg_attr(not(windows), allow(dead_code))]
    NativeOpening,
    Opened(String),
    Quarantined(Option<String>),
}

#[derive(Default)]
pub(super) struct PopupRequests {
    next_id: u64,
    entries: BTreeMap<u64, (PopupIntent, PopupState)>,
    initial_blank: BTreeSet<String>,
    native_deadlines: BTreeMap<u64, std::time::Instant>,
}

impl PopupRequests {
    pub(super) fn enqueue<T>(
        &mut self,
        tabs: &BrowserTabs<T>,
        opener: &str,
        requested: &Url,
    ) -> Option<PopupIntent> {
        let url = popup_target(tabs, opener, requested);
        self.insert(opener, url, PopupState::Pending)
    }

    fn insert(
        &mut self,
        opener: &str,
        url: Option<String>,
        state: PopupState,
    ) -> Option<PopupIntent> {
        if self.entries.len() >= MAX_PENDING_POPUPS {
            return None;
        }
        let id = self.next_id.checked_add(1)?;
        let intent = PopupIntent {
            id,
            opener_tab_id: opener.to_owned(),
            url: url?,
            native_tab_id: None,
        };
        self.next_id = id;
        self.entries.insert(id, (intent.clone(), state));
        Some(intent)
    }

    fn pending(&self) -> Vec<PopupIntent> {
        self.entries
            .values()
            .filter_map(|(intent, state)| {
                (!matches!(
                    state,
                    PopupState::Quarantined(_) | PopupState::NativeOpening
                ))
                .then_some(intent.clone())
            })
            .collect()
    }

    fn begin<T>(
        &mut self,
        tabs: &BrowserTabs<T>,
        id: u64,
        opener: &str,
    ) -> Result<PopupBegin, String> {
        let (intent, state) = self
            .entries
            .get_mut(&id)
            .ok_or("popup request unavailable")?;
        if intent.opener_tab_id != opener || !tabs.entries.contains_key(opener) {
            return Err("popup opener unavailable".into());
        }
        let native_blank = intent.url == INITIAL_BLANK
            && matches!(state, PopupState::NativeOpening | PopupState::Opened(_));
        if admit_url(&intent.url).is_err() && !native_blank {
            return Err("popup URL unavailable".into());
        }
        match state {
            PopupState::Pending => {
                *state = PopupState::Opening;
                Ok(PopupBegin::Create(intent.url.clone()))
            }
            PopupState::Opened(tab_id) => Ok(PopupBegin::Reuse(tab_id.clone())),
            PopupState::Opening | PopupState::NativeOpening => {
                Err("popup open is in progress".into())
            }
            PopupState::Quarantined(_) => Err("popup native close is unresolved".into()),
        }
    }

    fn opened(&mut self, id: u64, tab_id: &str) -> bool {
        let Some((_, state)) = self.entries.get_mut(&id) else {
            return false;
        };
        if !matches!(state, PopupState::Opening) {
            return false;
        }
        *state = PopupState::Opened(tab_id.to_owned());
        true
    }

    fn quarantine(&mut self, id: u64, tab_id: Option<String>) {
        if let Some((_, state)) = self.entries.get_mut(&id) {
            *state = PopupState::Quarantined(tab_id);
        }
    }

    fn failed_clean(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    pub(super) fn take_for_opener(&mut self, opener: &str) -> Vec<(u64, String)> {
        let ids = self
            .entries
            .iter()
            .filter_map(|(id, (intent, _))| (intent.opener_tab_id == opener).then_some(*id))
            .collect::<Vec<_>>();
        let mut children = Vec::new();
        for id in ids {
            if let Some((_, PopupState::Opened(tab_id))) = self.entries.get_mut(&id) {
                let tab_id = tab_id.clone();
                children.push((id, tab_id.clone()));
                self.quarantine(id, Some(tab_id));
            } else if self.entries.get(&id).is_some_and(|(_, state)| {
                matches!(
                    state,
                    PopupState::Pending | PopupState::Opening | PopupState::NativeOpening
                )
            }) {
                self.entries.remove(&id);
            }
        }
        children
    }

    fn cleanup_target(&mut self, id: u64, opener: &str, tab_id: &str) -> bool {
        let Some((intent, state)) = self.entries.get_mut(&id) else {
            return false;
        };
        if intent.opener_tab_id != opener {
            return false;
        }
        match state {
            PopupState::Opened(opened) if opened == tab_id => {
                *state = PopupState::Quarantined(Some(tab_id.to_owned()));
                true
            }
            PopupState::Quarantined(Some(opened)) if opened == tab_id => true,
            _ => false,
        }
    }

    pub(super) fn finish_cleanup(&mut self, id: u64, tab_id: &str) {
        if self.entries.get(&id).is_some_and(
            |(_, state)| matches!(state, PopupState::Quarantined(Some(opened)) if opened == tab_id),
        ) {
            self.entries.remove(&id);
        }
    }

    pub(super) fn forget_child(&mut self, tab_id: &str) {
        self.initial_blank.remove(tab_id);
        self.entries.retain(
            |_, (_, state)| !matches!(state, PopupState::Opened(opened) if opened == tab_id),
        );
    }
}

pub(super) fn popup_target<T>(
    tabs: &BrowserTabs<T>,
    opener: &str,
    requested: &Url,
) -> Option<String> {
    if !opener_has_room(tabs, opener) {
        return None;
    }
    admit_url(requested.as_str())
        .ok()
        .map(|url| url.to_string())
}

#[tauri::command]
pub(crate) fn browser_popup_pending(
    state: State<'_, BrowserState>,
) -> Result<Vec<PopupIntent>, String> {
    Ok(state
        .popups
        .lock()
        .map_err(|error| error.to_string())?
        .pending())
}

#[tauri::command]
pub(crate) async fn browser_open_popup(
    app: AppHandle,
    state: State<'_, BrowserState>,
    intent_id: u64,
    opener_tab_id: String,
) -> Result<PopupOpenResult, String> {
    let begin = {
        let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
        state
            .popups
            .lock()
            .map_err(|error| error.to_string())?
            .begin(&tabs, intent_id, &opener_tab_id)?
    };
    match begin {
        PopupBegin::Reuse(tab_id) => {
            if !state
                .tabs
                .lock()
                .map_err(|error| error.to_string())?
                .entries
                .contains_key(&tab_id)
            {
                state
                    .popups
                    .lock()
                    .map_err(|error| error.to_string())?
                    .quarantine(intent_id, Some(tab_id));
                return Err("popup tab unavailable".into());
            }
            let url = state
                .popups
                .lock()
                .map_err(|error| error.to_string())?
                .entries
                .get(&intent_id)
                .ok_or("popup request unavailable")?
                .0
                .url
                .clone();
            Ok(PopupOpenResult {
                tab_id,
                opener_tab_id,
                url,
            })
        }
        PopupBegin::Create(url) => {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            tauri::async_runtime::spawn(async move {
                let result = finish_popup_open(&app, intent_id, opener_tab_id, url).await;
                let _ = sender.send(result);
            });
            receiver
                .await
                .map_err(|_| "popup open is unresolved".to_owned())?
        }
    }
}

async fn finish_popup_open(
    app: &AppHandle,
    intent_id: u64,
    opener_tab_id: String,
    url: String,
) -> Result<PopupOpenResult, String> {
    let state = app.state::<BrowserState>();
    let opened =
        super::lifecycle::open_browser_tab(app.clone(), &state, Some(url.clone()), false).await;
    let tab_id = match opened {
        Ok(tab_id) => tab_id,
        Err(error) => {
            let tabs = state.tabs.lock().map_err(|cause| cause.to_string())?;
            let mut popups = state.popups.lock().map_err(|cause| cause.to_string())?;
            if tabs.teardown.blocked_message().is_none() {
                popups.failed_clean(intent_id);
            } else {
                popups.quarantine(intent_id, None);
            }
            return Err(error);
        }
    };
    let retained = {
        let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
        let mut popups = state.popups.lock().map_err(|error| error.to_string())?;
        tabs.entries.contains_key(&opener_tab_id) && popups.opened(intent_id, &tab_id)
    };
    if !retained {
        let close = super::lifecycle::close_browser_tab(app, &state, tab_id.clone()).await;
        if close.is_err() {
            state
                .popups
                .lock()
                .map_err(|error| error.to_string())?
                .quarantine(intent_id, Some(tab_id));
        }
        return Err(close
            .err()
            .unwrap_or_else(|| "popup opener closed during open".into()));
    }
    emit_popup_intent(app, intent_id);
    Ok(PopupOpenResult {
        tab_id,
        opener_tab_id,
        url,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PopupOpenResult {
    tab_id: String,
    opener_tab_id: String,
    url: String,
}

#[tauri::command]
pub(crate) fn browser_popup_ack(
    app: AppHandle,
    state: State<'_, BrowserState>,
    intent_id: u64,
    opener_tab_id: String,
    tab_id: String,
) -> Result<(), String> {
    {
        let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
        if !tabs.entries.contains_key(&tab_id) {
            return Err("popup tab unavailable".into());
        }
        if !state.popups.lock().map_err(|error| error.to_string())?.ack(
            intent_id,
            &opener_tab_id,
            &tab_id,
        ) {
            return Err("popup acknowledgement unavailable".into());
        }
    }
    agent::join_opener_attachment(&app, &state, &opener_tab_id, &tab_id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn browser_popup_abandon(
    app: AppHandle,
    state: State<'_, BrowserState>,
    intent_id: u64,
    opener_tab_id: String,
    tab_id: String,
) -> Result<(), String> {
    let exists = {
        let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
        if !state
            .popups
            .lock()
            .map_err(|error| error.to_string())?
            .cleanup_target(intent_id, &opener_tab_id, &tab_id)
        {
            return Err("popup cleanup unavailable".into());
        }
        tabs.entries.contains_key(&tab_id)
    };
    if exists {
        super::lifecycle::close_browser_tab(&app, &state, tab_id.clone())
            .await
            .map_err(|error| format!("popup native close is unresolved: {error}"))?;
    } else if state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .teardown
        .blocked_message()
        .is_some()
    {
        return Err("popup native close is unresolved".into());
    }
    state
        .popups
        .lock()
        .map_err(|error| error.to_string())?
        .finish_cleanup(intent_id, &tab_id);
    Ok(())
}

#[cfg(test)]
#[path = "popups/tests.rs"]
mod tests;
