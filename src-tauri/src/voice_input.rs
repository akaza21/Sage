#[cfg(windows)]
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[cfg(windows)]
use serde::Serialize;
use tauri::AppHandle;
#[cfg(windows)]
use tauri::{Emitter, Manager};

#[cfg(windows)]
const EVENT: &str = "voice-input";
#[cfg(windows)]
const MAX_TRANSCRIPT_CHARS: usize = 4096;

#[cfg(windows)]
#[derive(Default)]
pub struct VoiceInputState(Mutex<VoiceSlot<ActiveVoice>>);

#[cfg(windows)]
struct VoiceSlot<T> {
    active: Option<(String, T)>,
}

#[cfg(windows)]
impl<T> Default for VoiceSlot<T> {
    fn default() -> Self {
        Self { active: None }
    }
}

#[cfg(windows)]
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
}

#[cfg(not(windows))]
#[derive(Default)]
pub struct VoiceInputState;

#[cfg(windows)]
struct ActiveVoice {
    recognizer: windows::Media::SpeechRecognition::SpeechRecognizer,
    session: Option<windows::Media::SpeechRecognition::SpeechContinuousRecognitionSession>,
    hypothesis_token: Option<i64>,
    result_token: Option<i64>,
    completed_token: Option<i64>,
    closed: Arc<AtomicBool>,
}

#[cfg(windows)]
impl ActiveVoice {
    fn new(recognizer: windows::Media::SpeechRecognition::SpeechRecognizer) -> Self {
        Self {
            recognizer,
            session: None,
            hypothesis_token: None,
            result_token: None,
            completed_token: None,
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn detach_handlers(&mut self) {
        self.closed.store(true, Ordering::Release);
        if let Some(token) = self.hypothesis_token.take() {
            let _ = self.recognizer.RemoveHypothesisGenerated(token);
        }
        if let Some(session) = self.session.as_ref() {
            if let Some(token) = self.result_token.take() {
                let _ = session.RemoveResultGenerated(token);
            }
            if let Some(token) = self.completed_token.take() {
                let _ = session.RemoveCompleted(token);
            }
        }
    }
}

#[cfg(windows)]
impl Drop for ActiveVoice {
    fn drop(&mut self) {
        self.detach_handlers();
        let _ = self.recognizer.Close();
    }
}

#[cfg(windows)]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceEvent<'a> {
    id: &'a str,
    kind: &'a str,
    text: Option<&'a str>,
    message: Option<&'a str>,
}

#[cfg(windows)]
fn emit(app: &AppHandle, id: &str, kind: &str, text: Option<&str>, message: Option<&str>) {
    let bounded = text.map(|value| value.chars().take(MAX_TRANSCRIPT_CHARS).collect::<String>());
    let _ = app.emit_to(
        "main",
        EVENT,
        VoiceEvent {
            id,
            kind,
            text: bounded.as_deref(),
            message,
        },
    );
}

#[tauri::command]
pub async fn start_voice_input(app: AppHandle, id: String) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("Invalid voice-input identity.".into());
    }
    tauri::async_runtime::spawn_blocking(move || start_inner(&app, id))
        .await
        .map_err(|_| "Voice input could not start.".to_string())?
}

#[tauri::command]
pub async fn stop_voice_input(app: AppHandle, id: String, cancel: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || stop_inner(&app, &id, cancel))
        .await
        .map_err(|_| "Voice input could not stop.".to_string())?
}

#[cfg(not(windows))]
fn start_inner(_app: &AppHandle, _id: String) -> Result<(), String> {
    Err("Native dictation is available on Windows only.".into())
}

