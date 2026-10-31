//! Annotate's WebView2 side: isolated-world trusted input and hiding the
//! page behind its frozen frame while the user comments.

use std::{sync::mpsc, time::Duration};

use tauri::{Manager, Webview};

use super::{InspectOverlay, NativeOverlay, element, selection::Selection};
use crate::browser::devtools;

impl NativeOverlay<'_> {
    fn webview(&self, label: &str) -> Result<Webview, String> {
        self.0
            .webview_for_tab(label)
            .ok_or("browser tab unavailable".into())
    }

    /// Placement and cover changes run on this same thread. Never take the
    /// Annotate mode lock here: its caller holds it while waiting for us.
    fn with_visible_page(
        &self,
        label: &str,
        step: impl FnOnce(Webview) -> tauri::Result<()> + Send + 'static,
    ) -> Result<(), String> {
        let webview = self.webview(label)?;
        let visibility = self
            .0
            .page_visibility_for_tab(label)
            .ok_or("browser tab unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.1
            .run_on_main_thread(move || {
                let result = visibility.reveal(|| step(webview).map_err(unavailable));
                let _ = sender.send(result);
            })
            .map_err(unavailable)?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(unavailable)?
    }
}

fn unavailable<E>(_: E) -> String {
    "browser annotate unavailable".into()
}

impl InspectOverlay for NativeOverlay<'_> {
    /// Listens for clicks first, so no click in inspect mode goes unheard.
    fn start(&self, label: &str) -> Result<(), String> {
        let webview = self.webview(label)?;
        let app = webview.app_handle().clone();
        let tab = label.to_owned();
        devtools::annotation_input::watch(&webview, move |input, navigation, lease| {
            super::input::pick(&app, &tab, input, navigation, lease);
        })
        .map_err(unavailable)?;
        devtools::set_annotate(&webview, true).map_err(|error| {
            let _ = devtools::annotation_input::unwatch(&webview);
            unavailable(error)
        })
    }

    /// Always lets go of the click listener, which holds the app; a closing
    /// tab skips the overlay, whose webview may already be gone.
    fn stop(&self, label: &str, closing: bool) -> Result<(), String> {
        if closing {
            devtools::annotation_input::detach(self.1, label);
            return Ok(());
        }
        let webview = self.webview(label)?;
        let unwatched = devtools::annotation_input::unwatch(&webview).map_err(unavailable);
        devtools::set_annotate(&webview, false)
            .map_err(unavailable)
            .and(unwatched)
    }

    fn pick(
        &self,
        label: &str,
        backend_node_id: u64,
        freeze: &dyn Fn(),
    ) -> Result<Selection, String> {
        let webview = self.webview(label)?;
        let url = self
            .0
            .tabs
            .lock()
            .map_err(|_| "browser unavailable")?
            .openable_url(label)?;
        let node = devtools::pick_element(&webview, backend_node_id)?;
        let picked = element::pick(&node.page, &node.role, &node.name, label, &url)
            .and_then(|picked| {
                freeze();
                webview.hide().map_err(unavailable)?;
                Ok(picked)
            })
            .inspect_err(|_| {
                let _ = self.start(label);
            })?;
        Ok(Selection {
            masks: Vec::new(),
            crop_coverage: false,
            picked,
            viewport: node.page.viewport,
            frame: node.frame,
        })
    }

    fn area(
        &self,
        label: &str,
        rect: element::Rect,
        freeze: &dyn Fn(),
    ) -> Result<Selection, String> {
        let webview = self.webview(label)?;
        let url = self
            .0
            .tabs
            .lock()
            .map_err(|_| "browser unavailable")?
            .openable_url(label)?;
        let (page, rect, frame, crop_coverage) = devtools::capture_annotation_area(&webview, rect)?;
        let picked = super::area::pick(&page, rect, label, &url)?;
        freeze();
        webview.hide().map_err(unavailable)?;
        Ok(Selection {
            picked,
            viewport: page.viewport,
            frame,
            masks: page.masks,
            crop_coverage,
        })
    }

    fn release(&self, label: &str, reveal: bool, resume: bool) -> Result<(), String> {
        let webview = self.webview(label)?;
        let shown = if reveal {
            self.with_visible_page(label, |page| page.show())
        } else {
            Ok(())
        };
        let resumed = if resume { self.start(label) } else { Ok(()) };
        shown.and(resumed)
    }

    /// The hidden page keeps Windows keyboard focus until it is moved, so
    /// the comment box would get no typing, and a page shown again would
    /// get no Escape.
    fn focus(&self, label: &str, page: bool) -> Result<(), String> {
        if page {
            return self.with_visible_page(label, |page| page.set_focus());
        }
        let webview = self
            .1
            .get_webview("main")
            .ok_or("browser annotate unavailable")?;
        webview.set_focus().map_err(unavailable)
    }

    fn clear_outline(&self, label: &str) -> Result<(), String> {
        devtools::clear_annotate_outline(&self.webview(label)?).map_err(unavailable)
    }
}
