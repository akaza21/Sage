//! Annotate's WebView2 side: the inspect overlay, the click report it raises
//! while on, and hiding the page behind its frozen frame.

use tauri::{Manager, Webview};

use super::{InspectOverlay, NativeOverlay, element, selection::Selection};
use crate::browser::devtools;

impl NativeOverlay<'_> {
    fn webview(&self, label: &str) -> Result<Webview, String> {
        self.0
            .webview_for_tab(label)
            .ok_or("browser tab unavailable".into())
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
        devtools::watch_picks(&webview, move |backend_node_id| {
            super::pick(&app, &tab, backend_node_id);
        })
        .map_err(unavailable)?;
        devtools::set_annotate(&webview, true).map_err(|error| {
            let _ = devtools::unwatch_picks(self.1, label);
            unavailable(error)
        })
    }

    /// Always lets go of the click listener, which holds the app; a closing
    /// tab skips the overlay, whose webview may already be gone.
    fn stop(&self, label: &str, closing: bool) -> Result<(), String> {
        let unwatched = devtools::unwatch_picks(self.1, label).map_err(unavailable);
        if closing {
            return unwatched;
        }
        let webview = self.webview(label)?;
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
                let _ = devtools::set_annotate(&webview, true);
            })?;
        Ok(Selection {
            picked,
            viewport: node.page.viewport,
            frame: node.frame,
        })
    }

    fn release(&self, label: &str, reveal: bool, resume: bool) -> Result<(), String> {
        let webview = self.webview(label)?;
        let shown = if reveal {
            webview.show().map_err(unavailable)
        } else {
            Ok(())
        };
        let resumed = if resume {
            devtools::set_annotate(&webview, true).map_err(unavailable)
        } else {
            Ok(())
        };
        shown.and(resumed)
    }

    /// The hidden page keeps Windows keyboard focus until it is moved, so
    /// the comment box would get no typing, and a page shown again would
    /// get no Escape.
    fn focus(&self, label: &str, page: bool) -> Result<(), String> {
        let webview = if page {
            self.webview(label)?
        } else {
            self.1
                .get_webview("main")
                .ok_or("browser annotate unavailable")?
        };
        webview.set_focus().map_err(unavailable)
    }

    fn clear_outline(&self, label: &str) -> Result<(), String> {
        devtools::clear_annotate_outline(&self.webview(label)?).map_err(unavailable)
    }
}
