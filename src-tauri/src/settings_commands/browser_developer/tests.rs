use super::*;
use crate::acp::{
    browser::{BrowserScope, attachment::BrowserAttachments},
    contract::{SageChatId, SageProjectId},
};
use crate::settings_store::SettingsStore;
use std::sync::{Arc, Mutex};

#[test]
fn a_failed_switch_write_still_releases_the_old_grant_and_native_session() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../work");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("developer-switch-{}.db", uuid::Uuid::new_v4()));
    let settings = SettingsStore::open(&path).unwrap();
    settings.set_browser_developer_mode(true).unwrap();
    rusqlite::Connection::open(&path).unwrap().execute_batch(
        "CREATE TRIGGER refuse_switch BEFORE INSERT ON browser_developer_mode BEGIN SELECT RAISE(ABORT, 'unwritable switch'); END;"
    ).unwrap();
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-7",
            "https://example.com/",
            1,
        )
        .unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    lease
        .authority_for(&BrowserScope {
            session_id: "session-1".into(),
            tab_id: 7,
        })
        .unwrap();
    assert!(lease.grant_developer_mode("session-1", "https://example.com"));
    let state = Arc::new(vec![0_u8; 1024]);
    let weak = Arc::downgrade(&state);
    let held = Mutex::new(Some(state));
    lease
        .developer_scope()
        .bind(Arc::new(move |_| {
            held.lock().unwrap().take();
        }))
        .unwrap();
    let result = persist_after_revocation(&settings, false, || {
        registry.end_developer_mode();
    });
    assert!(result.is_err());
    assert!(settings.browser_developer_mode().unwrap());
    assert!(!lease.developer_mode_active());
    assert!(weak.upgrade().is_none());
    drop((registry, lease, settings));
    std::fs::remove_file(path).unwrap();
}
