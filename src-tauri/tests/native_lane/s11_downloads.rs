//! BP-H5b: a link whose response becomes a download must leave the tab on
//! the page it was on (URL, settled state, no New tab), and a real load
//! failure must still be reported. The tab is opened with Agent provenance,
//! so Sage denies the download in `begin()` and no save picker appears; the
//! denied download is itself one of the cases the fix covers.

use std::time::Duration;

use sage_lib::native_lane::Lane;

use crate::{
    report::{Report, quiet_period, wait_for},
    site::{DOWNLOAD, SLOW, Site},
};

const CLICK_DOWNLOAD: &str = "(() => { const link = document.createElement('a'); \
     link.href = '/download.bin'; link.textContent = 'download'; \
     document.body.append(link); link.click(); return true; })()";

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("BP-H5b a download keeps the tab on its page");
    let page = site.url("/plain.html");
    let tab = match lane.open_tab(Some(&page), true) {
        Ok(tab) => tab,
        Err(error) => {
            report.check("s11.open", false, format!("browser_open failed: {error}"));
            return;
        }
    };
    let settled = wait_for(Duration::from_secs(15), || lane.settled_url(&tab));
    if !report.check(
        "s11.page-loaded",
        settled.as_deref() == Some(page.as_str()),
        format!("settled on {settled:?}"),
    ) {
        return;
    }
    let clicked = lane.evaluate(&tab, CLICK_DOWNLOAD, true);
    let fetched = wait_for(Duration::from_secs(15), || {
        (site.hits(DOWNLOAD) > 0).then_some(())
    });
    report.check(
        "s11.download-requested",
        clicked.is_ok() && fetched.is_some(),
        format!(
            "click -> {clicked:?}; {DOWNLOAD} fetched {} times",
            site.hits(DOWNLOAD)
        ),
    );
    quiet_period();
    let status = wait_for(Duration::from_secs(15), || {
        lane.status(&tab).filter(|status| !status.loading)
    });
    report.check(
        "s11.page-kept",
        status.as_ref().is_some_and(|status| {
            status.committed_url.as_deref() == Some(page.as_str()) && status.error.is_none()
        }),
        format!(
            "after the download: {:?}",
            status.map(|status| (status.committed_url, status.error))
        ),
    );
    let settled = lane.settled_url(&tab);
    report.check(
        "s11.still-settled",
        settled.as_deref() == Some(page.as_str()),
        format!("settled on {settled:?}"),
    );
    report.check(
        "s11.download-trace",
        true,
        format!("native events: {:?}", lane.navigation_trace(&tab)),
    );

    // Stop during a slow load keeps the page as well.
    let navigated = lane.navigate(&tab, &site.url(SLOW));
    let requested = wait_for(Duration::from_secs(15), || {
        (site.hits(SLOW) > 0).then_some(())
    });
    let stopped = lane.stop(&tab);
    let status = wait_for(Duration::from_secs(15), || {
        lane.status(&tab).filter(|status| !status.loading)
    });
    report.check(
        "s11.stop-keeps-page",
        navigated.is_ok()
            && requested.is_some()
            && stopped.is_ok()
            && status.as_ref().is_some_and(|status| {
                status.committed_url.as_deref() == Some(page.as_str()) && status.error.is_none()
            }),
        format!(
            "navigate -> {navigated:?}; stop -> {stopped:?}; {:?}; native events: {:?}",
            status.map(|status| (status.committed_url, status.error)),
            lane.navigation_trace(&tab)
        ),
    );

    // Stop during a slow load to another origin (localhost vs 127.0.0.1):
    // the abandon must still keep this origin's page.
    let other = site.url(SLOW).replacen("127.0.0.1", "localhost", 1);
    let hits = site.hits(SLOW);
    let navigated = lane.navigate(&tab, &other);
    let requested = wait_for(Duration::from_secs(15), || {
        (site.hits(SLOW) > hits).then_some(())
    });
    let stopped = lane.stop(&tab);
    let status = wait_for(Duration::from_secs(15), || {
        lane.status(&tab).filter(|status| !status.loading)
    });
    report.check(
        "s11.stop-cross-origin-keeps-page",
        other.contains("localhost")
            && navigated.is_ok()
            && requested.is_some()
            && stopped.is_ok()
            && status.as_ref().is_some_and(|status| {
                status.committed_url.as_deref() == Some(page.as_str()) && status.error.is_none()
            })
            && lane.settled_url(&tab).as_deref() == Some(page.as_str()),
        format!(
            "{other}: navigate -> {navigated:?}; stop -> {stopped:?}; {:?}; native events: {:?}",
            status.map(|status| (status.committed_url, status.error)),
            lane.navigation_trace(&tab)
        ),
    );

    // Control: a refused connection is a real failure and is reported.
    let refused = "http://127.0.0.1:1/";
    let navigated = lane.navigate(&tab, refused);
    let failed = wait_for(Duration::from_secs(20), || {
        lane.status(&tab)
            .filter(|status| !status.loading && status.error.is_some())
    });
    report.check(
        "s11.failure-reported",
        navigated.is_ok()
            && failed
                .as_ref()
                .is_some_and(|status| status.committed_url.is_none()),
        format!(
            "navigate -> {navigated:?}; {:?}; native events: {:?}",
            failed.map(|status| (status.committed_url, status.error)),
            lane.navigation_trace(&tab)
        ),
    );
}
