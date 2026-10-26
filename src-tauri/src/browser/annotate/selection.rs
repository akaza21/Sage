//! The element waiting for the user's comment (M1b). While it waits the live
//! page is hidden and the chrome draws the badge and the comment box over a
//! frozen frame of it: the page is a native child view above Sage's UI, so
//! nothing Sage draws can sit on the live page.

use serde::Serialize;

use super::{
    AnnotateChange, AnnotateMode, InspectOverlay, change,
    element::{self, BrowserElement, ElementBox, Picked, Viewport},
};

/// A JPEG of the viewport as it was when the element was picked.
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
    role: String,
    name: String,
    #[serde(rename = "box")]
    bounds: ElementBox,
    viewport: Viewport,
}

impl Selection {
    fn view(&self) -> SelectionView {
        SelectionView {
            role: self.picked.role.clone(),
            name: self.picked.name.clone(),
            bounds: self.picked.outline,
            viewport: self.viewport,
        }
    }
}

impl AnnotateMode {
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
            }
            Err(error) => announced.error = Some(error),
        }
        (vec![announced], Ok(()))
    }

    /// Drops the waiting element, shows the page and resumes hovering.
    pub(super) fn cancel(
        &mut self,
        overlay: &impl InspectOverlay,
        label: &str,
    ) -> (Vec<AnnotateChange>, Result<(), String>) {
        if self.tab.as_deref() != Some(label) || self.pending.take().is_none() {
            return (Vec::new(), Ok(()));
        }
        let released = overlay.release(label, true, true);
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

    fn resumed(&self, label: &str, released: &Result<(), String>) -> AnnotateChange {
        let mut announced = change(label.to_owned(), true);
        announced.error = released.clone().err();
        announced
    }
}

#[cfg(test)]
#[path = "selection_tests.rs"]
pub(super) mod tests;
