use super::*;
use std::{
    sync::mpsc::sync_channel,
    time::{Duration, Instant},
};

#[test]
fn detaching_never_reports_developer_mode_off_before_the_grant_ends() {
    for agent in [false, true] {
        let registry = Arc::new(BrowserAttachments::default());
        let project = SageProjectId::parse("project-one").unwrap();
        let chat = SageChatId::parse("chat-one").unwrap();
        let scope = BrowserScope {
            session_id: "session-1".into(),
            tab_id: 7,
        };
        let lease = if agent {
            registry
                .attach_agent(
                    project.clone(),
                    chat.clone(),
                    "session-1",
                    "browser-7",
                    "https://example.com",
                    1,
                )
                .unwrap();
            registry
                .agent_resolve(&project, &chat, "session-1", 7)
                .unwrap()
        } else {
            registry
                .attach(
                    project.clone(),
                    chat.clone(),
                    "browser-7",
                    "https://example.com",
                    1,
                )
                .unwrap();
            registry.resolve(&project, &chat).unwrap()
        };
        lease.authority_for(&scope).unwrap();
        assert!(lease.grant_developer_mode("session-1", "https://example.com"));

        // Session replacement holds the authority slot while native resource
        // cleanup waits. Detach must not unpublish its still-live grant then.
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        lease
            .bind_resources(
                lease.resource_generation(),
                || true,
                Box::new(move || {
                    entered_tx.send(()).unwrap();
                    // A modeled native cleanup deadline, not a timing assertion.
                    let _ = release_rx.recv_timeout(Duration::from_secs(5));
                }),
            )
            .unwrap();
        let transferring = lease.clone();
        let transfer = std::thread::spawn(move || {
            transferring.authority_for(&BrowserScope {
                session_id: "session-2".into(),
                tab_id: 7,
            });
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(lease.developer_granted("session-1", "https://example.com"));

        let observing = registry.clone();
        let observed_lease = lease.clone();
        let (ready_tx, ready_rx) = sync_channel(1);
        let observer = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if !observing
                    .agent_control("browser-7")
                    .is_some_and(|c| c.developer_mode)
                {
                    // Read the grant AFTER the first off report: this grant
                    // only ends during the test, so off+live is inconsistent.
                    let live = observed_lease.developer_granted("session-1", "https://example.com");
                    let _ = release_tx.try_send(());
                    return live;
                }
                assert!(Instant::now() < deadline, "detach never reached the chrome");
                std::thread::yield_now();
            }
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        if agent {
            registry.agent_detach(&project, &chat, "session-1", 7);
        } else {
            registry.detach(&project, &chat).unwrap();
        }
        transfer.join().unwrap();
        let hidden_live_grant = observer.join().unwrap();
        assert!(
            !hidden_live_grant,
            "agent={agent}: a live grant disappeared from the chrome"
        );
        assert!(!lease.developer_granted("session-1", "https://example.com"));
        assert!(registry.agent_control("browser-7").is_none());
    }
}
