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
//! - committed error page: `Failed`, reported with the status's reason
//!   (Chromium commits an error page for real load failures: DNS, refused
//!   connection, certificate);
//! - no commit at all: `Abandoned`, the previous page stays. Nothing on
//!   screen changed, whatever `WebErrorStatus` says: on real WebView2 a
//!   navigation that became a download, and a Stop during a slow load,
//!   report a non-`OPERATION_CANCELED` status and no `ContentLoading`.
//!
//! The settle step still refuses an abandon whose `Source()` is not the
//! page the tab will report (`settle::abandon_consistent`).
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
    /// This navigation id committed (`IsSuccess`, or its own
    /// `ContentLoading`).
    Committed,
    /// The tab now reports `Source()` as committed, but the commit was not
    /// this navigation's: a superseded navigation's (`page_changed`) or the
    /// abandon invariant's. Never evidence that a user navigation landed.
    Adopted,
    Abandoned,
    Failed,
}

impl Settled {
    /// Whether this navigation id itself committed a document; the only
    /// outcome that may count as a completed user navigation.
    pub(crate) fn own_commit(settled: Option<Self>) -> bool {
        settled == Some(Self::Committed)
    }

    /// Whether a new document is on screen at the reported URL, so its
    /// title and favicon should be captured.
    pub(crate) fn shows_new_document(settled: Option<Self>) -> bool {
        matches!(settled, Some(Self::Committed | Self::Adopted))
    }
}

pub(crate) fn classify<'a>(completion: Completion<'a>, commit: Option<Commit>) -> Outcome<'a> {
    let source = completion.source.filter(|source| admit_url(source).is_ok());
    let reason = failure_reason(completion.web_error);
    if completion.success || commit == Some(Commit::Document) {
        return source.map_or(Outcome::Failed(reason), Outcome::Committed);
    }
    match commit {
        Some(_) => Outcome::Failed(reason),
        None => Outcome::Abandoned,
    }
}

pub(crate) fn failure_reason(web_error: i32) -> &'static str {
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
    fn nothing_committed_is_abandoned_whatever_the_status() {
        // Real WebView2: a download and a Stop report statuses other than
        // OPERATION_CANCELED (BP-H5b native run); neither committed anything.
        for web_error in [
            WEB_ERROR_UNKNOWN,
            9,
            11,
            12,
            WEB_ERROR_OPERATION_CANCELED,
            16,
        ] {
            assert_eq!(
                classify(failed(web_error, PAGE), None),
                Outcome::Abandoned,
                "status {web_error}"
            );
        }
        // A changed Source() alone is not a commit.
        let drifted = failed(WEB_ERROR_OPERATION_CANCELED, "https://example.com/page#x");
        assert_eq!(classify(drifted, None), Outcome::Abandoned);
    }

    #[test]
    fn committed_error_pages_are_reported() {
        assert_eq!(
            classify(
                failed(13, "https://missing.example/"),
                Some(Commit::ErrorPage)
            ),
            Outcome::Failed("The site's address could not be found")
        );
        assert_eq!(
            classify(failed(12, PAGE), Some(Commit::ErrorPage)),
            Outcome::Failed("The site could not be reached")
        );
        // A cancel after an error page committed is still that failure.
        assert_eq!(
            classify(
                failed(WEB_ERROR_OPERATION_CANCELED, PAGE),
                Some(Commit::ErrorPage)
            ),
            Outcome::Failed("The page could not be loaded")
        );
    }

    #[test]
    fn committed_documents_keep_their_url_whatever_the_status() {
        for web_error in [WEB_ERROR_UNKNOWN, WEB_ERROR_OPERATION_CANCELED, 17, 18] {
            assert_eq!(
                classify(failed(web_error, PAGE), Some(Commit::Document)),
                Outcome::Committed(PAGE)
            );
        }
        assert_eq!(
            classify(
                failed(WEB_ERROR_UNKNOWN, "file:///C:/secret"),
                Some(Commit::Document)
            ),
            Outcome::Failed("The page could not be loaded")
        );
    }
}
