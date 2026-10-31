//! The element waiting for the user's comment (M1b). While it waits the live
//! page is hidden and the chrome draws the badge and the comment box over a
//! frozen frame of it: the page is a native child view above Sage's UI, so
//! nothing Sage draws can sit on the live page.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::{
    AnnotateChange, AnnotateMode, BrowserState, Exit, InspectOverlay, NativeOverlay, change,
    element::{self, BrowserElement, ElementBox, Picked, Viewport},
    emit,
};

/// What the webview showed when the element was picked, as a JPEG.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Frame {
    pub jpeg_base64: String,
    pub width: u32,
    pub height: u32,
}

/// A picked element, the viewport it was picked in and the frozen frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Selection {
    pub picked: Picked,
    pub viewport: Viewport,
    pub frame: Frame,
}

/// What the chrome needs to draw the badge and place the comment box.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionView {
    /// What the comment box calls the element: its accessible name, or a
    /// plain word for its role.
    label: String,
    #[serde(rename = "box")]
    bounds: ElementBox,
    viewport: Viewport,
}

impl Selection {
    fn view(&self) -> SelectionView {
        SelectionView {
            label: label(&self.picked.role, &self.picked.name),
            bounds: self.picked.outline,
            viewport: self.viewport,
        }
    }
}

/// The element's accessible name, or a plain word for its role; never one of
/// Chromium's internal role names such as `LabelText`.
pub(crate) fn label(role: &str, name: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.to_owned();
    }
    match role {
        "link" | "button" | "heading" | "paragraph" | "checkbox" | "switch" | "slider" | "list"
        | "table" | "row" | "cell" | "form" | "navigation" | "dialog" | "tab" | "menu"
        | "article" | "banner" | "figure" | "region" | "search" | "separator" | "progressbar"
        | "tooltip" | "main" | "group" | "image" => role,
        "img" | "graphics-symbol" | "Canvas" | "Svg" | "video" | "Video" => "image",
        "LabelText" => "label",
        "StaticText" | "InlineTextBox" | "text" => "text",
        "textbox" | "searchbox" | "spinbutton" => "text field",
        "combobox" | "listbox" => "drop-down",
        "radio" => "radio button",
        "listitem" | "ListMarker" => "list item",
        "menuitem" | "menuitemcheckbox" | "menuitemradio" => "menu item",
        "gridcell" | "columnheader" | "rowheader" => "cell",
        "RootWebArea" | "WebArea" | "document" => "page",
        _ => "element",
    }
    .to_owned()
}

/// Escape inside the page: with an element waiting it drops the element and
/// Annotate stays on; otherwise Annotate turns off. The chrome follows the
/// same rule. Runs on its own thread, as the page's key handler is on the
/// main thread that the overlay needs.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn escape(app: &AppHandle, label: &str) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let overlay = NativeOverlay(&state, &app);
        let _ = state.annotate.run(
            |change| emit(&app, change),
            |mode| mode.escape(&overlay, &label, state.annotate.is_covered(&label)),
        );
        super::zoom::show_deferred(&app, &label);
    });
}

impl AnnotateMode {
    /// Escape's rule: cancel a waiting element, else leave Annotate.
    pub(super) fn escape(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
        covered: bool,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        if self.pending.is_some() {
            self.cancel(overlay, label, covered)
        } else {
            self.leave(overlay, label, Exit::Reveal)
        }
    }

    /// Picks the element Chromium reported for a click on `label`. A click
    /// while another element waits, or after Annotate ended, is ignored. A
    /// failed pick keeps Annotate on and tells the chrome why.
    pub(super) fn pick(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
        backend_node_id: u64,
        freeze: &dyn Fn(),
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        if self.tab.as_deref() != Some(label) || self.pending.is_some() {
            return (Vec::new(), Ok(()));
        }
        let mut announced = change(label.to_owned(), true);
        match overlay.pick(label, backend_node_id, freeze) {
            Ok(selection) => {
                announced.selection = Some(selection.view());
                self.pending = Some(selection);
                let _ = overlay.focus(label, false);
            }
            Err(error) => announced.error = Some(error),
        }
        (vec![announced], Ok(()))
    }

    /// Drops the waiting element and resumes hovering. A covered page stays
    /// hidden and leaves keyboard focus in the chrome.
    pub(super) fn cancel(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
        covered: bool,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        if self.tab.as_deref() != Some(label) || self.pending.take().is_none() {
            return (Vec::new(), Ok(()));
        }
        let released = overlay.release(label, !covered, true);
        // The page takes focus back, as in Codex, so the next Escape
        // reaches it and leaves Annotate.
        if !covered {
            let _ = overlay.focus(label, true);
        }
        (vec![self.resumed(label, &released)], released)
    }

    /// Numbers and comments the waiting element. The element keeps waiting:
    /// the chrome cancels the wait once the composer took the item, so an
    /// item the composer refuses can be sent again or dropped.
    pub(super) fn confirm(
        &self,
        label: &str,
        ordinal: u8,
        comment: &str,
    ) -> (Vec<AnnotateChange>, Result<BrowserElement, String>) {
        let Some(selection) = self
            .pending
            .as_ref()
            .filter(|_| self.tab.as_deref() == Some(label))
        else {
            return (Vec::new(), Err("no browser element is waiting".into()));
        };
        (
            Vec::new(),
            element::confirm(&selection.picked, ordinal, comment),
        )
    }

    /// The frozen frame of the waiting element on `label`.
    pub(super) fn frame(&self, label: &str) -> Result<Frame, String> {
        self.pending
            .as_ref()
            .filter(|_| self.tab.as_deref() == Some(label))
            .map(|selection| selection.frame.clone())
            .ok_or("no browser element is waiting".into())
    }

    pub(super) fn resumed(&self, label: &str, released: &Result<(), String>) -> AnnotateChange {
        let mut announced = change(label.to_owned(), true);
        announced.error = released.clone().err();
        announced
    }
}

#[cfg(test)]
#[path = "selection_tests.rs"]
pub(super) mod tests;
