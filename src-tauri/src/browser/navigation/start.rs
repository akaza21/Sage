use crate::browser::{BrowserTabs, site_permissions::SitePermissions};

pub(super) fn begin<T>(
    tabs: &mut BrowserTabs<T>,
    permissions: &mut SitePermissions,
    tab_id: &str,
    navigation_id: u64,
    uri: &str,
) -> (bool, Vec<u64>) {
    let started = tabs.start_navigation(tab_id, navigation_id, uri);
    // A redirect repeats `NavigationStarting` with the same id and is not a
    // new start: a navigation already under way when an Allow ended must not
    // become the one whose commit clears the stale mark.
    if started {
        permissions.navigation_started(tab_id, navigation_id);
    }
    let denied = permissions.navigation_to(tab_id, uri);
    (started, denied)
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
