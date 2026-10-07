use super::*;
use crate::browser::{
    downloads::{DownloadRegistry, NavigationProvenance},
    site_permissions::{PermissionKind, RequestOutcome, SitePermissions},
};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn blocked_navigation_target_cannot_become_the_committed_page() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, true);
    assert!(tabs.start_navigation("browser-1", 1, "https://safe.example/"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://safe.example/")));
    assert!(tabs.start_navigation("browser-1", 2, "https://safe.example/next"));
    for target in [
        "file:///C:/secret",
        "data:text/html,secret",
        "javascript:alert(1)",
    ] {
        assert!(admit_url(target).is_err());
        assert!(!tabs.finish_navigation("browser-1", 2, Some(target)));
        assert_eq!(
            tabs.snapshot("browser-1").unwrap().committed_url.as_deref(),
            Some("https://safe.example/")
        );
    }
}

#[test]
fn child_page_cannot_invoke_sage_or_open_unadmitted_popup() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let app = Url::parse("http://tauri.localhost/index.html").unwrap();
    let public = Url::parse("https://example.com/").unwrap();
    assert!(ipc_guard::admits_main_invoke("main", &app));
    assert!(!ipc_guard::admits_main_invoke("browser-1", &app));
    assert!(!ipc_guard::admits_main_invoke("browser-1", &public));
    assert_eq!(
        popup_target(&tabs, "browser-1", &public).as_deref(),
        Some("https://example.com/")
    );
    assert!(
        popup_target(
            &tabs,
            "browser-1",
            &Url::parse("file:///C:/secret").unwrap()
        )
        .is_none()
    );
    assert!(
        popup_target(
            &tabs,
            "browser-1",
            &Url::parse("https://user:pass@example.com/").unwrap()
        )
        .is_none()
    );
}

#[test]
fn agent_page_cannot_prompt_or_start_a_download() {
    let mut permissions = SitePermissions::default();
    assert!(matches!(
        permissions.request(
            "browser-1",
            "https://example.com/",
            PermissionKind::Camera,
            true
        ),
        RequestOutcome::Deny
    ));
    assert!(permissions.pending("browser-1").is_empty());
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::Agent);
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_err()
    );
    assert!(downloads.views("browser-1").is_empty());
}

#[test]
fn clear_admits_only_browser_tabs_and_uses_a_distinct_profile() {
    let root = std::env::temp_dir().join(format!("sage-browser-policy-{}", uuid::Uuid::new_v4()));
    let ui = root.join("ui");
    let durable = root.join("durable");
    fs::create_dir_all(&ui).unwrap();
    fs::create_dir_all(&durable).unwrap();
    let profile = LaunchProfile::Disposable {
        durable_dir: durable,
        webview_dir: ui.clone(),
        project_root: None,
    };
    let browser_dir = browser_profile_dir(&profile, ui.clone(), &[]).unwrap();
    assert_ne!(browser_dir, ui);
    assert!(!browser_dir.starts_with(&ui));
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    tabs.lock().unwrap().insert("browser-1".into(), 1, false);
    let clearing = AtomicBool::new(false);
    assert_eq!(
        clear_data::begin_clear(&tabs, &clearing).unwrap(),
        vec!["browser-1"]
    );
    assert!(clearing.load(Ordering::Acquire));
    assert!(ui.is_dir());
    fs::remove_dir_all(root).unwrap();
}
