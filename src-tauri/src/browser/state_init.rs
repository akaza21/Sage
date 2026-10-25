use std::{
    path::PathBuf,
    sync::{Mutex, atomic::AtomicBool},
};

use crate::launch_profile::LaunchProfile;

use super::{
    BrowserState, BrowserTabs, PopupRequests, annotate, downloads,
    profile::{browser_profile_dir, validate_project_roots},
    site_permissions,
};

impl BrowserState {
    pub(crate) fn mark_agent_download_provenance(&self, tab_id: &str) {
        if let Ok(mut downloads) = self.downloads.lock() {
            downloads.set_provenance(tab_id, downloads::NavigationProvenance::Agent);
        }
    }

    pub(crate) fn new(
        profile: &LaunchProfile,
        ui_webview_dir: PathBuf,
        active_project_roots: &[PathBuf],
        next_number: u64,
    ) -> Result<Self, String> {
        if next_number == 0 {
            return Err("browser tab labels exhausted".into());
        }
        let profile_dir = browser_profile_dir(profile, ui_webview_dir, active_project_roots)?;
        let mut tabs = BrowserTabs::new();
        tabs.next_number = next_number;
        Ok(Self {
            profile_dir,
            tabs: Mutex::new(tabs),
            downloads: Mutex::new(downloads::DownloadRegistry::default()),
            permissions: Mutex::new(site_permissions::SitePermissions::default()),
            popups: Mutex::new(PopupRequests::default()),
            annotate: Mutex::new(annotate::AnnotateMode::default()),
            clearing_data: AtomicBool::new(false),
        })
    }

    pub(crate) fn validate_project_roots(&self, roots: &[PathBuf]) -> Result<(), String> {
        validate_project_roots(
            &self.profile_dir,
            &roots.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        )
    }
}
