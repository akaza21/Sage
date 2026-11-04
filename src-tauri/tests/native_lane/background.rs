//! M2: production hide/place preserves page state while changing memory targets.
use crate::{
    report::{Report, wait_for},
    site::Site,
};
use sage_lib::native_lane::Lane;
use serde_json::json;
use std::time::Duration;

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("M2 hidden browser memory targets");
    let result = exercise(lane, site, report);
    report.check("m2.completed", result.is_ok(), format!("{result:?}"));
}

fn exercise(lane: &Lane, site: &Site, report: &mut Report) -> Result<(), String> {
    let first = lane.open_tab(Some(&site.url("/landing.html")), false)?;
    let second = lane.open_tab(Some(&site.url("/form.html")), false)?;
    for tab in [&first, &second] {
        if wait_for(Duration::from_secs(15), || lane.settled_url(tab)).is_none() {
            return Err(format!("{tab} did not load"));
        }
    }
    report.check(
        "m2.created-hidden-low",
        lane.browser_memory_target(&first)? == "low",
        "unplaced page starts LOW",
    );
    lane.place_browser(&first)?;
    report.check(
        "m2.shown-normal",
        lane.browser_memory_target(&first)? == "normal",
        "placed page reports NORMAL",
    );
    let setup = lane.evaluate(
        &first,
        "document.body.style.height='6000px'; window.m2Value=17; scrollTo(0,731); scrollY",
        false,
    )?;
    report.check("m2.scroll-setup", setup == json!(731), format!("{setup}"));
    lane.hide_browser(&first)?;
    lane.place_browser(&second)?;
    report.check(
        "m2.hidden-low",
        lane.browser_memory_target(&first)? == "low",
        "hidden page reports LOW immediately",
    );
    report.check(
        "m2.other-shown-normal",
        lane.browser_memory_target(&second)? == "normal",
        "active page reports NORMAL",
    );
    lane.hide_browser(&second)?;
    lane.place_browser(&first)?;
    report.check(
        "m2.shown-normal-again",
        lane.browser_memory_target(&first)? == "normal",
        "returning page reports NORMAL",
    );
    report.check(
        "m2.other-hidden-low",
        lane.browser_memory_target(&second)? == "low",
        "previous page reports LOW",
    );
    let preserved = lane.evaluate(&first, "({scroll:scrollY,value:window.m2Value})", false)?;
    report.check(
        "m2.page-state",
        preserved == json!({"scroll":731,"value":17}),
        format!("{preserved}"),
    );
    Ok(())
}
