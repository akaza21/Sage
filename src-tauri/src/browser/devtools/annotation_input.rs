use super::cdp_port::annotate_input::binding_input;
pub(crate) use super::cdp_port::annotate_input::{AnnotationInput, InputTarget};
use super::{
    native::user_port,
    picks::{self, Registration},
};
use crate::acp::browser::BrowserFailure;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{AppHandle, Webview};
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use windows_core::PCWSTR;

type InputRegistration = (Registration, String, Arc<AtomicBool>);

thread_local! {
    static INPUT: RefCell<HashMap<String, InputRegistration>> = RefCell::new(HashMap::new());
}

pub(crate) fn watch(
    webview: &Webview,
    on_input: impl Fn(AnnotationInput, u64, Arc<AtomicBool>) + Send + 'static,
) -> Result<(), BrowserFailure> {
    let port = user_port(webview);
    let navigation = port.stamp()?.navigation_id;
    let context = port.annotation_context()?;
    let binding = format!("sageAnnotate{}", uuid::Uuid::new_v4().simple());
    let label = webview.label().to_owned();
    let name = binding.clone();
    let once = Arc::new(AtomicBool::new(false));
    let live = Arc::new(AtomicBool::new(true));
    picks::on_main(webview, move |core, _| {
        let expected = name.clone();
        let lease = live.clone();
        let registration = picks::listen(core, "Runtime.bindingCalled", move |_, payload| {
            if let Some(input) = binding_input(payload, &expected, context)
                && lease.load(Ordering::Acquire)
                && !once.swap(true, Ordering::AcqRel)
            {
                on_input(input, navigation, lease.clone());
            }
        })?;
        if let Some(previous) =
            INPUT.with(|input| input.borrow_mut().insert(label, (registration, name, live)))
        {
            remove(core, previous);
        }
        Ok(())
    })?;
    if let Err(error) = port.install_annotation_input(context, &binding) {
        let _ = unwatch(webview);
        return Err(error);
    }
    Ok(())
}

pub(crate) fn unwatch(webview: &Webview) -> Result<(), BrowserFailure> {
    let label = webview.label().to_owned();
    picks::on_main(webview, move |core, _| {
        if let Some(previous) = INPUT.with(|input| input.borrow_mut().remove(&label)) {
            remove(core, previous);
        }
        Ok(())
    })
}

pub(crate) fn detach(app: &AppHandle, label: &str) {
    let label = label.to_owned();
    let _ = app.run_on_main_thread(move || {
        if let Some((registration, _, live)) = INPUT.with(|input| input.borrow_mut().remove(&label))
        {
            live.store(false, Ordering::Release);
            picks::unregister(registration);
        }
    });
}

fn remove(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    previous: InputRegistration,
) {
    previous.2.store(false, Ordering::Release);
    picks::unregister(previous.0);
    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let method = wide("Runtime.removeBinding");
    let params = wide(&json!({"name": previous.1}).to_string());
    let callback = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    let _ = unsafe {
        core.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &callback)
    };
}
