use super::*;
use crate::acp::browser::BrowserFailure;

#[derive(Clone)]
struct FakeRegistration(Arc<Mutex<Vec<&'static str>>>, &'static str);

impl Subscription for FakeRegistration {
    fn unregister(&self) {
        self.0.lock().unwrap().push(self.1);
    }
}

#[test]
fn repeated_attachment_reuses_the_tab_ring_without_new_registration() {
    let removed = Arc::new(Mutex::new(Vec::new()));
    let mut table = SubscriptionTable::<FakeRegistration>::default();
    let cancelled = AtomicBool::new(false);
    let first = table
        .attach_once("browser-1", &cancelled, |event, _| {
            Ok(FakeRegistration(removed.clone(), event))
        })
        .unwrap();
    first.state.store(ACTIVE, Ordering::Release);
    let second = table
        .attach_once("browser-1", &cancelled, |_, _| {
            panic!("existing tab must not register again")
        })
        .unwrap();
    assert!(Arc::ptr_eq(&first.rings, &second.rings));
    assert!(second.existing);
    assert_eq!(table.len(), 1);
}

#[test]
fn failed_attachment_unregisters_all_prior_events() {
    let removed = Arc::new(Mutex::new(Vec::new()));
    let mut table = SubscriptionTable::<FakeRegistration>::default();
    let cancelled = AtomicBool::new(false);
    let result = table.attach_once("browser-1", &cancelled, |event, _| {
        if event == EVENTS[2] {
            return Err(BrowserFailure::Unavailable);
        }
        Ok(FakeRegistration(removed.clone(), event))
    });
    assert!(matches!(result, Err(BrowserFailure::Unavailable)));
    assert_eq!(*removed.lock().unwrap(), vec![EVENTS[1], EVENTS[0]]);
    assert_eq!(table.len(), 0);
}

#[test]
fn detach_and_drop_unregister_every_event_once() {
    let removed = Arc::new(Mutex::new(Vec::new()));
    let mut table = SubscriptionTable::<FakeRegistration>::default();
    let cancelled = AtomicBool::new(false);
    table
        .attach_once("browser-1", &cancelled, |event, _| {
            Ok(FakeRegistration(removed.clone(), event))
        })
        .unwrap();
    table.detach("browser-1");
    table.detach("browser-1");
    assert_eq!(removed.lock().unwrap().len(), EVENTS.len());
    table
        .attach_once("browser-2", &cancelled, |event, _| {
            Ok(FakeRegistration(removed.clone(), event))
        })
        .unwrap();
    drop(table);
    assert_eq!(removed.lock().unwrap().len(), EVENTS.len() * 2);
}

#[test]
fn timed_out_attempt_cannot_leave_late_registrations() {
    let removed = Arc::new(Mutex::new(Vec::new()));
    let cancelled = AtomicBool::new(false);
    let mut table = SubscriptionTable::<FakeRegistration>::default();
    let result = table.attach_once("browser-1", &cancelled, |event, _| {
        if event == EVENTS[1] {
            cancelled.store(true, Ordering::Release);
        }
        Ok(FakeRegistration(removed.clone(), event))
    });
    assert!(matches!(result, Err(BrowserFailure::Unavailable)));
    assert_eq!(table.len(), 0);
    assert_eq!(removed.lock().unwrap().len(), 2);
}

#[test]
fn failed_enable_is_not_reused_and_retry_replaces_it() {
    let removed = Arc::new(Mutex::new(Vec::new()));
    let cancelled = AtomicBool::new(false);
    let mut table = SubscriptionTable::<FakeRegistration>::default();
    let failed = table
        .attach_once("browser-1", &cancelled, |event, _| {
            Ok(FakeRegistration(removed.clone(), event))
        })
        .unwrap();
    failed.state.store(FAILED, Ordering::Release);
    let retry = table
        .attach_once("browser-1", &cancelled, |event, _| {
            Ok(FakeRegistration(removed.clone(), event))
        })
        .unwrap();
    assert!(!retry.existing);
    assert!(!Arc::ptr_eq(&failed.rings, &retry.rings));
    assert_eq!(removed.lock().unwrap().len(), EVENTS.len());
    table.detach_if("browser-1", &failed.state);
    assert_eq!(table.len(), 1);
}

#[test]
fn a_stale_sleep_registration_cannot_reuse_or_remove_a_woken_ring() {
    use crate::acp::{
        browser::attachment::BrowserAttachments,
        contract::{SageChatId, SageProjectId},
    };
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry
        .attach(
            project.clone(),
            chat.clone(),
            "browser-1",
            "https://example.com/",
            1,
        )
        .unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    let removed = Arc::new(Mutex::new(Vec::new()));
    let mut table = SubscriptionTable::<FakeRegistration>::default();
    let cancelled = AtomicBool::new(false);
    let old_scope = lease.diagnostic_scope().unwrap();
    let old = table
        .attach_scoped_once(
            "browser-1",
            Some(old_scope.clone()),
            &cancelled,
            DiagnosticRings::default(),
            |event, _| Ok(FakeRegistration(removed.clone(), event)),
        )
        .unwrap();
    old.state.store(ACTIVE, Ordering::Release);
    lease.sleep_diagnostics(None);
    let current = table
        .attach_scoped_once(
            "browser-1",
            lease.diagnostic_scope(),
            &cancelled,
            DiagnosticRings::default(),
            |event, _| Ok(FakeRegistration(removed.clone(), event)),
        )
        .unwrap();
    current.state.store(ACTIVE, Ordering::Release);
    assert!(
        old_scope
            .with_registered_diagnostics(|| table
                .attach_scoped_once(
                    "browser-1",
                    Some(old_scope.clone()),
                    &cancelled,
                    DiagnosticRings::default(),
                    |event, _| Ok(FakeRegistration(removed.clone(), event))
                )
                .map(|lease| (lease, None)))
            .is_err()
    );
    table.detach_if("browser-1", &old.state);
    ingest(
        &current.rings,
        "Runtime.consoleAPICalled",
        r#"{"type":"log","args":[{"value":"after wake"}]}"#,
    );
    assert_eq!(
        current.rings.lock().unwrap().console(None, 1).unwrap().1[0].text,
        "after wake"
    );
    assert!(
        old.rings
            .lock()
            .unwrap()
            .console(None, 1)
            .unwrap()
            .1
            .is_empty()
    );
}

#[test]
fn domain_setup_completion_cannot_reactivate_a_suspended_subscription() {
    let removed = Arc::new(Mutex::new(Vec::new()));
    let mut table = SubscriptionTable::<FakeRegistration>::default();
    let lease = table
        .attach_once("browser-1", &AtomicBool::new(false), |event, _| {
            Ok(FakeRegistration(removed.clone(), event))
        })
        .unwrap();
    table.detach("browser-1");
    assert!(lease.activate().is_err());
    ingest(
        &lease.rings,
        "Runtime.consoleAPICalled",
        r#"{"type":"log","args":[{"value":"late setup"}]}"#,
    );
    assert!(
        lease
            .rings
            .lock()
            .unwrap()
            .console(None, 1)
            .unwrap()
            .1
            .is_empty()
    );
}
