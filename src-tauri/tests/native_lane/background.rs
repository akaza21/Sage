//! M2: production hide/place and CDP wake, observing native IsSuspended.
use crate::{
    report::{Report, wait_for},
    site::Site,
};
use sage_lib::native_lane::Lane;
use serde_json::json;
use std::time::Duration;

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("M2 hidden browser background and wake");
    let result = exercise(lane, site, report);
    report.check("m2.completed", result.is_ok(), format!("{result:?}"));
}

fn exercise(lane: &Lane, site: &Site, report: &mut Report) -> Result<(), String> {
    lane.show_browser_window()?;
    let first = lane.open_tab(Some(&site.url("/landing.html")), false)?;
    let second = lane.open_tab(Some(&site.url("/form.html")), false)?;
    for tab in [&first, &second] {
        if wait_for(Duration::from_secs(15), || lane.settled_url(tab)).is_none() {
            return Err(format!("{tab} did not load"));
        }
    }
    lane.place_browser(&first)?;
    // A single setup evaluation; no probes through CDP during the grace.
    let setup = lane.evaluate(&first, "document.body.style.height='6000px'; window.m2Counter=17; window.m2Timer=setInterval(()=>window.m2Counter++,100); scrollTo(0,731); scrollY", false)?;
    report.check("m2.scroll-setup", setup == json!(731), format!("{setup}"));
    lane.hide_browser(&first)?;
    lane.place_browser(&second)?;
    let suspended = wait_for(Duration::from_secs(40), || {
        lane.browser_suspended(&first)
            .ok()
            .filter(|suspended| *suspended)
    });
    if !report.check(
        "m2.hidden-suspended",
        suspended == Some(true),
        format!("{suspended:?}"),
    ) {
        return Err("hidden page never suspended".into());
    }
    lane.hide_browser(&second)?;
    lane.place_browser(&first)?;
    report.check(
        "m2.placed-resumed",
        !lane.browser_suspended(&first)?,
        "Resume completed before show",
    );
    let preserved = lane.evaluate(&first, "({scroll:scrollY,counter:window.m2Counter})", false)?;
    report.check(
        "m2.page-state",
        preserved["scroll"] == 731 && preserved["counter"].as_u64().is_some_and(|n| n > 17),
        format!("{preserved}"),
    );
    lane.hide_browser(&first)?;
    lane.place_browser(&second)?;
    let suspended = wait_for(Duration::from_secs(40), || {
        lane.browser_suspended(&first)
            .ok()
            .filter(|suspended| *suspended)
    });
    if !report.check(
        "m2.resuspended",
        suspended == Some(true),
        format!("{suspended:?}"),
    ) {
        return Err("page never resuspended".into());
    }
    let evaluated = lane.evaluate(&first, "window.m2Counter += 7; clearInterval(window.m2Timer); ({scroll:scrollY,counter:window.m2Counter})", false)?;
    report.check(
        "m2.cdp-wakes-hidden",
        evaluated["scroll"] == 731
            && evaluated["counter"]
                .as_u64()
                .zip(preserved["counter"].as_u64())
                .is_some_and(|(after, before)| after >= before + 7)
            && !lane.browser_suspended(&first)?
            && !lane.browser_visible(&first)?,
        format!("{evaluated}"),
    );
    // Keep the capability cached while revoking its real attachment. An
    // unpinned control must actually suspend before negative assertions.
    let agent = lane.agent_attachment(&second)?;
    lane.hide_browser(&second)?;
    lane.place_browser(&first)?;
    lane.hide_browser(&first)?;
    let control = wait_for(Duration::from_secs(40), || {
        lane.browser_suspended(&first)
            .ok()
            .filter(|suspended| *suspended)
    });
    report.check(
        "m2.agent-control-suspended",
        control == Some(true),
        format!("{control:?}"),
    );
    report.check(
        "m2.agent-session-pin",
        !lane.browser_suspended(&second)?,
        "live CDP session kept hidden page awake",
    );
    report.check("m2.take-over", agent.take_over(), "paused Agent control");
    let paused = wait_for(Duration::from_secs(40), || {
        lane.browser_suspended(&second)
            .ok()
            .filter(|suspended| *suspended)
    });
    report.check(
        "m2.user-control-releases-pin",
        paused == Some(true),
        format!("{paused:?}"),
    );
    report.check("m2.hand-back", agent.hand_back(), "resumed Agent control");
    let low = wait_for(Duration::from_secs(5), || {
        (!lane.browser_suspended(&second).ok()? && lane.browser_memory_low(&second).ok()?)
            .then_some(true)
    });
    report.check(
        "m2.hidden-session-wake-low",
        low == Some(true),
        "a hidden session resumes with the native memory target still LOW",
    );
    report.check(
        "m2.detach",
        agent.detach(),
        "revoked the attachment while retaining its native capability",
    );
    let released = wait_for(Duration::from_secs(40), || {
        lane.browser_suspended(&second)
            .ok()
            .filter(|suspended| *suspended)
    });
    report.check(
        "m2.session-release",
        released == Some(true),
        format!("{released:?}"),
    );
    // An inspector pins its own tab. An unpinned control proves the grace
    // actually elapsed, so this cannot pass by keeping every tab awake.
    lane.place_browser(&second)?;
    lane.open_browser_devtools(&second)?;
    crate::report::quiet_period();
    lane.hide_browser(&second)?;
    lane.place_browser(&first)?;
    lane.hide_browser(&first)?;
    let control = wait_for(Duration::from_secs(40), || {
        lane.browser_suspended(&first)
            .ok()
            .filter(|suspended| *suspended)
    });
    report.check(
        "m2.devtools-control-suspended",
        control == Some(true),
        format!("{control:?}"),
    );
    report.check(
        "m2.devtools-pin",
        !lane.browser_suspended(&second)?,
        "an open inspector kept its hidden tab awake",
    );
    drop(agent);
    Ok(())
}
