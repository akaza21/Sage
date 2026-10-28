//! How a completed native navigation settles a tab.
//!
//! WebView2 reports `IsSuccess = FALSE` for real load failures (DNS, refused
//! connection, certificate errors), for documents that did commit (a 4xx/5xx
//! page, an authentication page, `window.stop()` after commit, a stopped
//! reload) and for navigations that never committed at all (the response
//! became a download, the host or page cancelled it, it was superseded). Its
//! documentation tells hosts to read `WebErrorStatus` alongside `IsSuccess`.
//!
//! What decides is evidence that *this* navigation id committed: WebView2
//! raises `ContentLoading` (with the `NavigationId` and `IsErrorPage`) once a
//! document, or an error page, commits. So:
//! - committed document: `Committed` at the WebView's `Source()`;
//! - committed error page: `Failed`, reported with the status's reason;
//! - no commit, and `OPERATION_CANCELED` or a `DownloadStarting` for this
//!   navigation's own URI or redirect hop: `Abandoned`, the previous page
//!   stays;
//! - no commit otherwise: `Failed`.
//!
//! `Source()` differing from the committed URL is never taken as proof of a
//! commit: the page can change its own hash or history while a navigation is
//! pending.

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
    pub(crate) source: Option<&'a str>,
}

/// What `ContentLoading` said committed for the navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Commit {
    Document,
    ErrorPage,
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
    commit: Option<Commit>,
    became_download: bool,
) -> Outcome<'a> {
    let source = completion.source.filter(|source| admit_url(source).is_ok());
    let reason = failure_reason(completion.web_error);
    if completion.success || commit == Some(Commit::Document) {
        return source.map_or(Outcome::Failed(reason), Outcome::Committed);
    }
    if commit.is_none() && (completion.web_error == WEB_ERROR_OPERATION_CANCELED || became_download)
    {
        return Outcome::Abandoned;
    }
    Outcome::Failed(reason)
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
            source: Some(source),
        }
    }

    #[test]
    fn uncommitted_cancel_or_own_download_is_abandoned() {
        let canceled = failed(WEB_ERROR_OPERATION_CANCELED, PAGE);
        assert_eq!(classify(canceled, None, false), Outcome::Abandoned);
        // Whatever status WebView2 gives a download, the correlation holds.
        assert_eq!(classify(failed(9, PAGE), None, true), Outcome::Abandoned);
        // A changed Source() alone is not a commit.
        let drifted = failed(WEB_ERROR_OPERATION_CANCELED, "https://example.com/page#x");
        assert_eq!(classify(drifted, None, false), Outcome::Abandoned);
    }

    #[test]
    fn failures_and_error_pages_are_reported() {
        assert_eq!(
            classify(failed(13, "https://missing.example/"), None, false),
            Outcome::Failed("The site's address could not be found")
        );
        assert_eq!(
            classify(failed(12, PAGE), Some(Commit::ErrorPage), true),
            Outcome::Failed("The site could not be reached")
        );
        // A cancel after an error page committed is still that failure.
        assert_eq!(
            classify(
                failed(WEB_ERROR_OPERATION_CANCELED, PAGE),
                Some(Commit::ErrorPage),
                false
            ),
            Outcome::Failed("The page could not be loaded")
        );
    }

    #[test]
    fn committed_documents_keep_their_url_whatever_the_status() {
        for web_error in [WEB_ERROR_UNKNOWN, WEB_ERROR_OPERATION_CANCELED, 17, 18] {
            assert_eq!(
                classify(failed(web_error, PAGE), Some(Commit::Document), false),
                Outcome::Committed(PAGE)
            );
        }
        assert_eq!(
            classify(
                failed(WEB_ERROR_UNKNOWN, "file:///C:/secret"),
                Some(Commit::Document),
                false
            ),
            Outcome::Failed("The page could not be loaded")
        );
    }
}
