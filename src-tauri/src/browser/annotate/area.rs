use serde::Deserialize;

use super::element::{self, Picked, Rect, Viewport};
use crate::acp::browser::redact_url;

pub(crate) const AREA_ELEMENTS: usize = 64;
pub(crate) const AREA_MASKS: usize = 512;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AreaNode {
    pub role: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AreaPage {
    pub title: String,
    pub viewport: Viewport,
    pub nodes: Vec<AreaNode>,
    pub masks: Vec<Rect>,
    #[serde(default, rename = "motionSafe")]
    pub motion_safe: bool,
}

pub(crate) fn pick(page: &AreaPage, rect: Rect, tab_id: &str, url: &str) -> Result<Picked, String> {
    let viewport = page.viewport;
    if !viewport.admitted()
        || page.nodes.len() > AREA_ELEMENTS
        || page.masks.len() > AREA_MASKS
        || ![rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|value| value.is_finite())
        || rect.width < 1.0
        || rect.height < 1.0
        || rect.x < viewport.x
        || rect.y < viewport.y
        || rect.x + rect.width > viewport.x + viewport.client_width + 1.0
        || rect.y + rect.height > viewport.y + viewport.client_height + 1.0
    {
        return Err("browser area bounds rejected".into());
    }
    let mut pieces = Vec::new();
    for node in &page.nodes {
        if node.role.len() > 64 || node.text.len() > 2048 {
            return Err("browser area summary rejected".into());
        }
        let role = element::settled(&node.role, 64, false);
        let text = element::settled(&node.text, 256, false);
        pieces.push(format!("{role}: {text}"));
    }
    let bounds = element::element_box(rect)?;
    let picked = Picked {
        tab_id: tab_id.to_owned(),
        url: redact_url(url),
        title: element::settled(&page.title, 256, false),
        selector: String::new(),
        role: String::new(),
        name: String::new(),
        text: element::settled(&pieces.join("\n"), 1024, true),
        bounds,
        outline: bounds,
    };
    element::confirm(&picked, 1, "")?;
    Ok(picked)
}

#[cfg(test)]
#[path = "area_tests.rs"]
mod tests;
