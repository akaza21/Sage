//! How a completed native navigation settles a tab.
//!
//! WebView2 reports `IsSuccess = FALSE` both for real load failures (DNS,
//! refused connection, certificate errors) and for navigations that simply
//! never committed a new document: the response became a download, the host
//! or the page cancelled it, or it was stopped. Its documentation tells hosts
//! to read `WebErrorStatus` alongside `IsSuccess`, naming `OPERATION_CANCELED`
//! as the non-catastrophic status. A navigation is treated as abandoned only
//! when the WebView still shows the previous committed document (`Source()`
//! unchanged) and either WebView2 reports `OPERATION_CANCELED` or this
//! navigation turned into a download (`DownloadStarting` fired for it, which
//! also covers a download Sage denies). Everything else that failed is a
//! failure and is reported as one.

use super::admit_url;

/// `COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN`.
pub(crate) const WEB_ERROR_UNKNOWN: i32 = 0;
/// `COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED`.
pub(crate) const WEB_ERROR_OPERATION_CANCELED: i32 = 14;

/// What `NavigationCompleted` reported, platform-neutral.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Completion<'a> {
    pub(crate) success: bool,
    pub(crate) web_error: i32,
    pub(crate) http_status: i32,
    pub(crate) source: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome<'a> {
    /// A new document committed at this URL.
    Committed(&'a str),
    /// Nothing committed; the previous page is still shown.
    Abandoned,
    /// The load failed; the reason is shown to the user.
    Failed(&'static str),
}

/// How a tab settled, for the native handler's follow-up work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    Committed,
    Abandoned,
    Failed,
}

pub(crate) fn classify<'a>(
    completion: Completion<'a>,
    previous: Option<&str>,
    became_download: bool,
) -> Outcome<'a> {
    let source = completion.source.filter(|source| admit_url(source).is_ok());
    if completion.success {
        return source.map_or(Outcome::Failed(failure_reason(WEB_ERROR_UNKNOWN)), |url| {
            Outcome::Committed(url)
        });
    }
    let unchanged = source == previous;
    let canceled = completion.web_error == WEB_ERROR_OPERATION_CANCELED;
    if unchanged && (canceled || became_download) {
        return Outcome::Abandoned;
    }
    match source {
        // The server answered with its own error document (4xx/5xx), or the
        // load was stopped after a new document had already committed.
        Some(url)
            if (completion.web_error == WEB_ERROR_UNKNOWN && completion.http_status >= 400)
                || (canceled && !unchanged) =>
        {
            Outcome::Committed(url)
        }
        _ => Outcome::Failed(failure_reason(completion.web_error)),
    }
}

fn failure_reason(web_error: i32) -> &'static str {
    match web_error {
        1..=5 => "The site's security certificate is not trusted",
        6 | 9..=12 => "The site could not be reached",
        7 => "The site took too long to respond",
        13 => "The site's address could not be found",
        15 => "The site redirected too many times",
        17 | 18 => "The site requires authentication",
        _ => "The page could not be loaded",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "https://example.com/page";

    fn failed(web_error: i32, source: &str) -> Completion<'_> {
        Completion {
            success: false,
            web_error,
            http_status: 0,
            source: Some(source),
        }
    }

    #[test]
    fn download_or_cancel_on_the_unchanged_page_is_abandoned() {
        assert_eq!(
            classify(
                failed(WEB_ERROR_OPERATION_CANCELED, PAGE),
                Some(PAGE),
                false
            ),
            Outcome::Abandoned
        );
        // Whatever status WebView2 gives a download, the correlation holds.
        assert_eq!(
            classify(failed(9, PAGE), Some(PAGE), true),
            Outcome::Abandoned
        );
        // A fresh tab still on its inert blank page.
        assert_eq!(
            classify(
                failed(WEB_ERROR_OPERATION_CANCELED, "about:blank"),
                None,
                false
            ),
            Outcome::Abandoned
        );
    }

    #[test]
    fn real_failures_are_reported_even_on_the_same_url() {
        assert_eq!(
            classify(failed(13, "https://missing.example/"), Some(PAGE), false),
            Outcome::Failed("The site's address could not be found")
        );
        assert_eq!(
            classify(failed(12, PAGE), Some(PAGE), false),
            Outcome::Failed("The site could not be reached")
        );
        // A download marker never hides a failure that left the page.
        assert_eq!(
            classify(failed(12, "https://down.example/"), Some(PAGE), true),
            Outcome::Failed("The site could not be reached")
        );
    }

    #[test]
    fn server_error_documents_and_stopped_commits_keep_their_url() {
        let not_found = Completion {
            http_status: 404,
            ..failed(WEB_ERROR_UNKNOWN, "https://example.com/missing")
        };
        assert_eq!(
            classify(not_found, Some(PAGE), false),
            Outcome::Committed("https://example.com/missing")
        );
        assert_eq!(
            classify(
                failed(WEB_ERROR_OPERATION_CANCELED, "https://next.example/"),
                Some(PAGE),
                false
            ),
            Outcome::Committed("https://next.example/")
        );
        assert_eq!(
            classify(
                failed(WEB_ERROR_OPERATION_CANCELED, "file:///C:/secret"),
                Some(PAGE),
                false
            ),
            Outcome::Failed("The page could not be loaded")
        );
    }
}
