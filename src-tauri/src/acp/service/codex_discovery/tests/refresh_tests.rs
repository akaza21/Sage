use super::*;
use crate::acp::bindings::codex::{
    AdmittedDiscovery, CodexAccountSnapshot, CodexDiscoveryContent, CodexModelSnapshot,
};

fn content(account: CodexAccountSnapshot) -> CodexDiscoveryContent {
    if account == CodexAccountSnapshot::SignedOut {
        return CodexDiscoveryContent {
            account,
            models: vec![],
            default_model_id: None,
        };
    }
    CodexDiscoveryContent {
        account,
        models: vec![CodexModelSnapshot {
            id: RequiredText::new("modelId", "refreshed-model").unwrap(),
            name: RequiredText::new("modelName", "Refreshed model").unwrap(),
            is_default: true,
            effort_levels: vec![],
        }],
        default_model_id: Some(RequiredText::new("modelId", "refreshed-model").unwrap()),
    }
}

#[test]
fn a_new_project_generation_joins_running_discovery_and_receives_its_own_scope() {
    let (manager, owner, _joined) =
        admitted_joined_flight("generation-join", CodexDiscoveryDeadline::new().unwrap());
    manager.cache.clear();
    let mut request = owner.request.clone();
    request.project_generation = ProjectGeneration::new(30).unwrap();
    let root = owner.project_root.as_ref().unwrap();
    let next_root = CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            request.project_id.clone(),
            request.project_generation,
            root.root_id().clone(),
            root.as_path(),
        )
        .unwrap(),
    );
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let running = Arc::clone(&manager);
    let worker = thread::spawn(move || {
        running.discover_with(owner, |binding, root, epoch, _, _| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(AdmittedDiscovery::for_test(
                binding,
                root,
                epoch,
                content(CodexAccountSnapshot::ManagedChatgpt),
            ))
        })
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let next = manager.prepare(
        request.clone(),
        next_root,
        CodexDiscoveryDeadline::new().unwrap(),
    );
    // Always release the running probe, including on the regression's Busy path.
    release_tx.send(()).unwrap();
    let original = worker.join().unwrap().unwrap();
    let response = manager
        .discover(next.expect("a layout generation must join, not return Busy"))
        .unwrap();
    assert_eq!(
        original.project_generation,
        ProjectGeneration::new(29).unwrap()
    );
    assert_eq!(response.project_generation, request.project_generation);
    assert_eq!(response.runtime_epoch, original.runtime_epoch);
    assert_eq!(response.models, original.models);
    assert_eq!(response.models[0].id.as_str(), "refreshed-model");
    assert!(manager.gate.prepare().is_ok());
}

#[test]
fn a_refresh_cancelled_before_control_registration_preserves_the_cached_catalog() {
    let (manager, owner, _joined) = admitted_joined_flight(
        "refresh-cancel-before-control",
        CodexDiscoveryDeadline::new().unwrap(),
    );
    let root = owner.project_root.as_ref().unwrap().clone_for_test();
    let mut request = owner.request.clone();
    manager.discover(owner).unwrap();
    let digest = manager.registration.refresh().unwrap().binding_digest();
    request.refresh = true;
    let refresh = manager
        .prepare(
            request.clone(),
            root.clone_for_test(),
            CodexDiscoveryDeadline::new().unwrap(),
        )
        .unwrap();
    let cancel = manager.cancellation(&refresh);

    let result = manager.discover_with(refresh, |_, _, _, _, active| {
        cancel();
        active.register_cancellation(TestCancellation::new(false))?;
        unreachable!("cancelled discovery must reject process registration")
    });

    assert_eq!(result, Err(CodexDiscoveryRunError::Timeout));
    manager
        .registration
        .refresh()
        .expect("a local timeout is not binding drift");
    manager
        .validate_session_model(
            &root,
            &request.binding_id,
            digest,
            Some(&ModelId::new("modelId", "gpt-joined").unwrap()),
        )
        .unwrap();
}

