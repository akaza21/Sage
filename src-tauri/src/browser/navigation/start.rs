use crate::browser::{BrowserTabs, site_permissions::SitePermissions};

pub(super) fn begin<T>(
    tabs: &mut BrowserTabs<T>,
    permissions: &mut SitePermissions,
    tab_id: &str,
    navigation_id: u64,
    uri: &str,
) -> (bool, Vec<u64>) {
    let started = tabs.start_navigation(tab_id, navigation_id, uri);
    permissions.navigation_started(tab_id, navigation_id);
    let denied = permissions.navigation_to(tab_id, uri);
    (started, denied)
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
