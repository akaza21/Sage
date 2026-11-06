#[cfg(test)]
use std::fs;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(not(windows))]
use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Webview, WebviewBuilder,
    WebviewUrl,
};
use url::Url;

#[cfg(test)]
use crate::launch_profile::LaunchProfile;

mod admission;
pub(crate) mod annotate;
pub(crate) mod bounds;
pub(crate) mod browsing_history;
pub(crate) mod clear_data;
mod commit;
pub(crate) mod device;
pub(crate) mod devtools;
pub(crate) mod downloads;
mod favicon;
mod history;
pub(crate) mod ipc_guard;
pub(crate) mod lifecycle;
#[cfg(all(windows, feature = "native-lane"))]
pub mod native_lane;
pub(crate) mod navigation;
pub(crate) mod page_context;
pub(crate) mod page_controls;
pub(crate) mod passwords;
pub(crate) mod popups;
mod profile;
mod shortcuts;
pub(crate) mod site_permissions;
mod state_init;
pub(crate) mod state_query;
mod tab_title;
mod teardown;
pub(crate) mod user_screenshot;
mod visibility;

#[cfg(test)]
use admission::MAX_BROWSER_URL_BYTES;
use admission::admit_url;
use history::BrowserHistory;
#[cfg(test)]
use history::MAX_BROWSER_HISTORY;
#[cfg(test)]
mod clear_conformance_tests;
#[cfg(test)]
mod policy_conformance_tests;
use popups::PopupRequests;
#[cfg(test)]
use popups::popup_target;
#[cfg(test)]
use profile::browser_profile_dir;

const MAX_BROWSER_TABS: usize = 8;

pub(crate) struct BrowserState {
    profile_dir: PathBuf,
    tabs: Mutex<BrowserTabs<Webview>>,
    downloads: Mutex<downloads::DownloadRegistry>,
    permissions: Mutex<site_permissions::SitePermissions>,
    popups: Mutex<PopupRequests>,
    annotate: annotate::Annotate,
    clearing_data: AtomicBool,
    browsing_history: browsing_history::BrowsingHistory,
    /// Device toolbar updates apply one at a time across tabs.
    #[cfg_attr(not(windows), allow(dead_code))]
    device_updates: Mutex<()>,
}

impl BrowserState {
    pub(crate) fn webview_for_tab(&self, label: &str) -> Option<Webview> {
        self.tabs.lock().ok()?.handle(label).cloned()
    }

    pub(crate) fn clear_teardown_after_window_destroy(&self) {
        if let Ok(tabs) = self.tabs.lock() {
            tabs.teardown.clear_after_window_destroy();
        }
    }

    pub(crate) fn quarantine_failed_close(&self, label: &str) {
        if let Ok(tabs) = self.tabs.lock()
            && tabs.teardown.begin(label).is_ok()
        {
            tabs.teardown.mark_failed(label);
        }
    }

    #[cfg(windows)]
    pub(crate) fn close_for_shutdown(&self, label: &str) -> Result<(), String> {
        let webview = self
            .webview_for_tab(label)
            .ok_or("browser tab unavailable")?;
        webview.close().map_err(|error| error.to_string())?;
        self.tabs
            .lock()
            .map_err(|error| error.to_string())?
            .entries
            .remove(label);
        Ok(())
    }
}

struct BrowserTab<T> {
    handle: T,
    #[cfg(windows)]
    developer: devtools::developer_session::Slot,
    visibility: Arc<visibility::PageVisibility>,
    loading: bool,
    title: Option<String>,
    favicon: Option<String>,
    zoom_percent: u16,
    device: device::TabDevice,
    navigation_id: Option<u64>,
    revision: u64,
    document_revision: u64,
    committed_url: Option<String>,
    /// The URL of this tab's latest visit queued to the browsing history,
    /// which its later titles and same-document changes name; `None` when
    /// none was queued, as when a full queue dropped it.
    visited_url: Option<String>,
    history: BrowserHistory,
    fallback_url: Option<String>,
    last_error: Option<&'static str>,
    evidence: navigation::NavigationEvidence,
}

#[derive(Clone)]
struct BrowserSnapshot {
    loading: bool,
    title: Option<String>,
    favicon: Option<String>,
    zoom_percent: u16,
    device: device::DeviceHeld,
    navigation_id: Option<u64>,
    revision: u64,
    committed_url: Option<String>,
    error: Option<&'static str>,
    can_go_back: bool,
    can_go_forward: bool,
}

struct BrowserTabs<T> {
    next_number: u64,
    entries: BTreeMap<String, BrowserTab<T>>,
    opening: BTreeSet<String>,
    closing: BTreeSet<String>,
    teardown: lifecycle::TeardownQuarantine,
    browsing_history: browsing_history::BrowsingHistory,
}

impl<T> BrowserTabs<T> {
    fn new() -> Self {
        Self {
            next_number: 1,
            entries: BTreeMap::new(),
            opening: BTreeSet::new(),
            closing: BTreeSet::new(),
            teardown: lifecycle::TeardownQuarantine::default(),
            browsing_history: browsing_history::BrowsingHistory::default(),
        }
    }

    fn reserve_label(&mut self) -> Result<String, String> {
        if self.entries.len() >= MAX_BROWSER_TABS {
            return Err("browser tab limit reached".into());
        }
        let number = self.next_number;
        self.next_number = number
            .checked_add(1)
            .ok_or("browser tab labels exhausted")?;
        Ok(format!("browser-{number}"))
    }

