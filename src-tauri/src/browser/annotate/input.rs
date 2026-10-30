use super::{BrowserState, InspectOverlay, NativeOverlay, element::Rect, emit};
use crate::browser::devtools::{
    self,
    annotation_input::{AnnotationInput, InputTarget},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Manager};

pub(super) fn pick(
    app: &AppHandle,
    label: &str,
    input: AnnotationInput,
    navigation: u64,
    lease: Arc<AtomicBool>,
) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state, &app);
        let _ = state.annotate.run(
            |change| emit(&app, change),
            |mode| {
                let current = state
                    .tabs
                    .lock()
                    .ok()
                    .and_then(|tabs| tabs.snapshot(&label))
                    .is_some_and(|snapshot| {
                        !snapshot.loading && snapshot.navigation_id.unwrap_or(0) == navigation
                    });
                if !lease.load(Ordering::Acquire)
                    || !current
                    || mode.tab.as_deref() != Some(label.as_str())
                    || mode.pending.is_some()
                {
                    return (Vec::new(), Ok(()));
                }
                mode.select_with(&overlay, &label, || {
                    let result = match input.target {
                        InputTarget::Area => overlay.area(
                            &label,
                            Rect {
                                x: input.x,
                                y: input.y,
                                width: input.width,
                                height: input.height,
                            },
                            &|| state.annotate.freeze(&label),
                        ),
                        InputTarget::Element => state
                            .webview_for_tab(&label)
                            .ok_or("browser tab unavailable".into())
                            .and_then(|webview| devtools::annotation_node(&webview, input))
                            .and_then(|node| {
                                overlay.pick(&label, node, &|| state.annotate.freeze(&label))
                            }),
                    };
                    if result.is_err() {
                        let _ = overlay.start(&label);
                    }
                    result
                })
            },
        );
    });
}
