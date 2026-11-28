//! macOS dictation: the Speech framework (`SFSpeechRecognizer`) fed from the
//! microphone through an `AVAudioEngine` input tap. On-device recognition is
//! required whenever the recognizer supports it, so audio stays local and the
//! server's one-minute task limit does not apply.

use std::{
    ptr::NonNull,
    sync::{Arc, mpsc},
    time::Duration,
};

use block2::RcBlock;
use objc2::{AllocAnyThread, msg_send, rc::Retained, runtime::AnyClass, runtime::Bool, sel};
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
use objc2_foundation::{NSBundle, NSError, NSObjectProtocol, NSOperationQueue, NSString};
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
    SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};

use super::{Gate, MAX_TRANSCRIPT_CHARS, Signal, Sink};

pub(super) const STOP_ERROR: &str = "macOS dictation could not stop.";
const PROMPT_WAIT: Duration = Duration::from_secs(120);
const TAP_FRAMES: u32 = 1024;
const MIC_DENIED: &str = "Microphone access is off for Sage. Turn it on in System Settings > Privacy & Security > Microphone.";
const SPEECH_DENIED: &str = "Speech recognition is off for Sage. Turn it on in System Settings > Privacy & Security > Speech Recognition.";
const UNAVAILABLE: &str =
    "macOS dictation is unavailable. Check the system language and Siri & Dictation settings.";

// AVFoundation's media type constant; `AVCaptureDevice` is reached through the
// runtime so the large AVFoundation binding crate is not needed.
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

pub(super) struct Active {
    engine: Retained<AVAudioEngine>,
    request: Retained<SFSpeechAudioBufferRecognitionRequest>,
    task: Retained<SFSpeechRecognitionTask>,
    _recognizer: Retained<SFSpeechRecognizer>,
    gate: Arc<Gate>,
    halted: bool,
}

// SAFETY: the seam keeps `Active` behind its mutex, so these objects are used by
// one thread at a time. AVAudioEngine and the Speech objects are not tied to
// the main thread; results arrive on the recognizer's own operation queue.
unsafe impl Send for Active {}

impl Active {
    fn halt(&mut self, cancel: bool) {
        self.gate.close();
        if std::mem::replace(&mut self.halted, true) {
            return;
        }
        // SAFETY: plain calls on live objects; each is a no-op when already stopped.
        unsafe {
            self.engine.stop();
            self.engine.inputNode().removeTapOnBus(0);
            self.request.endAudio();
            if cancel {
                self.task.cancel();
            } else {
                self.task.finish();
            }
        }
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.halt(true);
    }
}

pub(super) fn authorize(wanted: &dyn Fn() -> bool) -> Result<(), String> {
    require_usage_strings()?;
    speech_authorized()?;
    if !wanted() {
        return Err("Dictation was stopped.".into());
    }
    microphone_authorized()
}

pub(super) fn open(sink: Sink) -> Result<Active, String> {
    // SAFETY: Speech and AVFAudio calls on objects created here; the blocks are
    // retained by the framework for as long as it calls them.
    unsafe {
        let recognizer = SFSpeechRecognizer::init(SFSpeechRecognizer::alloc())
            .filter(|recognizer| recognizer.isAvailable())
            .ok_or(UNAVAILABLE)?;
        // A serial queue keeps partial and final results in order.
        let queue = NSOperationQueue::new();
        queue.setMaxConcurrentOperationCount(1);
        recognizer.setQueue(&queue);
        let request = SFSpeechAudioBufferRecognitionRequest::new();
        request.setShouldReportPartialResults(true);
        if recognizer.supportsOnDeviceRecognition() {
            request.setRequiresOnDeviceRecognition(true);
        }
        // addsPunctuation is macOS 13+; Sage supports macOS 11.
        if request.respondsToSelector(sel!(setAddsPunctuation:)) {
            request.setAddsPunctuation(true);
        }
        let engine = AVAudioEngine::new();
        let input = engine.inputNode();
        let format = input.outputFormatForBus(0);
        if format.sampleRate() <= 0.0 || format.channelCount() == 0 {
            return Err("No microphone is available.".into());
        }
        let gate = Arc::new(Gate::default());
        let task = recognizer
            .recognitionTaskWithRequest_resultHandler(&request, &result_handler(sink, &gate));
        let mut active = Active {
            engine,
            request,
            task,
            _recognizer: recognizer,
            gate,
            halted: false,
        };
        let tap_request = active.request.clone();
        let tap = RcBlock::new(
            move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                tap_request.appendAudioPCMBuffer(buffer.as_ref());
            },
        );
        input.installTapOnBus_bufferSize_format_block(
            0,
            TAP_FRAMES,
            Some(&format),
            RcBlock::as_ptr(&tap),
        );
        active.engine.prepare();
        if active.engine.startAndReturnError().is_err() {
            active.halt(true);
            return Err("The microphone could not start.".into());
        }
        Ok(active)
    }
}

