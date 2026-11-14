//! WebView2 reports `window.open('javascript:…')` as an `about:blank` request
//! and raises no `NavigationStarting` for the `javascript:` URL, so the payload
//! runs in the blank child, which has the opener's origin. Chrome does the
//! same, and the opener could run that script itself, so it is contained
//! rather than denied.

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
    // Exactly one child, a Sage tab; anything else fails every check below
    // rather than skipping it.
    let [child] = children.as_slice() else {
        let why = format!(
            "one window.open made {} children {children:?} (want exactly 1); window.open -> {opened:?}; opener __jsRan {in_opener:?}; new linked popups {linked:?}",
            children.len()
        );
        for id in CHECKS {
            report.check(id, false, &why);
        }
        close(lane, &children);
        return;
    };
    let state = wait_for(Duration::from_secs(5), || {
        ran(child).ok().filter(|state| {
            !state["fetch"]
                .as_object()
                .is_some_and(|fetch| fetch.values().any(|outcome| outcome == "pending"))
        })
    })
    .map_or_else(|| ran(child), Ok);
    let link = linked.iter().find(|popup| &popup.child == child);
    let adopted = link.map(|link| lane.acknowledge_popup(link));
    let contained = state.as_ref().is_ok_and(|state| {
        state["ran"] == "number"
            && state["href"] == "about:blank"
            && state["origin"] == site.origin().as_str()
    }) && linked.len() == 1
        && link.is_some_and(|link| link.opener == opener && link.url == "about:blank")
        && matches!(adopted, Some(Ok(())))
        && lane.tabs().contains(child);
    report.check(
        CHECKS[0],
        contained && in_opener == Ok(json!("undefined")),
        format!(
            "window.open -> {opened:?}; child {child} (tabs {} -> {}); pending entry {link:?}; chrome ack {adopted:?}; child state {state:?}; opener __jsRan {in_opener:?}; {RELATIVE_MARKER} fetched {} times, {ABSOLUTE_MARKER} {} times; Abandon verdicts +{}",
            tabs.len(),
            tabs.len() + children.len(),
            site.hits(RELATIVE_MARKER),
            site.hits(ABSOLUTE_MARKER),
            lane.abandoned_popups().saturating_sub(abandoned)
        ),
    );
    let before_ipc = lane.tabs();
    let probes = ipc(lane, site, child);
    navigations(lane, site, report, child);
    cap(lane, report, child);
    // Backstop for unanswered paths: long after the probes, the command still
    // has not run and the tab list is unchanged.
    let after = lane.tabs();
    report.check(
        CHECKS[1],
        probes.0 && site.hits(COMMAND_MARKER) == 0 && after == before_ipc,
        format!(
            "{}; after navigation and cap steps: {COMMAND_MARKER} fetched {} times, tabs {before_ipc:?} -> {after:?}",
            probes.1,
            site.hits(COMMAND_MARKER)
        ),
    );
    close(lane, &children);
}

const CHECKS: [&str; 4] = [
    "s1.javascript-contained",
    "s1.javascript-contained.ipc",
    "s1.javascript-contained.navigation",
    "s1.javascript-contained.cap",
];

fn cap(lane: &Lane, report: &mut Report, child: &str) {
    let live = lane.tabs();
    let mut filler = Vec::new();
    let refusal = loop {
        match lane.open_tab(None, false) {
            Ok(tab) if filler.len() <= TAB_CAP => filler.push(tab),
            Ok(tab) => {
                filler.push(tab);
                break None;
            }
            Err(error) => break Some(error),
        }
    };
    let room = TAB_CAP.saturating_sub(live.len());
    close(lane, &filler);
    report.check(
        CHECKS[3],
        live.contains(&child.to_owned()) && filler.len() == room && refusal.is_some(),
        format!(
            "with {child} alive: {} live tabs {live:?}, {} more opened (want {room}, one fewer than without the child), then {refusal:?}",
            live.len(),
            filler.len()
        ),
    );
}

/// An unanswered raw message counts as refused; `run` backstops it.
fn ipc(lane: &Lane, site: &Site, child: &str) -> (bool, String) {
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
    quiet_period();
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
    (
        all_refused && site.hits(COMMAND_MARKER) == 0 && lane.tabs() == tabs,
        format!(
            "probe {started:?}; result {result:?}; after 6 s quiet: {COMMAND_MARKER} fetched {} times, tabs {tabs:?} -> {:?}",
            site.hits(COMMAND_MARKER),
            lane.tabs()
        ),
    )
}

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
        CHECKS[2],
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
