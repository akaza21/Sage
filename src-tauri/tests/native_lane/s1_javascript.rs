//! S1 `window.open('javascript:…')`: contained, not denied. WebView2 reports
//! it as an `about:blank` request and raises no `NavigationStarting` for the
//! `javascript:` URL, so the payload runs in the blank child, which has the
//! opener's origin. Chrome does the same, and the opener could run that
//! script itself (`window.open('')` then `eval` or `document.write`). What
//! must hold:
//!
//! - the child has the opener's origin, so it gains nothing;
//! - it has no Sage IPC: the S3 probe set, run inside it, is refused and
//!   no command runs;
//! - its next navigations are policed: HTTP(S) settles through Sage's
//!   hooks, and a refused scheme leaves it where it was;
//! - there is at most one child, and the chrome can see, adopt and close
//!   it, because it is a Sage tab (counted against the cap) listed in
//!   `browser_popup_pending`.
//!
//! The evidence always shows whether the payload ran and what its fetches
//! did.

use std::time::Duration;

use sage_lib::native_lane::{Lane, NativePopup};
use serde_json::{Value, json};

use crate::{
    report::{Report, quiet_period, wait_for},
    s1_popups::{TAB_CAP, open},
    s3_ipc::refused,
    site::Site,
};

const RELATIVE_MARKER: &str = "/s1/js-ran";
const ABSOLUTE_MARKER: &str = "/s1/js-ran-absolute";
const COMMAND_MARKER: &str = "/s1/js-command-ran";
const NEXT_BLOB_MARKER: &str = "/s1/js-next-blob-ran";
const PROBE: &str = include_str!("../../testdata/native-lane/ipc-probe.js");

pub fn run(lane: &Lane, site: &Site, report: &mut Report, opener: &str) {
    let tabs = lane.tabs();
    let popups = lane.native_popups();
    let abandoned = lane.abandoned_popups();
    // The payload records that it ran, its base URL, and how each fetch
    // (relative and absolute) ended, so a missing fixture hit is explained.
    let payload = format!(
        "javascript:void((window.__jsRan=1),(window.__jsBase=document.baseURI),(window.__jsFetch={{}}),[['relative','{RELATIVE_MARKER}'],['absolute','{}']].forEach(([key,url])=>{{window.__jsFetch[key]='pending';fetch(url,{{method:'POST'}}).then((response)=>window.__jsFetch[key]='status '+response.status,(error)=>window.__jsFetch[key]='error '+error)}}))",
        site.url(ABSOLUTE_MARKER)
    );
    let opened = open(lane, opener, &format!("window.__open({})", json!(payload)));
    quiet_period();
    let children: Vec<String> = lane
        .tabs()
        .into_iter()
        .filter(|tab| !tabs.contains(tab))
        .collect();
    let linked: Vec<NativePopup> = lane
        .native_popups()
        .into_iter()
        .filter(|popup| !popups.contains(popup))
        .collect();
    let in_opener = lane.evaluate(opener, "typeof window.__jsRan", false);
    let ran = |child: &str| {
        lane.evaluate(
            child,
            "JSON.stringify({ran: typeof window.__jsRan, base: window.__jsBase, fetch: window.__jsFetch, origin: self.origin, locationOrigin: location.origin, href: location.href})",
            false,
        )
        .and_then(|text| serde_json::from_str::<Value>(text.as_str().unwrap_or("null")).map_err(|error| error.to_string()))
    };
    let child = match children.as_slice() {
        [] => None,
        [child] => Some(child.as_str()),
        _ => {
            report.check(
                "s1.javascript-contained",
                false,
                format!(
                    "one window.open made {} children: {children:?}",
                    children.len()
                ),
            );
            close(lane, &children);
            return;
        }
    };
    let state = child.map(|child| {
        // Fetches may still be in flight; give them a few seconds to settle.
        wait_for(Duration::from_secs(5), || {
            ran(child).ok().filter(|state| {
                !state["fetch"]
                    .as_object()
                    .is_some_and(|fetch| fetch.values().any(|outcome| outcome == "pending"))
            })
        })
        .map_or_else(|| ran(child), Ok)
    });
    let link = child.and_then(|child| linked.iter().find(|popup| popup.child == child));
    let adopted = link.map(|link| lane.acknowledge_popup(link));
    let contained = match (child, &state) {
        (None, _) => linked.is_empty(),
        (Some(child), Some(Ok(state))) => {
            linked.len() == 1
                && link.is_some_and(|link| link.opener == opener && link.url == "about:blank")
                && matches!(adopted, Some(Ok(())))
                && lane.tabs().iter().any(|tab| tab == child)
                && state["href"] == "about:blank"
                && state["origin"] == site.origin().as_str()
        }
        _ => false,
    };
    report.check(
        "s1.javascript-contained",
        contained && in_opener == Ok(json!("undefined")),
        format!(
            "window.open -> {opened:?}; child {child:?} (tabs {} -> {}, cap {TAB_CAP}); pending entry {link:?}; chrome ack {adopted:?}; child state {state:?}; opener __jsRan {in_opener:?}; {RELATIVE_MARKER} fetched {} times, {ABSOLUTE_MARKER} {} times; Abandon verdicts +{}",
            tabs.len(),
            tabs.len() + children.len(),
            site.hits(RELATIVE_MARKER),
            site.hits(ABSOLUTE_MARKER),
            lane.abandoned_popups().saturating_sub(abandoned)
        ),
    );
    if let Some(child) = child {
        ipc(lane, site, report, child);
        navigations(lane, site, report, child);
        close(lane, &children);
    }
}