pub(super) fn close(mut active: Active, cancel: bool) -> Result<(), ()> {
    active.halt(cancel);
    Ok(())
}

/// Streams the cumulative transcript as partials and the last result as final.
/// A session ends at the transcript cap, so its native result stays bounded.
fn result_handler(
    sink: Sink,
    gate: &Arc<Gate>,
) -> RcBlock<dyn Fn(*mut SFSpeechRecognitionResult, *mut NSError)> {
    let gate = gate.clone();
    RcBlock::new(
        move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
            // SAFETY: the framework passes valid or null pointers for this call.
            let (result, error) = unsafe { (result.as_ref(), error.as_ref()) };
            gate.deliver(|| {
                let mut done = error.is_some();
                if let Some(result) = result {
                    // SAFETY: reads on a live result object.
                    let (spoken, last) = unsafe {
                        (
                            result.bestTranscription().formattedString(),
                            result.isFinal(),
                        )
                    };
                    let full = spoken.length() >= MAX_TRANSCRIPT_CHARS;
                    let text = spoken.to_string();
                    done |= last || full;
                    sink(if done {
                        Signal::Final(text)
                    } else {
                        Signal::Partial(text)
                    });
                }
                if done {
                    if let Some(error) = error {
                        sink(Signal::Error(error.localizedDescription().to_string()));
                    }
                    sink(Signal::Completed);
                }
                done
            });
        },
    )
}

/// Speech authorization aborts the process when the usage string is missing,
/// so an unbundled or stripped build fails here with a message instead.
fn require_usage_strings() -> Result<(), String> {
    let info = NSBundle::mainBundle();
    for key in [
        "NSSpeechRecognitionUsageDescription",
        "NSMicrophoneUsageDescription",
    ] {
        if info
            .objectForInfoDictionaryKey(&NSString::from_str(key))
            .is_none()
        {
            return Err(format!("This Sage build is missing {key} in Info.plist."));
        }
    }
    Ok(())
}

fn speech_authorized() -> Result<(), String> {
    // SAFETY: class methods with no preconditions beyond the usage string checked above.
    let mut status = unsafe { SFSpeechRecognizer::authorizationStatus() };
    if status == SFSpeechRecognizerAuthorizationStatus::NotDetermined {
        let (sender, answer) = mpsc::sync_channel(1);
        let handler = RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
            let _ = sender.try_send(status);
        });
        unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
        status = answer
            .recv_timeout(PROMPT_WAIT)
            .map_err(|_| SPEECH_DENIED)?;
    }
    if status == SFSpeechRecognizerAuthorizationStatus::Authorized {
        Ok(())
    } else {
        Err(SPEECH_DENIED.into())
    }
}

/// `AVAuthorizationStatus`: 0 not determined, 1 restricted, 2 denied, 3 authorized.
fn microphone_authorized() -> Result<(), String> {
    const NOT_DETERMINED: isize = 0;
    const AUTHORIZED: isize = 3;
    let device = AnyClass::get(c"AVCaptureDevice").ok_or(MIC_DENIED)?;
    // SAFETY: documented AVCaptureDevice class methods (macOS 10.14+) called
    // with AVFoundation's own media type constant.
    let status: isize =
        unsafe { msg_send![device, authorizationStatusForMediaType: AVMediaTypeAudio] };
    let granted = match status {
        AUTHORIZED => true,
        NOT_DETERMINED => {
            let (sender, answer) = mpsc::sync_channel(1);
            let handler = RcBlock::new(move |granted: Bool| {
                let _ = sender.try_send(granted.as_bool());
            });
            let _: () = unsafe {
                msg_send![device, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*handler]
            };
            answer.recv_timeout(PROMPT_WAIT).unwrap_or(false)
        }
        _ => false,
    };
    if granted {
        Ok(())
    } else {
        Err(MIC_DENIED.into())
    }
}
