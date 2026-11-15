use super::*;
use crate::acp::{
    browser::{
        BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest, BrowserScope,
        FakeBrowserCapability,
        mcp::{
            BrowserMcpError, BrowserMcpServer, BrowserMcpToken,
            transport::{self, BrowserMcpSessionState, MessagePipe},
        },
        permission::{
            BrowserAuthority, BrowserCapabilityDispatch, BrowserDispatchPermit,
            BrowserPermissionChoice, BrowserPermissionDecider, BrowserPermissionGate,
            BrowserPermissionPrompt, PendingBrowserResult,
        },
    },
    store::{AcpStore, DatabaseAcpStore, StoreCommit, StoreError, StoreMutation},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

struct ProbePipe<P> {
    pipe: P,
    deadline: Instant,
}

impl<P: MessagePipe> MessagePipe for ProbePipe<P> {
    fn read(&mut self, maximum: usize) -> Result<Option<Vec<u8>>, BrowserMcpError> {
        if Instant::now() >= self.deadline {
            return Err(BrowserMcpError::Unavailable);
        }
        self.pipe.read(maximum)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), BrowserMcpError> {
        self.pipe.write(bytes)
    }
}

struct SnapshotPort(FakeBrowserCapability);
impl BrowserCapabilityDispatch for SnapshotPort {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let result = permit.with_live_lock(|| {
            permit.mark_issued();
            self.0.perform(request)
        })?;
        Ok(Box::new(move || Ok(result)))
    }
}

struct Approval(Arc<AtomicUsize>);
impl BrowserPermissionDecider for Approval {
    fn decide(&mut self, _: &BrowserPermissionPrompt) -> BrowserPermissionChoice {
        self.0.fetch_add(1, Ordering::SeqCst);
        BrowserPermissionChoice::AllowForOriginSession
    }
}

fn snapshot(authority: BrowserAuthority, scope: &BrowserScope, approvals: Arc<AtomicUsize>) {
    let mut browser = FakeBrowserCapability::default();
    browser.open_tab(scope.clone());
    let mut gate =
        BrowserPermissionGate::new(SnapshotPort(browser), Approval(approvals), authority);
    gate.perform(&BrowserRequest {
        scope: scope.clone(),
        command: BrowserCommand::SnapshotPage {},
    })
    .unwrap();
}

fn connects(database: crate::persistence::Database, owner: RuntimeOwner) -> bool {
    let host = transport::BrowserMcpPipeHost::bind().unwrap();
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        owner.session_id.as_str().into(),
        u64::try_from(sage_core::time::now_millis()).unwrap(),
    );
    let environment = host.child_environment(owner.session_id.as_str(), token);
    let stopped = Arc::new(AtomicBool::new(false));
    let ended = stopped.clone();
    let session = owner.session_id.clone();
    let worker = std::thread::spawn(move || {
        host.serve(&mut server, session.as_str(), || {
            if ended.load(Ordering::SeqCst) {
                BrowserMcpSessionState::Ended
            } else {
                crate::acp::service::browser_mcp::session_state(&database, &owner)
            }
        })
    });
    let result = transport::connect_for_test(&environment).and_then(|pipe| {
        let mut pipe = ProbePipe {
            pipe,
            deadline: Instant::now() + Duration::from_secs(2),
        };
        let mut input =
            &b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}\n"[..];
        let mut output = Vec::new();
        transport::relay_stdio(
            &mut pipe,
            &mut input,
            &mut output,
            &environment[2].1,
            BrowserMcpToken::from_env_value(environment[3].1.clone()).unwrap(),
        )?;
        Ok(String::from_utf8_lossy(&output).contains("snapshotPage"))
    });
    stopped.store(true, Ordering::SeqCst);
    let _ = worker.join().unwrap();
    result.unwrap_or(false)
}

/// Observe the listener's real database check before activation can finish.
/// Checking only after Ready misses the Recovering interval of a resumed chat.
struct ActivationStore {
    inner: DatabaseAcpStore,
    database: crate::persistence::Database,
    states: mpsc::SyncSender<(BrowserMcpSessionState, bool)>,
}

impl AcpStore for ActivationStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let result = self.inner.commit(batch)?;
        for mutation in batch {
            if let StoreMutation::ActivateSession(command) = mutation {
                let owner = RuntimeOwner {
                    project_id: command.project_id.clone(),
                    chat_id: command.chat_id.clone(),
                    session_id: command.session_id.clone(),
                    runtime_epoch: command.runtime_epoch,
                };
                self.states
                    .try_send((
                        crate::acp::service::browser_mcp::session_state(&self.database, &owner),
                        connects(self.database.clone(), owner),
                    ))
                    .unwrap();
            }
        }
        Ok(result)
    }
}

