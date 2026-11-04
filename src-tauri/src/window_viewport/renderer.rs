use tokio::sync::oneshot;
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, ExecuteScriptCompletedHandler,
    Microsoft::Web::WebView2::Win32::{ICoreWebView2Controller, ICoreWebView2Controller3},
};
use windows::{
    Win32::{
        Foundation::{E_UNEXPECTED, HWND, RECT},
        UI::WindowsAndMessaging::{GetClientRect, IsIconic},
    },
    core::{Interface, PCWSTR, w},
};

type Completion = oneshot::Receiver<Result<(), String>>;
const MAX_OBSERVATION_BYTES: usize = 256;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Observation {
    width: f64,
    height: f64,
    pixel_ratio: f64,
}

pub(super) enum Correction {
    Unchanged,
    Deferred,
    Issued(Completion),
}

pub(super) fn observe(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<oneshot::Receiver<Result<Observation, String>>> {
    let (sender, receiver) = oneshot::channel();
    let mut sender = Some(sender);
    let done = ExecuteScriptCompletedHandler::create(Box::new(move |outcome, json| {
        let result = outcome.map_err(|error| error.to_string()).and_then(|()| {
            if json.len() > MAX_OBSERVATION_BYTES {
                return Err("Renderer viewport observation exceeds its bound".into());
            }
            serde_json::from_str::<Observation>(&json).map_err(|error| error.to_string())
        });
        if let Some(sender) = sender.take() {
            let _ = sender.send(result);
        }
        Ok(())
    }));
    unsafe {
        controller.CoreWebView2()?.ExecuteScript(
            w!("({width:innerWidth,height:innerHeight,pixelRatio:devicePixelRatio})"),
            &done,
        )?;
    }
    Ok(receiver)
}

pub(super) fn correct(
    parent: HWND,
    controller: &ICoreWebView2Controller,
    observed: Observation,
) -> windows::core::Result<Correction> {
    unsafe {
        if IsIconic(parent).as_bool() {
            return Ok(Correction::Deferred);
        }
        let mut client = RECT::default();
        GetClientRect(parent, &mut client)?;
        if client.right <= 0 || client.bottom <= 0 {
            return Ok(Correction::Deferred);
        }
        if ![observed.width, observed.height, observed.pixel_ratio]
            .into_iter()
            .all(f64::is_finite)
            || observed.width < 0.0
            || observed.height < 0.0
            || observed.pixel_ratio <= 0.0
        {
            return Err(windows::core::Error::from_hresult(E_UNEXPECTED));
        }
        // CSS viewport dimensions round to integers at fractional DPI/zoom.
        if (f64::from(client.right) - observed.width * observed.pixel_ratio).abs()
            <= observed.pixel_ratio
            && (f64::from(client.bottom) - observed.height * observed.pixel_ratio).abs()
                <= observed.pixel_ratio
        {
            return Ok(Correction::Unchanged);
        }
        let mut rasterization = 1.0_f64;
        controller
            .cast::<ICoreWebView2Controller3>()?
            .RasterizationScale(&mut rasterization)?;
        if !rasterization.is_finite() || rasterization <= 0.0 {
            return Err(windows::core::Error::from_hresult(E_UNEXPECTED));
        }
        let params = serde_json::json!({
            "width": (f64::from(client.right) / rasterization).round() as i32,
            "height": (f64::from(client.bottom) / rasterization).round() as i32,
        })
        .to_string()
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
        let (sender, completion) = oneshot::channel();
        let mut sender = Some(sender);
        let done =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |outcome, _| {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(outcome.map_err(|error| error.to_string()));
                }
                Ok(())
            }));
        // Chromium's ScreenshotCaptured restores a saved renderer size after a
        // concurrent native restore. The controller already caches the correct
        // bounds, so applying identical controller bounds cannot repair it.
        // The deprecated CDP visible-size operation remains available through
        // WebView2. It changes the renderer view, leaving device-metrics and
        // zoom overrides disabled; later native resizes remain authoritative.
        controller.CoreWebView2()?.CallDevToolsProtocolMethod(
            w!("Emulation.setVisibleSize"),
            PCWSTR(params.as_ptr()),
            &done,
        )?;
        Ok(Correction::Issued(completion))
    }
}
