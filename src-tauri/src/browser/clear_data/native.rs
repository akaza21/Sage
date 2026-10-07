use std::{sync::mpsc, time::Duration};

use tauri::Webview;
use webview2_com::{
    ClearBrowsingDataCompletedHandler,
    Microsoft::Web::WebView2::Win32::{ICoreWebView2_13, ICoreWebView2Profile2},
};
use windows_core::Interface;

pub(super) enum ClearProfileOutcome {
    Completed(Result<(), String>),
    Pending(mpsc::Receiver<Result<(), String>>),
}

fn classify_clear_completion(
    completion: Result<Result<(), String>, mpsc::RecvTimeoutError>,
    receiver: mpsc::Receiver<Result<(), String>>,
) -> Result<ClearProfileOutcome, String> {
    match completion {
        Ok(result) => Ok(ClearProfileOutcome::Completed(result)),
        Err(mpsc::RecvTimeoutError::Timeout) => Ok(ClearProfileOutcome::Pending(receiver)),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("browser data clear callback disconnected".into())
        }
    }
}

pub(super) fn clear_profile(webview: &Webview) -> Result<ClearProfileOutcome, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let callback_sender = sender.clone();
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }?;
                let profile = unsafe { core.cast::<ICoreWebView2_13>()?.Profile()? }
                    .cast::<ICoreWebView2Profile2>()?;
                unsafe {
                    profile.ClearBrowsingDataAll(&ClearBrowsingDataCompletedHandler::create(
                        Box::new(move |status| {
                            let _ = callback_sender.send(status.map_err(|e| e.to_string()));
                            Ok(())
                        }),
                    ))
                }
            })();
            if let Err(error) = result {
                let _ = sender.send(Err(error.to_string()));
            }
        })
        .map_err(|error| error.to_string())?;
    classify_clear_completion(receiver.recv_timeout(Duration::from_secs(30)), receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_native_callback_is_a_failed_clear() {
        let (sender, receiver) = mpsc::sync_channel::<Result<(), String>>(1);
        drop(sender);
        let completion = receiver.recv_timeout(Duration::from_millis(1));
        assert!(classify_clear_completion(completion, receiver).is_err());
    }
}