    fn reserve_open_label(&mut self) -> Result<String, String> {
        if self.slot_count() >= MAX_BROWSER_TABS {
            return Err("browser tab limit reached".into());
        }
        let label = self.reserve_label()?;
        self.opening.insert(label.clone());
        Ok(label)
    }

    fn slot_count(&self) -> usize {
        self.entries.len()
            + self
                .opening
                .iter()
                .filter(|label| !self.entries.contains_key(*label))
                .count()
            + self
                .closing
                .iter()
                .filter(|label| {
                    !self.entries.contains_key(*label) && !self.opening.contains(*label)
                })
                .count()
    }

    fn insert(&mut self, label: String, handle: T, loading: bool) {
        self.entries.insert(
            label,
            BrowserTab {
                handle,
                #[cfg(windows)]
                developer: devtools::developer_session::Slot::default(),
                visibility: Arc::default(),
                loading,
                title: None,
                favicon: None,
                zoom_percent: 100,
                device: device::TabDevice::default(),
                navigation_id: None,
                revision: 0,
                document_revision: 0,
                committed_url: None,
                visited_url: None,
                history: BrowserHistory::default(),
                fallback_url: None,
                last_error: None,
                evidence: navigation::NavigationEvidence::default(),
            },
        );
    }

    #[cfg(test)]
    fn contains(&self, label: &str) -> bool {
        self.entries.contains_key(label)
    }

    fn handle(&self, label: &str) -> Option<&T> {
        self.entries.get(label).map(|tab| &tab.handle)
    }

    fn snapshot(&self, label: &str) -> Option<BrowserSnapshot> {
        self.entries.get(label).map(|tab| BrowserSnapshot {
            loading: tab.loading,
            title: tab.title.clone(),
            favicon: tab.favicon.clone(),
            zoom_percent: tab.zoom_percent,
            device: tab.device.held(),
            navigation_id: tab.navigation_id,
            revision: tab.revision,
            committed_url: if tab.last_error.is_none() {
                tab.committed_url.clone()
            } else {
                None
            },
            error: tab.last_error,
            can_go_back: tab.history.bounds().0,
            can_go_forward: tab.history.bounds().1,
        })
    }

    fn openable_url(&self, label: &str) -> Result<String, String> {
        let tab = self.entries.get(label).ok_or("browser tab unavailable")?;
        if tab.loading {
            return Err("browser page is still loading".into());
        }
        if tab.last_error.is_some() {
            return Err("browser page navigation failed".into());
        }
        tab.committed_url
            .clone()
            .ok_or_else(|| "browser page has no committed URL".into())
    }

    fn start_navigation(&mut self, label: &str, navigation_id: u64, url: &str) -> bool {
        if self.entries.get(label).is_none_or(|tab| {
            tab.navigation_id
                .is_some_and(|current| navigation_id <= current)
        }) {
            return false;
        }
        self.begin_evidence(label, navigation_id);
        let Some(tab) = self.entries.get_mut(label) else {
            return false;
        };
        tab.navigation_id = Some(navigation_id);
        tab.history.start(navigation_id, url);
        tab.loading = true;
        tab.title = None;
        tab.favicon = None;
        tab.last_error = None;
        tab.revision = tab.revision.wrapping_add(1);
        tab.document_revision = tab.document_revision.wrapping_add(1);
        true
    }

    fn set_zoom_percent(&mut self, label: &str, percent: u16) -> bool {
        let Some(tab) = self.entries.get_mut(label) else {
            return false;
        };
        tab.zoom_percent = percent;
        tab.revision = tab.revision.wrapping_add(1);
        true
    }

    #[cfg(test)]
    fn history_len(&self, label: &str) -> Option<usize> {
        self.entries.get(label).map(|tab| tab.history.len())
    }

    fn traverse(&mut self, label: &str, direction: i8) -> Result<String, String> {
        let tab = self
            .entries
            .get_mut(label)
            .ok_or("browser tab unavailable")?;
        if tab.loading {
            return Err("browser navigation is busy".into());
        }
        tab.history.traverse(direction)
    }

    fn cancel_traversal(&mut self, label: &str) {
        if let Some(tab) = self.entries.get_mut(label) {
            tab.history.cancel_traversal();
        }
    }

    fn native_history_aligned(&self, label: &str) -> bool {
        self.entries
            .get(label)
            .is_some_and(|tab| tab.history.native_aligned())
    }

    fn mark_native_history_diverged(&mut self, label: &str) {
        if let Some(tab) = self.entries.get_mut(label) {
            tab.history.mark_native_diverged();
        }
    }

    fn take_fallback(&mut self, label: &str) -> Option<String> {
        self.entries.get_mut(label)?.fallback_url.take()
    }

    fn abort_fallback(&mut self, label: &str) {
        if let Some(tab) = self.entries.get_mut(label) {
            tab.history.cancel_traversal();
            tab.loading = false;
            tab.last_error = Some("Back or Forward could not reach the saved page");
            tab.revision = tab.revision.wrapping_add(1);
            tab.document_revision = tab.document_revision.wrapping_add(1);
        }
    }

    #[cfg(test)]
    fn close_with(
        &mut self,
        label: &str,
        close: impl FnOnce(&T) -> Result<(), String>,
    ) -> Result<(), String> {
        let tab = self.entries.get(label).ok_or("browser tab unavailable")?;
        close(&tab.handle)?;
        self.entries.remove(label);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