#[cfg(not(windows))]
fn stop_inner(_app: &AppHandle, _id: &str, _cancel: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn start_inner(app: &AppHandle, id: String) -> Result<(), String> {
    use windows::{
        Media::SpeechRecognition::{
            ISpeechRecognitionConstraint, SpeechContinuousRecognitionCompletedEventArgs,
            SpeechContinuousRecognitionResultGeneratedEventArgs,
            SpeechContinuousRecognitionSession, SpeechRecognitionHypothesisGeneratedEventArgs,
            SpeechRecognitionResultStatus, SpeechRecognitionScenario,
            SpeechRecognitionTopicConstraint, SpeechRecognizer,
        },
        core::{HSTRING, Interface},
    };

    let state = app.state::<VoiceInputState>();
    let mut guard = state.0.lock().map_err(|_| "Voice input is unavailable.")?;
    if let Some((previous_id, previous)) = guard.active.take() {
        let _ = close_active(previous, true);
        emit(app, &previous_id, "ended", None, None);
    }
    let mut active = ActiveVoice::new(SpeechRecognizer::new().map_err(|_| permission_error())?);
    let constraint = SpeechRecognitionTopicConstraint::Create(
        SpeechRecognitionScenario::Dictation,
        &HSTRING::from("Sage composer dictation"),
    )
    .map_err(|_| permission_error())?;
    active
        .recognizer
        .Constraints()
        .and_then(|constraints| {
            constraints.Append(&constraint.cast::<ISpeechRecognitionConstraint>()?)
        })
        .map_err(|_| permission_error())?;
    let compiled = active
        .recognizer
        .CompileConstraintsAsync()
        .and_then(|operation| operation.get())
        .map_err(|_| permission_error())?;
    if compiled.Status().map_err(|_| permission_error())? != SpeechRecognitionResultStatus::Success
    {
        return Err("Windows dictation is unavailable. Check speech language and Online speech recognition settings.".into());
    }
    let session = active
        .recognizer
        .ContinuousRecognitionSession()
        .map_err(|_| permission_error())?;
    active.session = Some(session.clone());
    let hypothesis_app = app.clone();
    let hypothesis_id = id.clone();
    let hypothesis_closed = active.closed.clone();
    active.hypothesis_token = Some(
        active
            .recognizer
            .HypothesisGenerated(&windows::Foundation::TypedEventHandler::<
                SpeechRecognizer,
                SpeechRecognitionHypothesisGeneratedEventArgs,
            >::new(move |_, args| {
                if !hypothesis_closed.load(Ordering::Acquire)
                    && let Some(args) = args.as_ref()
                    && let Ok(text) = args.Hypothesis().and_then(|hypothesis| hypothesis.Text())
                {
                    emit(
                        &hypothesis_app,
                        &hypothesis_id,
                        "partial",
                        Some(&text.to_string()),
                        None,
                    );
                }
                Ok(())
            }))
            .map_err(|_| permission_error())?,
    );
    let result_app = app.clone();
    let result_id = id.clone();
    let result_closed = active.closed.clone();
    active.result_token = Some(
        session
            .ResultGenerated(&windows::Foundation::TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionResultGeneratedEventArgs,
            >::new(move |_, args| {
                if !result_closed.load(Ordering::Acquire)
                    && let Some(args) = args.as_ref()
                    && let Ok(result) = args.Result()
                    && result
                        .Status()
                        .is_ok_and(|status| status == SpeechRecognitionResultStatus::Success)
                    && let Ok(text) = result.Text()
                {
                    emit(
                        &result_app,
                        &result_id,
                        "final",
                        Some(&text.to_string()),
                        None,
                    );
                }
                Ok(())
            }))
            .map_err(|_| permission_error())?,
    );
    let completed_app = app.clone();
    let completed_id = id.clone();
    let completed_closed = active.closed.clone();
    active.completed_token = Some(
        session
            .Completed(&windows::Foundation::TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionCompletedEventArgs,
            >::new(move |_, args| {
                if completed_closed.swap(true, Ordering::AcqRel) {
                    return Ok(());
                }
                if let Some(args) = args.as_ref()
                    && let Ok(status) = args.Status()
                    && status != SpeechRecognitionResultStatus::Success
                    && status != SpeechRecognitionResultStatus::UserCanceled
                {
                    emit(
                        &completed_app,
                        &completed_id,
                        "error",
                        None,
                        Some(permission_error()),
                    );
                }
                let cleanup_app = completed_app.clone();
                let cleanup_id = completed_id.clone();
                drop(tauri::async_runtime::spawn_blocking(move || {
                    let state = cleanup_app.state::<VoiceInputState>();
                    let settled = state
                        .0
                        .lock()
                        .is_ok_and(|mut slot| slot.complete(&cleanup_id));
                    if settled {
                        emit(&cleanup_app, &cleanup_id, "ended", None, None);
                    }
                }));
                Ok(())
            }))
            .map_err(|_| permission_error())?,
    );
    session
        .StartAsync()
        .and_then(|operation| operation.get())
        .map_err(|_| permission_error())?;
    guard.replace(id, active);
    Ok(())
}

#[cfg(windows)]
fn stop_inner(app: &AppHandle, id: &str, cancel: bool) -> Result<(), String> {
    let state = app.state::<VoiceInputState>();
    let mut guard = state.0.lock().map_err(|_| "Voice input is unavailable.")?;
    let Some(active) = guard.take_if(id) else {
        return Ok(());
    };
    let result = close_active(active, cancel);
    emit(app, id, "ended", None, None);
    result.map_err(|_| "Windows dictation could not stop.".to_string())
}

#[cfg(windows)]
fn close_active(mut active: ActiveVoice, cancel: bool) -> windows::core::Result<()> {
    active.detach_handlers();
    let session = active.session.as_ref().expect("active voice has a session");
    if cancel {
        session.CancelAsync()
    } else {
        session.StopAsync()
    }
    .and_then(|operation| operation.get())
}

#[cfg(windows)]
fn permission_error() -> &'static str {
    "Windows dictation failed. Check microphone access and Online speech recognition settings."
}

#[cfg(all(test, windows))]
mod tests {
    use super::VoiceSlot;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct CloseProbe(Arc<AtomicUsize>);

    impl Drop for CloseProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn autonomous_completion_closes_once_and_ignores_late_completion() {
        let closed = Arc::new(AtomicUsize::new(0));
        let mut slot = VoiceSlot::default();
        assert!(
            slot.replace("current".into(), CloseProbe(closed.clone()))
                .is_none()
        );
        assert!(!slot.complete("stale"));
        assert_eq!(closed.load(Ordering::SeqCst), 0);
        assert!(slot.complete("current"));
        assert_eq!(closed.load(Ordering::SeqCst), 1);
        assert!(!slot.complete("current"));
        assert_eq!(closed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stop_and_completion_race_releases_only_the_matching_session() {
        let closed = Arc::new(AtomicUsize::new(0));
        let mut slot = VoiceSlot::default();
        assert!(
            slot.replace("first".into(), CloseProbe(closed.clone()))
                .is_none()
        );
        assert!(slot.take_if("other").is_none());
        drop(slot.take_if("first"));
        assert!(!slot.complete("first"));
        assert_eq!(closed.load(Ordering::SeqCst), 1);
    }
}
