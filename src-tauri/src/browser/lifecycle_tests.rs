use super::*;

#[tokio::test]
async fn timed_out_close_blocks_open_and_clear_until_late_acknowledgment() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let clearing_data = AtomicBool::new(false);
    let quarantine = tabs.lock().unwrap().teardown.clone();
    quarantine.begin("browser-1").unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    assert_eq!(
        wait_for_native_close(receiver, std::time::Duration::ZERO).await,
        Err(
            "Browser is still closing; restart Sage before browsing again: native close timed out"
                .into()
        )
    );
    assert_eq!(
        open_with(&tabs, &clearing_data, false, |_| Ok(1)).err(),
        Some("Browser is still closing, try again".into())
    );
    assert_eq!(
        super::super::clear_data::begin_clear(&tabs, &clearing_data).err(),
        Some("Browser is still closing, try again".into())
    );
    complete_native_close(&tabs, &quarantine, "browser-1", Ok(()), sender, |_| {});
    let (_, reservation) = open_with(&tabs, &clearing_data, false, |_| Ok(1)).unwrap();
    drop(reservation);
}

#[tokio::test]
async fn late_success_reconciles_timed_out_tab_and_publishes_closure() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    tabs.lock().unwrap().insert("browser-1".into(), 1, false);
    let quarantine = tabs.lock().unwrap().teardown.clone();
    quarantine.begin("browser-1").unwrap();
    let clearing_data = AtomicBool::new(false);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let close = close_with_unlocked(&tabs, "browser-1", |_| async {
        wait_for_native_close(receiver, std::time::Duration::ZERO).await
    });
    assert!(close.await.is_err());
    let downloads = Mutex::new(downloads::DownloadRegistry::default());
    downloads
        .lock()
        .unwrap()
        .set_provenance("browser-1", downloads::NavigationProvenance::Agent);
    let permissions = Mutex::new(site_permissions::SitePermissions::default());
    permissions.lock().unwrap().cancel_tab("browser-1");
    let mut revoked = Vec::new();
    let mut events = Vec::new();
    complete_native_close(&tabs, &quarantine, "browser-1", Ok(()), sender, |label| {
        finish_browser_tab_close(&downloads, &permissions, label, |id| {
            revoked.push(id.to_owned())
        });
        events.push(label.to_owned());
    });
    assert!(!tabs.lock().unwrap().contains("browser-1"));
    assert_eq!(events, ["browser-1"]);
    assert_eq!(revoked, ["browser-1"]);
    assert!(!downloads.lock().unwrap().is_agent("browser-1"));
    assert!(matches!(
        permissions.lock().unwrap().request(
            "browser-1",
            "https://example.com/",
            site_permissions::PermissionKind::Camera,
            false,
        ),
        site_permissions::RequestOutcome::Prompt(_)
    ));
    let (_, reservation) = open_with(&tabs, &clearing_data, false, |_| Ok(2)).unwrap();
    drop(reservation);
}

#[tokio::test]
async fn failed_close_stays_quarantined_and_requests_restart() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let quarantine = tabs.lock().unwrap().teardown.clone();
    quarantine.begin("browser-1").unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    complete_native_close(
        &tabs,
        &quarantine,
        "browser-1",
        Err("native failure".into()),
        sender,
        |_| {},
    );
    let error = wait_for_native_close(receiver, std::time::Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(quarantine.blocked_message().is_some());
    assert!(error.contains("restart Sage"));
    let clearing_data = AtomicBool::new(false);
    assert_eq!(
        open_with(&tabs, &clearing_data, false, |_| Ok(1)).err(),
        Some("Browser is still closing; restart Sage before browsing again".into())
    );
    assert_eq!(
        super::super::clear_data::begin_clear(&tabs, &clearing_data).err(),
        Some("Browser is still closing; restart Sage before browsing again".into())
    );
}

#[test]
fn quarantine_is_bounded_and_clears_after_window_destroy() {
    let quarantine = TeardownQuarantine::default();
    for number in 0..=MAX_BROWSER_TABS {
        quarantine.begin(&format!("browser-{number}")).unwrap();
    }
    assert!(quarantine.begin("one-more").is_err());
    quarantine.clear_after_window_destroy();
    assert!(quarantine.blocked_message().is_none());
}

#[tokio::test]
async fn failed_hide_keeps_open_reserved_until_close_acknowledgment() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let clearing_data = AtomicBool::new(false);
    let (label, reservation) = open_with(&tabs, &clearing_data, false, |_| Ok(1)).unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let cleanup = hide_or_close(
        &tabs,
        &label,
        |_| Err("hide failed".into()),
        |_| async { receiver.await.map_err(|error| error.to_string()) },
    );
    tokio::pin!(cleanup);
    assert!(
        tokio::time::timeout(std::time::Duration::ZERO, &mut cleanup)
            .await
            .is_err()
    );
    assert!(super::super::clear_data::begin_clear(&tabs, &clearing_data).is_err());
    sender.send(()).unwrap();
    assert!(cleanup.await.is_err());
    drop(reservation);
    assert!(super::super::clear_data::begin_clear(&tabs, &clearing_data).is_ok());
}

