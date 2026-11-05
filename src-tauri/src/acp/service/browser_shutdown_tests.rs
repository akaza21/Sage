use super::*;
use crate::acp::browser::BrowserScope;
use std::sync::{Arc, Mutex};

#[test]
fn chat_and_app_exit_end_user_and_agent_grants_even_with_retained_leases() {
    for path in ["chat close", "shutdown", "elapsed shutdown budget", "drop"] {
        let (service, owner) =
            super::super::permission::tests::support::service_with_pending_requests(Arc::new(
                Mutex::new(Vec::new()),
            ));
        let registry = &service.browser_attachments;
        let (project, chat, session) =
            (&owner.project_id, &owner.chat_id, owner.session_id.as_str());
        registry
            .attach_agent(
                project.clone(),
                chat.clone(),
                session,
                "browser-8",
                "https://example.com",
                1,
            )
            .unwrap();
        registry
            .attach(
                project.clone(),
                chat.clone(),
                "browser-8",
                "https://example.com",
                1,
            )
            .unwrap();
        let leases = [
            registry.resolve(project, chat).unwrap(),
            registry.agent_resolve(project, chat, session, 8).unwrap(),
        ];
        let mut native_states = Vec::new();
        for lease in &leases {
            lease
                .authority_for(&BrowserScope {
                    session_id: session.into(),
                    tab_id: 8,
                })
                .unwrap();
            assert!(lease.grant_developer_mode(session, "https://example.com"));
            let native = Arc::new(String::from("privileged native session"));
            native_states.push(Arc::downgrade(&native));
            let held = Mutex::new(Some(native));
            lease
                .developer_scope()
                .bind(Arc::new(move |force| {
                    if force {
                        held.lock().unwrap().take();
                    }
                }))
                .unwrap();
        }
        assert!(
            service
                .browser_agent_control("browser-8")
                .unwrap()
                .developer_mode
        );
        let mut service = Some(service);
        match path {
            "chat close" => service.as_ref().unwrap().revoke_browser_chat(project, chat),
            "shutdown" => service.as_ref().unwrap().shutdown_blocking().unwrap(),
            "elapsed shutdown budget" => service
                .as_ref()
                .unwrap()
                .close_agent_tabs_for_shutdown(Instant::now()),
            "drop" => drop(service.take()),
            _ => unreachable!(),
        }
        for lease in &leases {
            assert!(
                !lease.developer_granted(session, "https://example.com"),
                "{path}"
            );
            assert!(!lease.is_live(), "{path}");
        }
        assert!(
            native_states.iter().all(|state| state.upgrade().is_none()),
            "{path}: native state released before exit"
        );
        if let Some(service) = service {
            assert!(
                service.browser_agent_control("browser-8").is_none(),
                "{path}"
            );
            if path != "shutdown" {
                service.shutdown_blocking().unwrap();
            }
        }
    }
}
