use super::*;
use crate::acp::browser::{
    BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest, BrowserResult, BrowserScope,
    attachment::BrowserAttachments,
    permission::{
        BrowserCapabilityDispatch, BrowserDispatchPermit, BrowserPermissionChoice,
        BrowserPermissionGate, BrowserPermissionPrompt, PendingBrowserResult,
    },
};
use sage_acp_protocol::contract::{SageChatId, SageProjectId};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn clear_closes_selected_tabs_and_releases_their_history() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    {
        let mut tabs = tabs.lock().unwrap();
        tabs.insert("browser-1".into(), 1, false);
        assert!(tabs.start_navigation("browser-1", 1, "https://a.example/"));
        assert!(tabs.finish_navigation("browser-1", 1, Some("https://a.example/")));
        assert!(tabs.start_navigation("browser-1", 2, "https://b.example/"));
        assert!(tabs.finish_navigation("browser-1", 2, Some("https://b.example/")));
        assert_eq!(tabs.history_len("browser-1"), Some(2));
    }
    let clearing = AtomicBool::new(false);
    for label in clear_data::begin_clear(&tabs, &clearing).unwrap() {
        tabs.lock().unwrap().close_with(&label, |_| Ok(())).unwrap();
    }
    let mut tabs = tabs.lock().unwrap();
    assert_eq!(tabs.history_len("browser-1"), None);
    let next = tabs.reserve_label().unwrap();
    tabs.insert(next.clone(), 2, false);
    assert_eq!(tabs.history_len(&next), Some(0));
}

struct SnapshotPort;

impl BrowserCapabilityDispatch for SnapshotPort {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        assert!(matches!(request.command, BrowserCommand::SnapshotPage {}));
        let result = permit.with_live_lock(|| {
            Ok(BrowserResult::SnapshotPage {
                navigation_epoch: 1,
                text: "safe".into(),
                nodes: vec![],
            })
        })?;
        Ok(Box::new(move || Ok(result)))
    }
}

#[test]
fn clear_revokes_the_attached_agents_origin_grant() {
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-7",
            "https://a.example/",
            1,
        )
        .unwrap();
    let scope = BrowserScope {
        session_id: "session-one".into(),
        tab_id: 7,
    };
    let authority = registry
        .resolve(&project, &chat)
        .unwrap()
        .authority_for(&scope)
        .unwrap();
    let decisions = AtomicUsize::new(0);
    let mut gate = BrowserPermissionGate::new(
        SnapshotPort,
        |_: &BrowserPermissionPrompt| {
            decisions.fetch_add(1, Ordering::SeqCst);
            BrowserPermissionChoice::AllowForOriginSession
        },
        authority,
    );
    let request = BrowserRequest {
        scope,
        command: BrowserCommand::SnapshotPage {},
    };
    assert!(gate.perform(&request).is_ok());
    assert!(gate.perform(&request).is_ok());
    assert_eq!(decisions.load(Ordering::SeqCst), 1);
    registry.revoke_tab("browser-7");
    assert_eq!(gate.perform(&request), Err(BrowserFailure::Denied));
    assert!(registry.resolve(&project, &chat).is_none());
}

#[test]
fn browser_clear_profile_is_disjoint_from_preview_ui_storage() {
    let root = std::env::temp_dir().join(format!("sage-clear-policy-{}", uuid::Uuid::new_v4()));
    let ui_preview = root.join("ui-preview");
    let durable = root.join("durable");
    fs::create_dir_all(&ui_preview).unwrap();
    fs::create_dir_all(&durable).unwrap();
    let profile = LaunchProfile::Disposable {
        durable_dir: durable,
        webview_dir: ui_preview.clone(),
        project_root: None,
    };
    let browser = browser_profile_dir(&profile, ui_preview.clone(), &[]).unwrap();
    assert_ne!(browser, ui_preview);
    assert!(!browser.starts_with(&ui_preview));
    assert!(!ui_preview.starts_with(&browser));
    assert_eq!(profile.webview_dir(), Some(ui_preview.as_path()));
    fs::remove_dir_all(root).unwrap();
}