/// The S3 probe set, from inside the child: every path refused or absent,
/// and `browser_open` of the marker never runs.
fn ipc(lane: &Lane, site: &Site, report: &mut Report, child: &str) {
    let tabs = lane.tabs();
    let started = lane.evaluate(
        child,
        &format!(
            "{PROBE}\nwindow.__ipcProbe({})",
            json!(site.url(COMMAND_MARKER))
        ),
        false,
    );
    let result = wait_for(Duration::from_secs(30), || {
        lane.evaluate(child, "window.__ipcResult", false)
            .ok()
            .filter(|result| !result.is_null())
    });
    quiet_period();
    let probes = result
        .as_ref()
        .and_then(|result| result["probes"].as_array().cloned())
        .unwrap_or_default();
    let all_refused = probes.len() == 7
        && probes.iter().all(|probe| {
            refused(
                probe["path"].as_str().unwrap_or("?"),
                probe["outcome"].as_str().unwrap_or("?"),
            )
        });
    report.check(
        "s1.javascript-contained.ipc",
        all_refused && site.hits(COMMAND_MARKER) == 0 && lane.tabs() == tabs,
        format!(
            "probe {started:?}; result {result:?}; {COMMAND_MARKER} fetched {} times; tabs {tabs:?} -> {:?}",
            site.hits(COMMAND_MARKER),
            lane.tabs()
        ),
    );
}

/// The child's next navigations: an HTTP(S) page settles through Sage's
/// hooks, then a `blob:` page (a scheme Sage refuses) leaves it there and
/// never reports.
fn navigations(lane: &Lane, site: &Site, report: &mut Report, child: &str) {
    let target = site.url("/s1-popup.html?report=s1-js-next");
    let assigned = lane.evaluate(child, &format!("location.href = {}", json!(target)), false);
    let settled = wait_for(Duration::from_secs(15), || {
        lane.settled_url(child).filter(|url| *url == target)
    });
    let page = format!(
        "<script>fetch({}, {{method: 'POST'}})</script>",
        json!(site.url(NEXT_BLOB_MARKER))
    );
    let blob = lane.evaluate(
        child,
        &format!(
            "location.href = URL.createObjectURL(new Blob([{}], {{type: 'text/html'}}))",
            json!(page)
        ),
        false,
    );
    quiet_period();
    let stayed = lane.settled_url(child);
    let href = lane.evaluate(child, "location.href", false);
    report.check(
        "s1.javascript-contained.navigation",
        settled.is_some()
            && stayed.as_deref() == Some(&target)
            && href == Ok(json!(target))
            && site.hits(NEXT_BLOB_MARKER) == 0,
        format!(
            "http(s): assign {assigned:?}, settled {settled:?}; blob: assign {blob:?}, then settled {stayed:?}, location {href:?}, {NEXT_BLOB_MARKER} fetched {} times",
            site.hits(NEXT_BLOB_MARKER)
        ),
    );
}

fn close(lane: &Lane, children: &[String]) {
    for child in children {
        if let Err(error) = lane.close_tab(child) {
            println!("  (closing {child} failed: {error})");
        }
    }
}
