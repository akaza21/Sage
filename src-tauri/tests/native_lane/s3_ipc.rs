//! `browser_open` of the marker URL would add a tab and fetch the marker from
//! the fixture, so the marker shows whether a refused call still ran.

use std::time::Duration;

use sage_lib::native_lane::Lane;
use serde_json::Value;

use crate::{
    report::{Report, quiet_period, wait_for},
    site::Site,
};

const MARKER: &str = "/s3/command-ran";

pub fn refused(path: &str, outcome: &str) -> bool {
    if path.starts_with("invoke ") {
        matches!(outcome, "rejected" | "no-bridge")
    } else {
        // Raw messages may also be dropped unanswered, or the path absent.
        matches!(outcome, "rejected" | "unanswered" | "absent")
    }
}

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("S3 page IPC from a remote browser child (BF-6c)");
    let probe = site.url("/s3-probe.html");
    let tab = match lane.open_tab(Some(&probe), false) {
        Ok(tab) => tab,
        Err(error) => {
            report.check("s3.open", false, format!("browser_open failed: {error}"));
            return;
        }
    };
    report.check(
        "s3.child-label",
        tab.starts_with("browser-"),
        format!("probe page opened in {tab}"),
    );
    let settled = wait_for(Duration::from_secs(15), || lane.settled_url(&tab));
    report.check(
        "s3.page-loaded",
        settled.as_deref() == Some(probe.as_str()),
        format!("settled on {settled:?}"),
    );
    let posted = wait_for(Duration::from_secs(45), || {
        site.reports("s3").into_iter().next()
    });
    let Some(posted) = posted else {
        report.check("s3.report", false, "the page never posted its results");
        return;
    };
    report.check(
        "s3.bridge",
        true,
        format!(
            "typeof window.__TAURI_INTERNALS__ = {} (Tauri injects its bridge into every webview; the guard, not its absence, must refuse)",
            posted["bridge"]
        ),
    );
    let probes = posted["probes"].as_array().cloned().unwrap_or_default();
    report.check(
        "s3.probes",
        probes.len() == 7,
        format!("{} of 7 IPC paths reported", probes.len()),
    );
    for probe in &probes {
        let path = probe["path"].as_str().unwrap_or("?");
        let outcome = probe["outcome"].as_str().unwrap_or("?");
        report.check(
            &format!("s3.refused {path}"),
            refused(path, outcome),
            format!("{outcome}: {}", text(&probe["detail"])),
        );
    }
    quiet_period();
    let tabs = lane.tabs();
    report.check(
        "s3.no-command-ran",
        site.hits(MARKER) == 0 && tabs == [tab.clone()],
        format!(
            "marker fetched {} times; live tabs {tabs:?}",
            site.hits(MARKER)
        ),
    );
    // Control: the same command from the trusted side does open the marker,
    // so the zero above is an observation, not a blind spot.
    let control = lane.open_tab(Some(&site.url(MARKER)), false);
    let seen = wait_for(Duration::from_secs(15), || {
        (site.hits(MARKER) > 0).then_some(())
    });
    report.check(
        "s3.control",
        control.is_ok() && seen.is_some(),
        format!(
            "trusted browser_open -> {control:?}; marker fetched {} times",
            site.hits(MARKER)
        ),
    );
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}
