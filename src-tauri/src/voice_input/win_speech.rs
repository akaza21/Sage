//! Windows dictation: continuous `SpeechRecognizer` sessions.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use windows::{
    Foundation::TypedEventHandler,
    Media::SpeechRecognition::{
        ISpeechRecognitionConstraint, SpeechContinuousRecognitionCompletedEventArgs,
        SpeechContinuousRecognitionResultGeneratedEventArgs, SpeechContinuousRecognitionSession,
        SpeechRecognitionHypothesisGeneratedEventArgs, SpeechRecognitionResultStatus,
        SpeechRecognitionScenario, SpeechRecognitionTopicConstraint, SpeechRecognizer,
    },
    core::{HSTRING, Interface},
};

use super::{Signal, Sink};

pub(super) const STOP_ERROR: &str = "Windows dictation could not stop.";

pub(super) struct Active {
    recognizer: SpeechRecognizer,
    session: Option<SpeechContinuousRecognitionSession>,
    hypothesis_token: Option<i64>,
    result_token: Option<i64>,
    completed_token: Option<i64>,
    closed: Arc<AtomicBool>,
}

impl Active {
    fn new(recognizer: SpeechRecognizer) -> Self {
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

impl Drop for Active {
    fn drop(&mut self) {
        self.detach_handlers();
        let _ = self.recognizer.Close();
    }
}

/// Windows asks for microphone access itself when the session starts.
pub(super) fn authorize() -> Result<(), String> {
    Ok(())
}

pub(super) fn open(sink: Sink) -> Result<Active, String> {
    let mut active = Active::new(SpeechRecognizer::new().map_err(|_| permission_error())?);
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
    let hypothesis_sink = sink.clone();
    let hypothesis_closed = active.closed.clone();
    active.hypothesis_token = Some(
        active
            .recognizer
            .HypothesisGenerated(&TypedEventHandler::<
                SpeechRecognizer,
                SpeechRecognitionHypothesisGeneratedEventArgs,
            >::new(move |_, args| {
                if !hypothesis_closed.load(Ordering::Acquire)
                    && let Some(args) = args.as_ref()
                    && let Ok(text) = args.Hypothesis().and_then(|hypothesis| hypothesis.Text())
                {
                    hypothesis_sink(Signal::Partial(text.to_string()));
                }
                Ok(())
            }))
            .map_err(|_| permission_error())?,
    );
    let result_sink = sink.clone();
    let result_closed = active.closed.clone();
    active.result_token = Some(
        session
            .ResultGenerated(&TypedEventHandler::<
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
                    result_sink(Signal::Final(text.to_string()));
                }
                Ok(())
            }))
            .map_err(|_| permission_error())?,
    );
    let completed_closed = active.closed.clone();
    active.completed_token = Some(
        session
            .Completed(&TypedEventHandler::<
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
                    sink(Signal::Error(permission_error().into()));
                }
                sink(Signal::Completed);
                Ok(())
            }))
            .map_err(|_| permission_error())?,
    );
    session
        .StartAsync()
        .and_then(|operation| operation.get())
        .map_err(|_| permission_error())?;
    Ok(active)
}

pub(super) fn close(mut active: Active, cancel: bool) -> Result<(), ()> {
    active.detach_handlers();
    let session = active.session.as_ref().expect("active voice has a session");
    if cancel {
        session.CancelAsync()
    } else {
        session.StopAsync()
    }
    .and_then(|operation| operation.get())
    .map_err(|_| ())
}

fn permission_error() -> &'static str {
    "Windows dictation failed. Check microphone access and Online speech recognition settings."
}
