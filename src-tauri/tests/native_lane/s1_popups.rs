use std::time::Duration;

use sage_lib::native_lane::{Lane, NativePopup};
use serde_json::{Value, json};

use crate::{
    report::{Report, quiet_period, wait_for},
    s1_javascript,
    site::Site,
};

/// Sage's tab cap (`MAX_BROWSER_TABS`).
pub const TAB_CAP: usize = 8;
const BLOB_MARKER: &str = "/s1/blob-ran";

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("S1 popups keep window.opener as policed Sage tabs");
    let opener_url = site.url("/s1-opener.html");
    let opener = match lane.open_tab(Some(&opener_url), false) {
        Ok(tab) => tab,
        Err(error) => {
            report.check("s1.open", false, error);
            return;
        }
    };
    let settled = wait_for(Duration::from_secs(15), || lane.settled_url(&opener));
    if !report.check(
        "s1.opener-loaded",
        settled.as_deref() == Some(&opener_url),
        format!("{opener} settled on {settled:?}"),
    ) {
        return;
    }
    let popup_url = site.url("/s1-popup.html?report=s1-popup");
    let opened = open(
        lane,
        &opener,
        &format!("window.__open({})", json!(popup_url)),
    );
    popup(
        lane, site, report, &opener, "s1-popup", &popup_url, &popup_url, &opened,
    );
    let target = site.url("/s1-popup.html?report=s1-blank");
    let opened = open(
        lane,
        &opener,
        &format!("window.__openBlank({})", json!(target)),
    );
    popup(
        lane,
        site,
        report,
        &opener,
        "s1-blank",
        "about:blank",
        &target,
        &opened,
    );
    s1_javascript::run(lane, site, report, &opener);
    blob_popup(lane, site, report, &opener);
    for _ in lane.tabs().len()..TAB_CAP {
        if let Err(error) = lane.open_tab(None, false) {
            report.check("s1.fill-cap", false, error);
            return;
        }
    }
    let cap = "/s1-popup.html?report=s1-cap";
    denied(
        lane,
        site,
        report,
        &opener,
        "s1.tab-cap",
        &site.url(cap),
        cap,
    );
}

pub fn open(lane: &Lane, opener: &str, script: &str) -> Result<Value, String> {
    lane.evaluate(opener, script, true)
}

#[allow(clippy::too_many_arguments)]
fn popup(
    lane: &Lane,
    site: &Site,
    report: &mut Report,
    opener: &str,
    name: &str,
    requested: &str,
    target: &str,
    opened: &Result<Value, String>,
) {
    let linked = wait_for(Duration::from_secs(10), || {
        lane.native_popups()
            .into_iter()
            .find(|popup| popup.opener == opener && popup.url == requested)
    });
    let Some(linked) = linked else {
        report.check(
            &format!("{name}.native-tab"),
            false,
            format!(
                "window.open -> {opened:?}; no opener-linked tab for {requested}; live tabs {:?}",
                lane.tabs()
            ),
        );
        return;
    };
    let acknowledged = lane.acknowledge_popup(&linked);
    let child = &linked.child;
    let settled = wait_for(Duration::from_secs(15), || {
        lane.settled_url(child).filter(|url| url == target)
    });
    report.check(
        &format!("{name}.native-tab"),
        child.starts_with("browser-")
            && lane.tabs().contains(child)
            && acknowledged.is_ok()
            && settled.is_some(),
        format!(
            "window.open -> {opened:?}; {child} linked to {opener} (requested {requested}); ack {acknowledged:?}; settled {settled:?}"
        ),
    );
    let seen = wait_for(Duration::from_secs(10), || {
        site.reports(name).into_iter().next()
    });
    report.check(
        &format!("{name}.has-opener"),
        seen.as_ref()
            .is_some_and(|seen| seen["hasOpener"] == true && seen["posted"] == true),
        format!("popup reported {seen:?}"),
    );
    let kind = format!("{name}-hello");
    let message = wait_for(Duration::from_secs(10), || {
        site.reports("s1-message")
            .into_iter()
            .find(|message| message["data"]["kind"] == kind.as_str())
    });
    report.check(
        &format!("{name}.opener-message"),
        message
            .as_ref()
            .is_some_and(|message| message["origin"] == site.origin().as_str()),
        format!("opener received {message:?}"),
    );
}

fn blob_popup(lane: &Lane, site: &Site, report: &mut Report, opener: &str) {
    let abandoned = lane.abandoned_popups();
    let tabs = lane.tabs();
    let popups = lane.native_popups();
    let opened = open(
        lane,
        opener,
        &format!("window.__openBlob({})", json!(BLOB_MARKER)),
    );
    let gone = gone(lane, site, (&tabs, &popups, &opened), BLOB_MARKER);
    let abandons = lane.abandoned_popups().saturating_sub(abandoned);
    report.check(
        "s1.blob-abandoned",
        opened == Ok(json!("assigned")) && abandons == 1 && gone.0,
        format!("{}; Abandon verdicts {abandons}", gone.1),
    );
}

fn denied(
    lane: &Lane,
    site: &Site,
    report: &mut Report,
    opener: &str,
    id: &str,
    url: &str,
    marker: &str,
) {
    let tabs = lane.tabs();
    let popups = lane.native_popups();
    let opened = open(lane, opener, &format!("window.__open({})", json!(url)));
    let (passed, evidence) = gone(lane, site, (&tabs, &popups, &opened), marker);
    report.check(id, passed, evidence);
}

fn gone(
    lane: &Lane,
    site: &Site,
    before: (&[String], &[NativePopup], &Result<Value, String>),
    marker: &str,
) -> (bool, String) {
    let (tabs, popups, opened) = before;
    quiet_period();
    let settled = wait_for(Duration::from_secs(10), || {
        (lane.tabs() == tabs).then_some(())
    });
    let after = lane.tabs();
    let linked: Vec<NativePopup> = lane
        .native_popups()
        .into_iter()
        .filter(|popup| !popups.contains(popup))
        .collect();
    (
        settled.is_some() && linked.is_empty() && site.hits(marker) == 0,
        format!(
            "window.open -> {opened:?}; tabs {tabs:?} -> {after:?}; new linked popups {linked:?}; {marker} fetched {} times",
            site.hits(marker)
        ),
    )
}
