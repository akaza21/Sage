//! The Rust half of the Downloads/Tab/Comment/Escape regression: the real
//! cancellation command must leave the native WebView2 controller hidden.
//! Playwright separately drives those keys through the production components.

use std::time::Duration;

use sage_lib::native_lane::Lane;
use serde_json::json;

use crate::{
    report::{Report, wait_for},
    site::Site,
};

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("Annotate cancellation while Downloads covers WebView2");
    let result = covered_cancel(lane, site, report);
    report.check("annotate.completed", result.is_ok(), format!("{result:?}"));
}

fn covered_cancel(lane: &Lane, site: &Site, report: &mut Report) -> Result<(), String> {
    let url = site.url("/annotate.html");
    let tab = lane.open_tab(Some(&url), false)?;
    let settled = wait_for(Duration::from_secs(15), || lane.settled_url(&tab));
    if settled.as_deref() != Some(&url) {
        return Err(format!("page did not settle: {settled:?}"));
    }
    lane.place_browser(&tab)?;
    report.check(
        "annotate.placed",
        lane.browser_visible(&tab)?,
        "native controller visible after placement",
    );
    lane.annotate_control(&tab, "annotateOn")?;
    let document = lane.cdp(&tab, "DOM.getDocument", &json!({}))?;
    let node = lane.cdp(
        &tab,
        "DOM.querySelector",
        &json!({"nodeId": document["root"]["nodeId"], "selector": "#press"}),
    )?;
    let described = lane.cdp(&tab, "DOM.describeNode", &json!({"nodeId": node["nodeId"]}))?;
    let backend = described["node"]["backendNodeId"]
        .as_u64()
        .ok_or("no picked backend node")?;
    lane.pick_annotation(&tab, backend)?;
    report.check(
        "annotate.picked-hidden",
        !lane.browser_visible(&tab)?,
        "selection froze and hid the real page",
    );
    lane.cover_browser(&tab)?;
    lane.annotate_control(&tab, "annotateCancel")?;
    report.check(
        "annotate.covered-cancel-hidden",
        !lane.browser_visible(&tab)?,
        "Escape's cancellation command kept WebView2 hidden under Downloads",
    );
    report.check(
        "annotate.selection-released",
        lane.annotate_control(&tab, "annotateFrame").is_err(),
        "cancel released the frozen selection",
    );
    lane.place_browser(&tab)?;
    report.check(
        "annotate.uncovered-visible",
        lane.browser_visible(&tab)?,
        "closing Downloads and placing the page shows it again",
    );
    lane.evaluate(&tab, r#"for (const type of ['mousedown','mousemove','mouseup']) {
        document.body.dispatchEvent(new MouseEvent(type, {bubbles:true, button:0, clientX:200, clientY:160}));
    }"#, false)?;
    std::thread::sleep(Duration::from_millis(100));
    report.check(
        "annotate.page-script-cannot-pick",
        lane.annotate_control(&tab, "annotateFrame").is_err() && lane.browser_visible(&tab)?,
        "page-dispatched input did not capture or hide the native page",
    );
    lane.drag_annotation(&tab, 40.0, 20.0, 240.0, 160.0)?;
    report.check(
        "annotate.area-hidden",
        !lane.browser_visible(&tab)?,
        "trusted area drag froze the page",
    );
    lane.cover_browser(&tab)?;
    lane.zoom_browser(&tab, 125)?;
    let released = wait_for(Duration::from_secs(10), || {
        lane.annotate_control(&tab, "annotateFrame")
            .is_err()
            .then_some(())
    });
    report.check(
        "annotate.area-zoom-covered",
        released.is_some() && !lane.browser_visible(&tab)?,
        "zoom released the stale area but kept the page hidden under the popover",
    );
    lane.place_browser(&tab)?;
    report.check(
        "annotate.area-uncovered-visible",
        lane.browser_visible(&tab)?,
        "closing the popover restored the page",
    );
    lane.drag_annotation(&tab, 40.0, 20.0, 240.0, 160.0)?;
    report.check(
        "annotate.area-after-zoom",
        !lane.browser_visible(&tab)?,
        "Annotate stayed on and accepted another area at the new zoom",
    );
    lane.annotate_control(&tab, "annotateCancel")?;
    lane.annotate_control(&tab, "annotateOff")?;
    Ok(())
}
