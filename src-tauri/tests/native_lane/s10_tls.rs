//! Sage installs no `ServerCertificateErrorDetected` handler, so WebView2's
//! default (deny) applies; the source check keeps it that way.

use std::{sync::atomic::Ordering, time::Duration};

use sage_lib::{acp::browser::BrowserCommand, native_lane::Lane};

use crate::{
    report::{Report, quiet_period, sage_sources, wait_for},
    site::Site,
    tls::{SECRET, TlsSite},
};

const BYPASSES: &[&str] = &[
    "ServerCertificateErrorDetected",
    "SERVER_CERTIFICATE_ERROR_ACTION",
    "ignore-certificate-errors",
    "allow-insecure-localhost",
    "additionalBrowserArgs",
    "additional_browser_args",
];
const FIRST_ATTEMPT: &str = "s10.user-open";

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("S10 TLS: an untrusted certificate never shows content");
    source_has_no_bypass(report);
    let tls = match TlsSite::start() {
        Ok(tls) => tls,
        Err(error) => {
            report.check("s10.fixture", false, error);
            return;
        }
    };
    let target = tls.url("/secret.html");
    let control = tls.fetch_pinned("/secret.html");
    report.check(
        "s10.control",
        control.as_ref().is_ok_and(|page| page.contains(SECRET))
            && tls.counts.requests.load(Ordering::SeqCst) == 1,
        format!(
            "a client pinning the certificate fetched the page: {}",
            control.map_or_else(|error| error, |page| page.len().to_string() + " bytes")
        ),
    );
    for agent in [false, true] {
        let id = if agent {
            "s10.agent-open-tab"
        } else {
            "s10.user-open"
        };
        let before = tls.counts.connections.load(Ordering::SeqCst);
        match lane.open_tab(Some(&target), agent) {
            Ok(tab) => {
                let status = wait_for(Duration::from_secs(20), || {
                    lane.status(&tab).filter(|status| !status.loading)
                });
                refused(lane, report, &tls, id, &tab, &format!("{status:?}"), before);
            }
            Err(error) => {
                report.check(id, false, format!("open failed before WebView2: {error}"));
            }
        }
    }
    let plain = site.url("/plain.html");
    let Ok(tab) = lane.open_tab(Some(&plain), false) else {
        report.check("s10.plain-tab", false, "could not open the plain page");
        return;
    };
    if wait_for(Duration::from_secs(15), || lane.settled_url(&tab)).as_deref() != Some(&plain) {
        report.check("s10.plain-tab", false, "the plain page never settled");
        return;
    }
    let before = tls.counts.connections.load(Ordering::SeqCst);
    let navigated = lane.navigate(&tab, &target);
    let status = wait_for(Duration::from_secs(20), || {
        lane.status(&tab)
            .filter(|status| !status.loading && status.committed_url.as_deref() != Some(&plain))
    });
    refused(
        lane,
        report,
        &tls,
        "s10.user-navigate",
        &tab,
        &format!(
            "navigate -> {navigated:?}; {status:?}; native events: {:?}",
            lane.navigation_trace(&tab)
        ),
        before,
    );
    let back = lane.navigate(&tab, &plain).ok().and_then(|()| {
        wait_for(Duration::from_secs(15), || {
            lane.settled_url(&tab).filter(|url| *url == plain)
        })
    });
    if back.is_none() {
        report.check(
            "s10.agent-navigate",
            false,
            "the tab never returned to the plain page",
        );
        return;
    }
    let before = tls.counts.connections.load(Ordering::SeqCst);
    let outcome = lane.agent(&tab).and_then(|mut agent| {
        agent
            .perform(BrowserCommand::Navigate {
                url: target.clone(),
            })
            .map_err(|failure| format!("{failure:?}"))
    });
    report.check(
        "s10.agent-navigate-fails",
        outcome.is_err(),
        format!("Navigate -> {outcome:?}"),
    );
    refused(
        lane,
        report,
        &tls,
        "s10.agent-navigate",
        &tab,
        "after Navigate",
        before,
    );
}

/// On the first attempt WebView2 must also have connected, so the refusal was
/// observed rather than assumed; later attempts may reuse Chromium's verdict.
fn refused(
    lane: &Lane,
    report: &mut Report,
    tls: &TlsSite,
    id: &str,
    tab: &str,
    observed: &str,
    connections_before: u32,
) {
    quiet_period();
    let committed = lane.settled_url(tab);
    let shown = lane.evaluate(
        tab,
        &format!("(document.documentElement?.innerText ?? '').includes('{SECRET}')"),
        false,
    );
    let connections = tls.counts.connections.load(Ordering::SeqCst) - connections_before;
    let handshakes = tls.counts.handshakes.load(Ordering::SeqCst);
    // The pinned control made the one permitted request.
    let requests = tls.counts.requests.load(Ordering::SeqCst).saturating_sub(1);
    report.check(
        id,
        committed
            .as_deref()
            .is_none_or(|url| !url.starts_with("https://"))
            && !matches!(shown, Ok(serde_json::Value::Bool(true)))
            && requests == 0
            && (connections > 0 || id != FIRST_ATTEMPT),
        format!(
            "{observed}; settled {committed:?}; secret shown {shown:?}; TLS connections {connections}, handshakes {handshakes} (1 is the control), browser requests {requests}"
        ),
    );
}

fn source_has_no_bypass(report: &mut Report) {
    let files = sage_sources();
    let mut found = Vec::new();
    for (file, source) in &files {
        for pattern in BYPASSES {
            if source.contains(pattern) {
                found.push(format!("{} contains {pattern}", file.display()));
            }
        }
    }
    let arguments = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
    if arguments.contains("certificate") || arguments.contains("insecure") {
        found.push(format!("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS={arguments}"));
    }
    report.check(
        "s10.no-certificate-handler",
        found.is_empty() && files.len() > 100,
        if found.is_empty() {
            format!(
                "{} source and config files scanned: no ServerCertificateErrorDetected handler or certificate bypass, so WebView2 denies by default",
                files.len()
            )
        } else {
            found.join("; ")
        },
    );
}
