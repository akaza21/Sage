//! Native composer dictation. One shared seam (commands, the `voice-input`
//! event, the transcript cap, one active session) over a small per-platform
//! engine: Windows speech on Windows, the Speech framework on macOS.
// Elsewhere only the unsupported path is live; the shared seam stays compiled
// so its tests run on every platform.
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

#[cfg(any(windows, target_os = "macos"))]
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::AppHandle;
#[cfg(any(windows, target_os = "macos"))]
use tauri::{Emitter, Manager};

#[cfg(target_os = "macos")]
mod mac_speech;
#[cfg(windows)]
mod win_speech;

#[cfg(target_os = "macos")]
use mac_speech as engine;
#[cfg(windows)]
use win_speech as engine;

#[cfg(any(windows, target_os = "macos"))]
const EVENT: &str = "voice-input";
pub(crate) const MAX_TRANSCRIPT_CHARS: usize = 4096;
const MAX_MESSAGE_CHARS: usize = 240;
#[cfg(not(any(windows, target_os = "macos")))]
const UNSUPPORTED: &str = "Native dictation is available on Windows and macOS only.";

/// What an engine reports. Engines never touch Tauri; the seam turns these
/// into bounded `voice-input` events.
pub(crate) enum Signal {
    Partial(String),
    Final(String),
    Error(String),
    /// The engine stopped on its own; the seam releases the session.
    Completed,
}

#[cfg(any(windows, target_os = "macos"))]
pub(crate) type Sink = Arc<dyn Fn(Signal) + Send + Sync>;

#[cfg(any(windows, target_os = "macos"))]
#[derive(Default)]
pub struct VoiceInputState(Mutex<VoiceSlot<engine::Active>>);

#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Default)]
pub struct VoiceInputState;

struct VoiceSlot<T> {
    active: Option<(String, T)>,
    /// The one start still waiting on permission prompts. A newer start
    /// supersedes it, and a stop withdraws it, so it stays bounded.
    pending: Option<String>,
}

impl<T> Default for VoiceSlot<T> {
    fn default() -> Self {
        Self {
            active: None,
            pending: None,
        }
    }
}

impl<T> VoiceSlot<T> {
    fn replace(&mut self, id: String, value: T) -> Option<(String, T)> {
        self.active.replace((id, value))
    }

    fn take_if(&mut self, id: &str) -> Option<T> {
        if self
            .active
            .as_ref()
            .is_some_and(|(current, _)| current == id)
        {
            self.active.take().map(|(_, value)| value)
        } else {
            None
        }
    }

    fn complete(&mut self, id: &str) -> bool {
        self.take_if(id).is_some()
    }

    fn begin(&mut self, id: &str) {
        self.pending = Some(id.to_owned());
    }

    fn is_pending(&self, id: &str) -> bool {
        self.pending.as_deref() == Some(id)
    }

    /// True only for the newest start that was not stopped while it waited.
    fn claim(&mut self, id: &str) -> bool {
        let current = self.is_pending(id);
        if current {
            self.pending = None;
        }
        current
    }

    fn stop(&mut self, id: &str) -> Option<T> {
        if self.is_pending(id) {
            self.pending = None;
        }
        self.take_if(id)
    }
}

/// Orders an engine's callbacks against shutdown: deliveries run one at a time,
/// none run after `close`, and `close` waits for one in flight.
#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
pub(crate) struct Gate(std::sync::Mutex<bool>);

#[cfg(any(target_os = "macos", test))]
impl Gate {
    fn closed(&self) -> std::sync::MutexGuard<'_, bool> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Runs `deliver` unless closed; it returns true when the engine finished,
    /// which closes the gate.
    pub(crate) fn deliver(&self, deliver: impl FnOnce() -> bool) {
        let mut closed = self.closed();
        if !*closed && deliver() {
            *closed = true;
        }
    }

