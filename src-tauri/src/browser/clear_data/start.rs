use std::sync::atomic::AtomicBool;

use super::ClearBrowsingDataResult;
#[cfg(windows)]
use super::begin_clear;

#[cfg_attr(windows, allow(dead_code))]
pub(super) const UNSUPPORTED_PLATFORM: &str = "Clearing browsing data requires Windows WebView2. Nothing was cleared and no tabs were closed.";

pub(super) enum ClearStart {
    #[cfg_attr(windows, allow(dead_code))]
    Refused(ClearBrowsingDataResult),
    Begun(Vec<String>),
}

pub(super) fn start_clear<T>(
    tabs: &std::sync::Mutex<super::super::BrowserTabs<T>>,
    clearing_data: &AtomicBool,
) -> Result<ClearStart, String> {
    #[cfg(not(windows))]
    {
        let _ = (tabs, clearing_data);
        Ok(ClearStart::Refused(ClearBrowsingDataResult {
            closed_tab_ids: Vec::new(),
            cleared: false,
            pending: false,
            error: Some(UNSUPPORTED_PLATFORM.into()),
        }))
    }
    #[cfg(windows)]
    {
        begin_clear(tabs, clearing_data).map(ClearStart::Begun)
    }
}

#[cfg(test)]
mod platform_tests {
    #[cfg(not(windows))]
    use super::super::begin_clear;
    use super::*;
    use std::sync::atomic::Ordering;

    fn one_open_tab() -> std::sync::Mutex<super::super::super::BrowserTabs<usize>> {
        let tabs = std::sync::Mutex::new(super::super::super::BrowserTabs::<usize>::new());
        tabs.lock().unwrap().insert("browser-1".into(), 1, false);
        tabs
    }

    #[cfg(not(windows))]
    #[test]
    fn without_webview2_clear_refuses_before_any_tab_closes_or_access_is_revoked() {
        let tabs = one_open_tab();
        let clearing_data = AtomicBool::new(false);
        let ClearStart::Refused(result) = start_clear(&tabs, &clearing_data).unwrap() else {
            panic!("a platform without WebView2 must refuse before beginning");
        };
        assert!(result.closed_tab_ids.is_empty());
        assert!(!result.cleared);
        assert!(!result.pending);
        assert_eq!(result.error.as_deref(), Some(UNSUPPORTED_PLATFORM));
        assert!(!clearing_data.load(Ordering::Acquire));
        assert_eq!(
            begin_clear(&tabs, &clearing_data).unwrap(),
            vec!["browser-1".to_owned()]
        );
    }

    #[cfg(windows)]
    #[test]
    fn with_webview2_clear_begins_with_every_open_tab() {
        let tabs = one_open_tab();
        let clearing_data = AtomicBool::new(false);
        let ClearStart::Begun(ids) = start_clear(&tabs, &clearing_data).unwrap() else {
            panic!("WebView2 builds must begin the clear");
        };
        assert_eq!(ids, vec!["browser-1".to_owned()]);
        assert!(clearing_data.load(Ordering::Acquire));
    }
}
