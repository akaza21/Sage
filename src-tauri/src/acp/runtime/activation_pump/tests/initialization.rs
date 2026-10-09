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