#[tokio::test]
async fn missing_main_thread_close_acknowledgment_times_out() {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let _ = sender.send(Ok(()));
    });
    let result = wait_for_native_close(receiver, std::time::Duration::ZERO).await;
    assert!(result.unwrap_err().contains("timed out"));
}

#[tokio::test]
async fn missing_main_thread_close_acknowledgment_disconnects() {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    drop(sender);
    assert!(
        wait_for_native_close(receiver, std::time::Duration::ZERO)
            .await
            .is_err()
    );
}

#[test]
fn browser_navigation_rejects_blank_and_admits_https() {
    let initial = Url::parse("about:blank").unwrap();
    assert!(!allowed_browser_navigation(&initial));
    assert!(allowed_browser_navigation(
        &Url::parse("https://example.com/").unwrap()
    ));
}

#[test]
fn open_path_replies_for_url_and_blank() {
    for target in [None, Some(Url::parse("https://example.com/").unwrap())] {
        let mut navigated = Vec::new();
        let reply = complete_browser_open("browser-1".into(), target.clone(), |url| {
            navigated.push(url);
            Ok(())
        });
        assert_eq!(reply.as_deref(), Ok("browser-1"));
        assert_eq!(navigated, target.into_iter().collect::<Vec<_>>());
    }
}

#[tokio::test]
async fn open_close_reopen_never_holds_tab_lock_during_native_operations() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let clearing_data = AtomicBool::new(false);
    let (first, first_opening) = open_with(&tabs, &clearing_data, false, |_| {
        assert!(tabs.try_lock().is_ok());
        Ok(1)
    })
    .unwrap();
    drop(first_opening);
    close_with_unlocked(&tabs, &first, |_| async {
        assert!(tabs.try_lock().is_ok());
        Ok(())
    })
    .await
    .unwrap();
    let (second, second_opening) = open_with(&tabs, &clearing_data, true, |_| {
        assert!(tabs.try_lock().is_ok());
        Ok(2)
    })
    .unwrap();
    drop(second_opening);
    assert_ne!(first, second);
    assert!(tabs.lock().unwrap().contains(&second));
}

#[test]
fn clear_data_gate_rejects_new_tab_before_reservation() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let clearing_data = AtomicBool::new(true);
    assert_eq!(
        open_with(&tabs, &clearing_data, false, |_| panic!(
            "native creation invoked"
        ))
        .err(),
        Some("browser data clearing is in progress".into())
    );
    assert!(tabs.lock().unwrap().opening.is_empty());
}

#[test]
fn clear_cannot_start_while_open_is_registering() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let clearing_data = AtomicBool::new(false);
    let (label, opening) = open_with(&tabs, &clearing_data, true, |_| Ok(1)).unwrap();
    let registration = || -> Result<(), String> {
        assert!(tabs.lock().unwrap().opening.contains(&label));
        assert_eq!(
            super::super::clear_data::begin_clear(&tabs, &clearing_data).err(),
            Some("browser tabs are still opening or closing".into())
        );
        Ok(())
    };
    registration().unwrap();
    drop(opening);
    assert!(tabs.lock().unwrap().opening.is_empty());
    assert!(super::super::clear_data::begin_clear(&tabs, &clearing_data).is_ok());
}

#[test]
fn failed_registration_releases_open_reservation() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let clearing_data = AtomicBool::new(false);
    let result = (|| -> Result<(), String> {
        let (_, _opening) = open_with(&tabs, &clearing_data, true, |_| Ok(1))?;
        assert!(!tabs.lock().unwrap().opening.is_empty());
        Err("registration failed".into())
    })();
    assert_eq!(result, Err("registration failed".into()));
    assert!(tabs.lock().unwrap().opening.is_empty());
    assert!(super::super::clear_data::begin_clear(&tabs, &clearing_data).is_ok());
}

#[tokio::test]
async fn closing_tab_still_counts_against_capacity_until_native_close_finishes() {
    let tabs = Mutex::new(BrowserTabs::<usize>::new());
    let clearing_data = AtomicBool::new(false);
    let (first, first_opening) = open_with(&tabs, &clearing_data, false, |_| Ok(1)).unwrap();
    drop(first_opening);
    for number in 2..=MAX_BROWSER_TABS {
        let (_, opening) = open_with(&tabs, &clearing_data, false, |_| Ok(number)).unwrap();
        drop(opening);
    }
    close_with_unlocked(&tabs, &first, |_| async {
        assert_eq!(
            open_with(&tabs, &clearing_data, false, |_| Ok(99)).err(),
            Some("browser tab limit reached".into())
        );
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(tabs.lock().unwrap().entries.len(), MAX_BROWSER_TABS - 1);
}