#[test]
fn browser_take_over_stop_then_next_turn_reconnects() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let mut setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-cancel-reuse",
                "characterization-resume-turn-success",
            ]))
            .await;
        let (states, observed) = mpsc::sync_channel(2);
        let database = setup.service.database.clone();
        bootstrap_support::install_store(
            &mut setup.service,
            ActivationStore {
                inner: DatabaseAcpStore::new(database.clone()),
                database,
                states,
            },
        );
        let setup = Arc::new(setup);
        let _cleanup = bootstrap_support::Cleanup(setup.clone());
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        assert_eq!(
            observed.recv_timeout(bootstrap_support::WAIT).unwrap(),
            (BrowserMcpSessionState::Live, true)
        );
        setup
            .service
            .send_codex_turn(setup.turn_command(&owner))
            .unwrap();
        let turn_id = setup.wait_for_running_turn(&owner);
        let attach = || {
            setup
                .service
                .attach_browser_tab(
                    setup.project_id.clone(),
                    setup.chat_id.clone(),
                    "browser-1",
                    "https://example.com/",
                    1,
                )
                .unwrap()
        };
        attach();
        let scope = BrowserScope {
            session_id: owner.session_id.as_str().into(),
            tab_id: 1,
        };
        let prompt = BrowserPermissionPrompt {
            scope: scope.clone(),
            origin: "https://example.com".into(),
            operation: "snapshotPage",
        };
        let old = setup
            .service
            .browser_attachment(&setup.project_id, &setup.chat_id)
            .unwrap();
        let authority = old.authority_for(&scope).unwrap();
        let approvals = Arc::new(AtomicUsize::new(0));
        snapshot(authority.clone(), &scope, approvals.clone());
        assert_eq!(approvals.load(Ordering::SeqCst), 1);
        let old_permission = authority.liveness_for(&prompt).unwrap();
        assert!(old_permission());
        assert!(setup.service.take_over_browser_tab("browser-1"));
        assert!(!old_permission());
        setup.service.revoke_browser_tab("browser-1");
        setup
            .service
            .cancel_codex_turn(HostCancelIntent {
                owner: owner.clone(),
                turn_id,
                client_intent_id: ClientIntentId::new(),
                now_millis: sage_core::time::now_millis(),
            })
            .unwrap();
        setup.wait_for_cancelled(&owner);
        setup.wait_for_job_zero();
        assert!(!old.is_live());
        assert!(!setup.service.hand_back_browser_tab("browser-1"));
        assert_eq!(
            crate::acp::service::browser_mcp::session_state(&setup.service.database, &owner),
            BrowserMcpSessionState::Ended
        );

        let resumed = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..owner.clone()
        };
        attach();
        setup.activate(&resumed).await.unwrap();
        assert_eq!(
            observed.recv_timeout(bootstrap_support::WAIT).unwrap(),
            (BrowserMcpSessionState::Live, true),
            "the new browser MCP pipe must survive the resumed activation"
        );
        setup.wait_for_ready(&resumed);
        setup.send_and_wait_completed(&resumed).await;
        assert_eq!(
            crate::acp::service::browser_mcp::session_state(&setup.service.database, &owner),
            BrowserMcpSessionState::Ended,
            "a new epoch never revives the stopped listener"
        );
        let fresh = setup
            .service
            .browser_attachment(&setup.project_id, &setup.chat_id)
            .unwrap();
        assert_eq!(fresh.tab_label, old.tab_label);
        assert!(!fresh.same_attachment(&old));
        assert!(
            fresh
                .authority_for(&scope)
                .unwrap()
                .liveness_for(&prompt)
                .is_some()
        );
        assert!(
            !old_permission(),
            "reattaching never revives the cancelled permission"
        );
        snapshot(
            fresh.authority_for(&scope).unwrap(),
            &scope,
            approvals.clone(),
        );
        assert_eq!(
            approvals.load(Ordering::SeqCst),
            2,
            "the new attachment asks again despite the old origin grant"
        );
        setup
            .service
            .detach_browser_tab(&setup.project_id, &setup.chat_id)
            .unwrap();
        assert!(
            fresh.authority_for(&scope).is_none(),
            "Detach still revokes"
        );
        setup.close(&resumed);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}
