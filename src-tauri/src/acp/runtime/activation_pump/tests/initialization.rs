use super::*;

#[test]
fn initialization_followups_are_bounded_and_control_egress_is_rejected() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let limit = LIMITS.queues.outstanding_rpc_per_session as usize;
        let batches = (1..=limit)
            .map(|request_id| {
                AdapterBatch::new(
                    vec![],
                    vec![AdapterEgress::new(
                        crate::acp::transport::OutboundFrame::new(b"{}".to_vec()).unwrap(),
                        EgressToken::request(request_id as u64 + 1),
                    )],
                )
                .unwrap()
            })
            .collect();
        let (journal, commands) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(1).unwrap(),
        }));
        let mut pump = ActivationPump::new(
            adapter(advertised.clone(), batches),
            journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        pump.flush_one(&mut transport).unwrap();
        for request_id in 1..limit {
            push_frame(&mut pump, request_id as i64).await.unwrap();
            assert_eq!(
                pump.flush_one(&mut transport).unwrap(),
                ActivationFlush::Written
            );
            assert!(!pump.completion_ready());
        }
        assert!(matches!(
            push_frame(&mut pump, limit as i64).await,
            Err(ActivationPumpError::Capacity)
        ));
        assert!(pump.teardown_required());
        assert!(commands.lock().unwrap().is_empty());

        for token in [
            EgressToken::cancel_request(2),
            EgressToken::protocol_notification(),
        ] {
            let batch = AdapterBatch::new(
                vec![],
                vec![AdapterEgress::new(
                    crate::acp::transport::OutboundFrame::new(b"{}".to_vec()).unwrap(),
                    token,
                )],
            )
            .unwrap();
            let (journal, commands) = super::journal(Ok(ActivationReceipt {
                projection_revision: ProjectionRevision::new(1).unwrap(),
            }));
            let mut pump = ActivationPump::new(
                adapter(advertised.clone(), vec![batch]),
                journal,
                NoResources(17),
                intent(ActivationMode::New {
                    native_root: "/repo".into(),
                    model: None,
                }),
            )
            .unwrap();
            pump.begin().unwrap();
            pump.flush_one(&mut transport).unwrap();
            assert!(matches!(
                push_frame(&mut pump, 1).await,
                Err(ActivationPumpError::Intent)
            ));
            assert!(pump.teardown_required());
            assert!(commands.lock().unwrap().is_empty());
        }
    });
}

fn policy_request(id: u64) -> AdapterEgress {
    AdapterEgress::new(
        crate::acp::transport::OutboundFrame::new(format!("{{\"id\":{id}}}").into_bytes()).unwrap(),
        EgressToken::request(id),
    )
}

type ActivatedPump = (
    ActivationPump<FakeAdapter, JournalState, NoResources>,
    Arc<Mutex<Vec<CompleteActivationCommand>>>,
    Result<bool, ActivationPumpError>,
);

/// Drives a new session through initialize and `session/new`, then feeds
/// `after_session` as the adapter's batches once the session answered.
async fn activate_with(after_session: Vec<AdapterBatch>) -> ActivatedPump {
    let advertised = capabilities(true, true);
    let mut batches = vec![
        AdapterBatch::new(
            vec![AdapterEvent::Initialized {
                capabilities: advertised.clone(),
            }],
            vec![],
        )
        .unwrap(),
    ];
    batches.extend(after_session);
    let (journal, commands) = journal(Ok(ActivationReceipt {
        projection_revision: ProjectionRevision::new(1).unwrap(),
    }));
    let mut pump = ActivationPump::new(
        adapter(advertised, batches),
        journal,
        NoResources(17),
        intent(ActivationMode::New {
            native_root: "/repo".into(),
            model: None,
        }),
    )
    .unwrap();
    let mut transport = Transport(VecDeque::new());
    pump.begin().unwrap();
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        ActivationFlush::Written
    );
    push_frame(&mut pump, 1).await.unwrap();
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        ActivationFlush::Written,
        "session/new is written"
    );
    let answer = push_frame(&mut pump, 2).await;
    (pump, commands, answer)
}

#[test]
fn one_declared_policy_step_is_admitted_between_the_session_answer_and_ready() {
    runtime().block_on(async {
        let epoch = RuntimeEpoch::new(7).unwrap();
        let (mut pump, commands, answer) = activate_with(vec![
            AdapterBatch::new(
                vec![AdapterEvent::SessionPolicyPending],
                vec![policy_request(3)],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: identity("external-1", epoch),
                    loaded: false,
                }],
                vec![],
            )
            .unwrap(),
        ])
        .await;
        answer.unwrap();
        assert!(
            commands.lock().unwrap().is_empty(),
            "not ready before the step"
        );
        let mut transport = Transport(VecDeque::new());
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written,
            "the policy request is written"
        );
        push_frame(&mut pump, 3).await.unwrap();
        assert_eq!(commands.lock().unwrap().len(), 1);
    });
}

#[test]
fn an_undeclared_request_after_the_session_answer_is_refused() {
    runtime().block_on(async {
        let (pump, commands, answer) = activate_with(vec![
            AdapterBatch::new(vec![], vec![policy_request(3)]).unwrap(),
        ])
        .await;
        assert!(matches!(answer, Err(ActivationPumpError::Intent)));
        assert!(pump.teardown_required());
        assert!(commands.lock().unwrap().is_empty());
    });
}

#[test]
fn a_second_policy_step_or_a_step_without_one_request_is_refused() {
    runtime().block_on(async {
        let (mut pump, commands, answer) = activate_with(vec![
            AdapterBatch::new(
                vec![AdapterEvent::SessionPolicyPending],
                vec![policy_request(3)],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionPolicyPending],
                vec![policy_request(4)],
            )
            .unwrap(),
        ])
        .await;
        answer.unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.flush_one(&mut transport).unwrap();
        assert!(matches!(
            push_frame(&mut pump, 3).await,
            Err(ActivationPumpError::Intent)
        ));
        assert!(pump.teardown_required());
        assert!(commands.lock().unwrap().is_empty());

        for egress in [vec![], vec![policy_request(3), policy_request(4)]] {
            let (pump, commands, answer) = activate_with(vec![
                AdapterBatch::new(vec![AdapterEvent::SessionPolicyPending], egress).unwrap(),
            ])
            .await;
            assert!(matches!(answer, Err(ActivationPumpError::Intent)));
            assert!(pump.teardown_required());
            assert!(commands.lock().unwrap().is_empty());
        }
    });
}