#[test]
fn refresh_rereads_the_account_through_the_real_cache_and_coalesces() {
    let (manager, mut first, joined) =
        admitted_joined_flight("account-refresh", CodexDiscoveryDeadline::new().unwrap());
    // Start with an actual signed-out discovery, not a frontend status mock.
    manager.cache.clear();
    let root = first.project_root.as_ref().unwrap().clone_for_test();
    let mut request = first.request.clone();
    let signed_out = manager
        .discover_with(first, |binding, root, epoch, _, _| {
            Ok(AdmittedDiscovery::for_test(
                binding,
                root,
                epoch,
                content(CodexAccountSnapshot::SignedOut),
            ))
        })
        .unwrap();
    assert_eq!(signed_out.account, CodexDiscoveryAccount::SignedOut);
    assert_eq!(
        manager.discover(joined).unwrap().account,
        CodexDiscoveryAccount::SignedOut
    );
    // Account changes outside Sage. Ordinary discovery still uses the snapshot.
    let cached = manager
        .prepare(
            request.clone(),
            root.clone_for_test(),
            CodexDiscoveryDeadline::new().unwrap(),
        )
        .unwrap();
    assert_eq!(
        manager.discover(cached).unwrap().account,
        CodexDiscoveryAccount::SignedOut
    );
    request.refresh = true;
    first = manager
        .prepare(
            request.clone(),
            root.clone_for_test(),
            CodexDiscoveryDeadline::new().unwrap(),
        )
        .unwrap();
    let joined = manager
        .prepare(
            request.clone(),
            root.clone_for_test(),
            CodexDiscoveryDeadline::new().unwrap(),
        )
        .unwrap();
    let refreshed = manager
        .discover_with(first, |binding, root, epoch, _, _| {
            Ok(AdmittedDiscovery::for_test(
                binding,
                root,
                epoch,
                content(CodexAccountSnapshot::ManagedChatgpt),
            ))
        })
        .unwrap();
    assert_eq!(refreshed.account, CodexDiscoveryAccount::ManagedChatgpt);
    assert_eq!(manager.discover(joined).unwrap(), refreshed);
    let digest = manager.registration.refresh().unwrap().binding_digest();
    let model = ModelId::new("modelId", "refreshed-model").unwrap();
    manager
        .validate_session_model(&root, &request.binding_id, digest, Some(&model))
        .unwrap();

    // A failed refresh must preserve the usable catalog and release admission.
    let failed = manager
        .prepare(
            request.clone(),
            root.clone_for_test(),
            CodexDiscoveryDeadline::new().unwrap(),
        )
        .unwrap();
    assert_eq!(
        manager.discover_with(failed, |_, _, _, _, _| Err(
            CodexDiscoveryRunError::Transport
        )),
        Err(CodexDiscoveryRunError::Transport)
    );
    manager
        .validate_session_model(&root, &request.binding_id, digest, Some(&model))
        .unwrap();
    let login = manager.gate.prepare().unwrap();
    assert!(matches!(
        manager.prepare(
            request.clone(),
            root.clone_for_test(),
            CodexDiscoveryDeadline::new().unwrap()
        ),
        Err(CodexDiscoveryRunError::Busy)
    ));
    manager
        .validate_session_model(&root, &request.binding_id, digest, Some(&model))
        .unwrap();
    drop(login);
    // A new refusal also needs a fresh read of a previously signed-in snapshot.
    let refused = manager
        .prepare(request, root, CodexDiscoveryDeadline::new().unwrap())
        .unwrap();
    assert_eq!(
        manager
            .discover_with(refused, |binding, root, epoch, _, _| {
                Ok(AdmittedDiscovery::for_test(
                    binding,
                    root,
                    epoch,
                    content(CodexAccountSnapshot::SignedOut),
                ))
            })
            .unwrap()
            .account,
        CodexDiscoveryAccount::SignedOut
    );
}