    /// Closes the gate; false when it was already closed.
    pub(crate) fn close(&self) -> bool {
        !std::mem::replace(&mut *self.closed(), true)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceEvent<'a> {
    id: &'a str,
    kind: &'a str,
    text: Option<String>,
    message: Option<String>,
}

fn bounded(value: &str, cap: usize) -> String {
    value.chars().take(cap).collect()
}

impl<'a> VoiceEvent<'a> {
    fn new(id: &'a str, kind: &'a str, text: Option<&str>, message: Option<&str>) -> Self {
        Self {
            id,
            kind,
            text: text.map(|value| bounded(value, MAX_TRANSCRIPT_CHARS)),
            message: message.map(|value| bounded(value, MAX_MESSAGE_CHARS)),
        }
    }

    /// `None` for `Completed`: the seam settles the slot before it emits `ended`.
    fn from_signal(id: &'a str, signal: &Signal) -> Option<Self> {
        Some(match signal {
            Signal::Partial(text) => Self::new(id, "partial", Some(text), None),
            Signal::Final(text) => Self::new(id, "final", Some(text), None),
            Signal::Error(message) => Self::new(id, "error", None, Some(message)),
            Signal::Completed => return None,
        })
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(any(windows, target_os = "macos"))]
fn emit(app: &AppHandle, event: VoiceEvent<'_>) {
    let _ = app.emit_to("main", EVENT, event);
}

#[cfg(any(windows, target_os = "macos"))]
fn ended(app: &AppHandle, id: &str) {
    emit(app, VoiceEvent::new(id, "ended", None, None));
}

#[cfg(any(windows, target_os = "macos"))]
fn sink(app: &AppHandle, id: &str) -> Sink {
    let app = app.clone();
    let id = id.to_owned();
    Arc::new(move |signal| {
        if let Some(event) = VoiceEvent::from_signal(&id, &signal) {
            emit(&app, event);
            return;
        }
        let app = app.clone();
        let id = id.clone();
        drop(tauri::async_runtime::spawn_blocking(move || {
            let settled = app
                .state::<VoiceInputState>()
                .0
                .lock()
                .is_ok_and(|mut slot| slot.complete(&id));
            if settled {
                ended(&app, &id);
            }
        }));
    })
}

#[tauri::command]
pub async fn start_voice_input(app: AppHandle, id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid voice-input identity.".into());
    }
    tauri::async_runtime::spawn_blocking(move || start_inner(&app, id))
        .await
        .map_err(|_| "Voice input could not start.".to_string())?
}

#[tauri::command]
pub async fn stop_voice_input(app: AppHandle, id: String, cancel: bool) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid voice-input identity.".into());
    }
    tauri::async_runtime::spawn_blocking(move || stop_inner(&app, &id, cancel))
        .await
        .map_err(|_| "Voice input could not stop.".to_string())?
}

#[cfg(not(any(windows, target_os = "macos")))]
fn start_inner(_app: &AppHandle, _id: String) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn stop_inner(_app: &AppHandle, _id: &str, _cancel: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(any(windows, target_os = "macos"))]
fn start_inner(app: &AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<VoiceInputState>();
    let unavailable = |_| "Voice input is unavailable.";
    state.0.lock().map_err(unavailable)?.begin(&id);
    // Permission prompts can wait on the user, so they run outside the lock and
    // stop asking once this start is stopped or superseded.
    let authorized = engine::authorize(&|| state.0.lock().is_ok_and(|slot| slot.is_pending(&id)));
    let mut guard = state.0.lock().map_err(unavailable)?;
    if !guard.claim(&id) {
        return Ok(());
    }
    authorized?;
    if let Some((previous_id, previous)) = guard.active.take() {
        let _ = engine::close(previous, true);
        ended(app, &previous_id);
    }
    let active = engine::open(sink(app, &id))?;
    guard.replace(id, active);
    Ok(())
}

#[cfg(any(windows, target_os = "macos"))]
fn stop_inner(app: &AppHandle, id: &str, cancel: bool) -> Result<(), String> {
    let state = app.state::<VoiceInputState>();
    let mut guard = state.0.lock().map_err(|_| "Voice input is unavailable.")?;
    let Some(active) = guard.stop(id) else {
        return Ok(());
    };
    let result = engine::close(active, cancel);
    ended(app, id);
    result.map_err(|()| engine::STOP_ERROR.to_string())
}

#[cfg(test)]
mod tests;
